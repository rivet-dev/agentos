use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agentos_native_sidecar::extension::ExtensionSnapshot;
use agentos_native_sidecar::wire::{
    CloseStdinRequest, ExecuteRequest, GuestFilesystemCallRequest, GuestFilesystemOperation,
    GuestRuntimeKind, KillProcessRequest, OwnershipScope, ResizePtyRequest,
    RootFilesystemEntryEncoding, WriteStdinRequest,
};
use agentos_native_sidecar::{
    Extension, ExtensionCallbackCancellation, ExtensionContext, ExtensionFuture,
    ExtensionInterrupt, ExtensionInterruptRequest, ExtensionInterruptResponse, ExtensionResponse,
    SidecarError,
};
use agentos_protocol::generated::v1::{
    AcpAbortPendingRequest, AcpCallback, AcpCallbackResponse, AcpDeliverAgentOutputRequest,
    AcpDeliverAgentStderrRequest, AcpErrorResponse, AcpEvent, AcpPendingAbortReason,
    AcpPermissionCallback, AcpRequest, AcpResponse, AcpRuntimeKind,
};
use agentos_protocol::ACP_EXTENSION_NAMESPACE;
use agentos_sidecar_core::behavior::{cancel_notification, unsupported_inbound_request_response};
use agentos_sidecar_core::host::{AcpHost, AgentOutput, SpawnAgentRequest, SpawnedAgent};
use agentos_sidecar_core::{AcpCore, AcpCoreError, ProjectedAgentLaunch};
use base64::Engine;
use serde_json::{json, Map, Value};
use tokio::sync::{mpsc, Mutex};

const PERMISSION_CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);
const PERMISSION_CALLBACK_CLEANUP_GRACE: Duration = Duration::from_secs(5);
const ACP_CANCEL_METHOD: &str = "session/cancel";
const DEFAULT_ACP_TERMINAL_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;
const MAX_ACP_TERMINAL_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;
const ACP_TERMINAL_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const ACP_CANCEL_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

type NativeCoreProcessRoutes = BTreeMap<(String, String), Arc<Mutex<NativeCoreProcess>>>;

#[derive(Debug, Default)]
pub struct AcpExtension {
    cores: Mutex<BTreeMap<String, Arc<std::sync::Mutex<AcpCore>>>>,
    next_process_id: AtomicUsize,
    next_terminal_id: AtomicUsize,
    terminals: Mutex<BTreeMap<String, NativeAcpTerminal>>,
    core_processes: Mutex<NativeCoreProcessRoutes>,
    core_owners: Mutex<BTreeMap<String, NativeCoreOwner>>,
    permission_waits: Mutex<BTreeMap<NativePermissionWaitKey, ExtensionCallbackCancellation>>,
}

#[derive(Debug, Clone)]
struct NativeCoreOwner {
    connection_id: String,
    wire_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct NativeCoreProcess {
    owner_id: String,
    session_id: Option<String>,
    pending_output: VecDeque<AgentOutput>,
    exit_observed: bool,
    /// The native host bounds this batch with
    /// `NativeSidecarConfig::max_extension_session_cleanup_events` before it is
    /// returned through `dispose_session_resources_wire`.
    pending_cleanup_events: VecDeque<agentos_native_sidecar::wire::EventFrame>,
    cleanup: NativeRouteCleanupProgress,
}

#[derive(Debug, Clone, Default)]
struct NativeRouteCleanupProgress {
    output_buffer_stopped: bool,
    terminals_cleaned: bool,
    session_resources_disposed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NativePermissionWaitKey {
    owner_id: String,
    session_id: String,
    process_id: String,
}

type NativeCoreReply<T> = SyncSender<Result<T, AcpCoreError>>;

enum NativeCoreCommand {
    ResolveAgent {
        id: String,
        reply: NativeCoreReply<Option<ProjectedAgentLaunch>>,
    },
    ListAgents {
        reply: NativeCoreReply<Vec<ProjectedAgentLaunch>>,
    },
    HostToolReference {
        reply: NativeCoreReply<String>,
    },
    SpawnAgent {
        request: SpawnAgentRequest,
        reply: NativeCoreReply<SpawnedAgent>,
    },
    BindSession {
        session_id: String,
        process_id: String,
        reply: NativeCoreReply<()>,
    },
    WriteStdin {
        process_id: String,
        chunk: Vec<u8>,
        reply: NativeCoreReply<()>,
    },
    CloseStdin {
        process_id: String,
        reply: NativeCoreReply<()>,
    },
    PollOutput {
        process_id: String,
        reply: NativeCoreReply<Option<AgentOutput>>,
    },
    KillAgent {
        process_id: String,
        signal: String,
        reply: NativeCoreReply<()>,
    },
    AbortAgent {
        process_id: String,
        reply: NativeCoreReply<()>,
    },
    FinalizeSessionCleanup {
        session_id: String,
        process_id: String,
        reply: NativeCoreReply<()>,
    },
    WaitForExit {
        process_id: String,
        timeout_ms: u64,
        reply: NativeCoreReply<Option<i32>>,
    },
    WriteFile {
        path: String,
        contents: Vec<u8>,
        reply: NativeCoreReply<()>,
    },
    ReadFile {
        path: String,
        reply: NativeCoreReply<Vec<u8>>,
    },
    InboundRequest {
        process_id: String,
        request: Value,
        reply: NativeCoreReply<Value>,
    },
    Finished,
}

struct NativeCoreHost {
    commands: mpsc::Sender<NativeCoreCommand>,
    started_at: Instant,
}

impl NativeCoreHost {
    fn exchange<T>(
        &self,
        command: impl FnOnce(NativeCoreReply<T>) -> NativeCoreCommand,
    ) -> Result<T, AcpCoreError> {
        let (reply, response) = sync_channel(1);
        self.commands
            .blocking_send(command(reply))
            .map_err(|_| AcpCoreError::Execution(String::from("native ACP host broker stopped")))?;
        response.recv().map_err(|_| {
            AcpCoreError::Execution(String::from("native ACP host broker dropped a reply"))
        })?
    }
}

impl AcpHost for NativeCoreHost {
    fn resolve_projected_agent(
        &mut self,
        id: &str,
    ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
        let resolved = self.exchange(|reply| NativeCoreCommand::ResolveAgent {
            id: id.to_string(),
            reply,
        })?;
        Ok(resolved)
    }

    fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::ListAgents { reply })
    }

    fn registered_host_tool_reference(&mut self) -> Result<String, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::HostToolReference { reply })
    }

    fn handle_inbound_request(
        &mut self,
        process_id: &str,
        request: &Value,
    ) -> Result<Value, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::InboundRequest {
            process_id: process_id.to_string(),
            request: request.clone(),
            reply,
        })
    }

    fn spawn_agent(&mut self, request: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::SpawnAgent { request, reply })
    }

    fn bind_session(&mut self, session_id: &str, process_id: &str) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::BindSession {
            session_id: session_id.to_string(),
            process_id: process_id.to_string(),
            reply,
        })
    }

    fn write_stdin(&mut self, process_id: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::WriteStdin {
            process_id: process_id.to_string(),
            chunk: chunk.to_vec(),
            reply,
        })
    }

    fn close_stdin(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::CloseStdin {
            process_id: process_id.to_string(),
            reply,
        })
    }

    fn poll_output(&mut self, process_id: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::PollOutput {
            process_id: process_id.to_string(),
            reply,
        })
    }

    fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::KillAgent {
            process_id: process_id.to_string(),
            signal: signal.to_string(),
            reply,
        })
    }

    fn abort_agent(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::AbortAgent {
            process_id: process_id.to_string(),
            reply,
        })
    }

    fn finalize_session_cleanup(
        &mut self,
        session_id: &str,
        process_id: &str,
    ) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::FinalizeSessionCleanup {
            session_id: session_id.to_string(),
            process_id: process_id.to_string(),
            reply,
        })
    }

    fn wait_for_exit(
        &mut self,
        process_id: &str,
        timeout_ms: u64,
    ) -> Result<Option<i32>, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::WaitForExit {
            process_id: process_id.to_string(),
            timeout_ms,
            reply,
        })
    }

    fn write_file(&mut self, path: &str, contents: &[u8]) -> Result<(), AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::WriteFile {
            path: path.to_string(),
            contents: contents.to_vec(),
            reply,
        })
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AcpCoreError> {
        self.exchange(|reply| NativeCoreCommand::ReadFile {
            path: path.to_string(),
            reply,
        })
    }

    fn now_ms(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Debug)]
struct NativeAcpTerminal {
    ownership: OwnershipScope,
    session_id: String,
    process_id: String,
    output: Vec<u8>,
    truncated: bool,
    output_byte_limit: usize,
    exit_code: Option<i32>,
}

impl AcpExtension {
    pub fn new() -> Self {
        Self::default()
    }

    async fn handle_payload(
        &self,
        ctx: ExtensionContext<'_>,
        payload: &[u8],
    ) -> Result<ExtensionResponse, SidecarError> {
        let mut request = decode_request(payload)?;
        let kind = Self::acp_request_kind(&request);
        let start = std::time::Instant::now();
        tracing::info!(target: "agentos_sidecar::acp_extension", kind, "ext request received");

        if let AcpRequest::AcpCreateSessionRequest(create) = &mut request {
            let mut context = ctx;
            let base_instructions = context.agent_additional_instructions().await?;
            create.additional_instructions = agentos_protocol::combine_additional_instructions(
                base_instructions.as_deref(),
                create.additional_instructions.as_deref(),
            );
            return self
                .handle_decoded_payload(context, request, kind, start)
                .await;
        }

        self.handle_decoded_payload(ctx, request, kind, start).await
    }

    async fn handle_decoded_payload(
        &self,
        ctx: ExtensionContext<'_>,
        request: AcpRequest,
        kind: &'static str,
        start: Instant,
    ) -> Result<ExtensionResponse, SidecarError> {
        use tracing::Instrument as _;

        let work = async move {
            match request {
                AcpRequest::AcpDeliverAgentOutputRequest(_) => AcpHandlerOutput::response(Err(
                    SidecarError::InvalidState(
                        "AcpDeliverAgentOutputRequest is dispatched by the engine/browser resumable path, not the native ACP extension".to_string(),
                    ),
                )),
                AcpRequest::AcpAbortPendingRequest(_) => AcpHandlerOutput::response(Err(
                    SidecarError::InvalidState(
                        "AcpAbortPendingRequest is dispatched by the engine/browser resumable path, not the native ACP extension".to_string(),
                    ),
                )),
                request => self.dispatch_shared_core(ctx, request).await,
            }
        }
        .instrument(tracing::info_span!(
            target: "agentos_sidecar::acp_extension",
            "ext.request",
            kind
        ));

        // Stall watchdog: while the request is in flight, warn periodically so a
        // hang surfaces as a breadcrumb long before the host's 120s frame
        // timeout. This never interrupts the work itself.
        tokio::pin!(work);
        let response = loop {
            tokio::select! {
                result = &mut work => break result,
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                    tracing::warn!(
                        target: "agentos_sidecar::acp_extension",
                        kind,
                        elapsed_ms = start.elapsed().as_millis() as u64,
                        "ext request still pending — possible stall before response frame",
                    );
                }
            }
        };
        tracing::info!(
            target: "agentos_sidecar::acp_extension",
            kind,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "ext request handled",
        );
        let payload = encode_response(response.response.unwrap_or_else(error_response))?;
        ExtensionResponse::with_wire_events(payload, response.events)
    }

    /// Stable label for an ACP request kind, used as a tracing field.
    fn acp_request_kind(request: &AcpRequest) -> &'static str {
        match request {
            AcpRequest::AcpCreateSessionRequest(_) => "create_session",
            AcpRequest::AcpGetSessionStateRequest(_) => "get_session_state",
            AcpRequest::AcpListSessionsRequest(_) => "list_sessions",
            AcpRequest::AcpCloseSessionRequest(_) => "close_session",
            AcpRequest::AcpSessionRequest(_) => "session_request",
            AcpRequest::AcpSetSessionConfigRequest(_) => "set_session_config",
            AcpRequest::AcpResumeSessionRequest(_) => "resume_session",
            AcpRequest::AcpListAgentsRequest(_) => "list_agents",
            AcpRequest::AcpDeliverAgentOutputRequest(_) => "deliver_agent_output",
            AcpRequest::AcpDeliverAgentStderrRequest(_) => "deliver_agent_stderr",
            AcpRequest::AcpAbortPendingRequest(_) => "abort_pending",
        }
    }

    async fn dispatch_shared_core(
        &self,
        mut ctx: ExtensionContext<'_>,
        request: AcpRequest,
    ) -> AcpHandlerOutput {
        let owner_id = ownership_owner_id(ctx.ownership());
        let mut broker_events = Vec::new();
        let (connection_id, wire_session_id) = ownership_session_identity(ctx.ownership());
        self.core_owners.lock().await.insert(
            owner_id.clone(),
            NativeCoreOwner {
                connection_id,
                wire_session_id,
            },
        );
        let core = self.core_for_owner(&owner_id).await;
        let mut result = self
            .run_core_transition(&core, &mut ctx, &owner_id, request, &mut broker_events)
            .await;
        while let Ok(AcpResponse::AcpPendingResponse(pending)) = &result {
            result = self
                .drive_native_pending(
                    &core,
                    &mut ctx,
                    &owner_id,
                    pending.clone(),
                    &mut broker_events,
                )
                .await;
        }

        let mut core = match core.lock() {
            Ok(core) => core,
            Err(poisoned) => {
                tracing::error!(
                    "native ACP core mutex was poisoned during event delivery; recovering state"
                );
                poisoned.into_inner()
            }
        };
        for event in core.events_for_delivery(&owner_id) {
            let payload = match encode_event(event) {
                Ok(payload) => payload,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "failed to encode a committed native ACP event; retaining it for retry"
                    );
                    break;
                }
            };
            let frame = match ctx.ext_event_wire(payload) {
                Ok(frame) => frame,
                Err(error) => {
                    tracing::error!(
                        error = %error,
                        "failed to frame a committed native ACP event; retaining it for retry"
                    );
                    break;
                }
            };
            if let Err(error) = deliver_event(&ctx, &mut broker_events, frame) {
                tracing::error!(
                    error = %error,
                    "failed to deliver a committed native ACP event; retaining it for retry"
                );
                break;
            }
            if let Err(error) = core.acknowledge_delivered_events(&owner_id, 1) {
                tracing::error!(
                    error = %error,
                    "failed to acknowledge a delivered native ACP event; it may be retried"
                );
                break;
            }
        }

        AcpHandlerOutput {
            response: Ok(match result {
                Ok(response) => response,
                Err(error) => agentos_sidecar_core::error_response(&error),
            }),
            events: broker_events,
        }
    }

    async fn run_core_transition(
        &self,
        core: &Arc<std::sync::Mutex<AcpCore>>,
        ctx: &mut ExtensionContext<'_>,
        owner_id: &str,
        request: AcpRequest,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<AcpResponse, AcpCoreError> {
        let (commands, receiver) = mpsc::channel(1);
        let core = Arc::clone(core);
        let worker_owner_id = owner_id.to_string();
        let finished = commands.clone();
        let worker = thread::spawn(move || {
            let result = {
                let mut core = match core.lock() {
                    Ok(core) => core,
                    Err(poisoned) => {
                        tracing::error!("native ACP core mutex was poisoned; recovering state");
                        poisoned.into_inner()
                    }
                };
                let mut host = NativeCoreHost {
                    commands,
                    started_at: Instant::now(),
                };
                core.dispatch_resumable(&mut host, &worker_owner_id, request)
            };
            if finished.blocking_send(NativeCoreCommand::Finished).is_err() {
                tracing::warn!("native ACP request was cancelled before the core completed");
            }
            result
        });
        self.drive_native_core_broker(ctx, receiver, broker_events)
            .await;
        worker.join().unwrap_or_else(|_| {
            Err(AcpCoreError::Execution(String::from(
                "native ACP core worker panicked",
            )))
        })
    }

    async fn drive_native_pending(
        &self,
        core: &Arc<std::sync::Mutex<AcpCore>>,
        ctx: &mut ExtensionContext<'_>,
        owner_id: &str,
        pending: agentos_protocol::generated::v1::AcpPendingResponse,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<AcpResponse, AcpCoreError> {
        let deadline = Instant::now() + Duration::from_millis(u64::from(pending.timeout_ms));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return self
                    .run_core_transition(
                        core,
                        ctx,
                        owner_id,
                        AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                            process_id: pending.process_id,
                            reason: AcpPendingAbortReason::InteractionTimeout,
                            exit_code: None,
                        }),
                        broker_events,
                    )
                    .await;
            }
            match self
                .poll_native_core_output(
                    ctx,
                    &pending.process_id,
                    remaining.min(ACP_TERMINAL_OUTPUT_POLL_INTERVAL),
                )
                .await?
            {
                Some(AgentOutput::Stdout(chunk)) => {
                    return self
                        .run_core_transition(
                            core,
                            ctx,
                            owner_id,
                            AcpRequest::AcpDeliverAgentOutputRequest(
                                AcpDeliverAgentOutputRequest {
                                    process_id: pending.process_id,
                                    chunk,
                                },
                            ),
                            broker_events,
                        )
                        .await;
                }
                Some(AgentOutput::Exited(exit_code)) => {
                    return self
                        .run_core_transition(
                            core,
                            ctx,
                            owner_id,
                            AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                                process_id: pending.process_id,
                                reason: AcpPendingAbortReason::AgentExited,
                                exit_code,
                            }),
                            broker_events,
                        )
                        .await;
                }
                Some(AgentOutput::Stderr(chunk)) => {
                    self.run_core_transition(
                        core,
                        ctx,
                        owner_id,
                        AcpRequest::AcpDeliverAgentStderrRequest(AcpDeliverAgentStderrRequest {
                            process_id: pending.process_id.clone(),
                            chunk,
                        }),
                        broker_events,
                    )
                    .await?;
                }
                None => {}
            }
        }
    }

    async fn core_for_owner(&self, owner_id: &str) -> Arc<std::sync::Mutex<AcpCore>> {
        let mut cores = self.cores.lock().await;
        Arc::clone(
            cores
                .entry(owner_id.to_string())
                .or_insert_with(|| Arc::new(std::sync::Mutex::new(AcpCore::default()))),
        )
    }

    async fn release_interrupted_prompt_state(
        &self,
        owner_id: &str,
        process_id: &str,
    ) -> Result<(), SidecarError> {
        let core = self.core_for_owner(owner_id).await;
        let mut core = match core.lock() {
            Ok(core) => core,
            Err(poisoned) => {
                tracing::error!("native ACP core mutex was poisoned during prompt interrupt");
                poisoned.into_inner()
            }
        };
        core.abandon_pending_prompt(owner_id, process_id)
            .map(|_| ())
            .map_err(|error| SidecarError::InvalidState(error.to_string()))
    }

    async fn mark_interrupted_prompt_for_drain(
        &self,
        owner_id: &str,
        process_id: &str,
    ) -> Result<(), SidecarError> {
        let core = self.core_for_owner(owner_id).await;
        let mut core = match core.lock() {
            Ok(core) => core,
            Err(poisoned) => {
                tracing::error!(
                    "native ACP core mutex was poisoned while marking prompt cancellation"
                );
                poisoned.into_inner()
            }
        };
        core.interrupt_pending_prompt(owner_id, process_id)
            .map(|_| ())
            .map_err(|error| SidecarError::InvalidState(error.to_string()))
    }

    async fn drain_interrupted_prompt_boundary(
        &self,
        core: &Arc<std::sync::Mutex<AcpCore>>,
        ctx: &mut ExtensionContext<'_>,
        owner_id: &str,
        session_id: &str,
        process_id: &str,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<(), SidecarError> {
        let deadline = Instant::now() + ACP_CANCEL_DRAIN_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                tracing::warn!(
                    owner_id,
                    session_id,
                    process_id,
                    timeout_ms = ACP_CANCEL_DRAIN_TIMEOUT.as_millis() as u64,
                    "ACP adapter did not reach its cancelled prompt response boundary; restarting it"
                );
                return self
                    .recover_interrupted_prompt_adapter(
                        core,
                        ctx,
                        owner_id,
                        session_id,
                        process_id,
                        None,
                        broker_events,
                    )
                    .await;
            }
            let output = match self
                .poll_native_core_output(
                    ctx,
                    process_id,
                    remaining.min(ACP_TERMINAL_OUTPUT_POLL_INTERVAL),
                )
                .await
            {
                Ok(output) => output,
                Err(error) => {
                    tracing::warn!(
                        owner_id,
                        session_id,
                        process_id,
                        error = %error,
                        "failed to poll the cancelled ACP prompt boundary; restarting the adapter"
                    );
                    return self
                        .recover_interrupted_prompt_adapter(
                            core,
                            ctx,
                            owner_id,
                            session_id,
                            process_id,
                            None,
                            broker_events,
                        )
                        .await
                        .map_err(|recovery_error| {
                            SidecarError::Execution(format!(
                                "failed to drain cancelled ACP prompt {process_id}: {error}; recovery failed: {recovery_error}"
                            ))
                        });
                }
            };
            match output {
                Some(AgentOutput::Stdout(chunk)) => {
                    let response = self
                        .run_core_transition(
                            core,
                            ctx,
                            owner_id,
                            AcpRequest::AcpDeliverAgentOutputRequest(
                                AcpDeliverAgentOutputRequest {
                                    process_id: process_id.to_string(),
                                    chunk,
                                },
                            ),
                            broker_events,
                        )
                        .await
                        .map_err(|error| SidecarError::Execution(error.to_string()))?;
                    match response {
                        AcpResponse::AcpPendingResponse(_) => {}
                        AcpResponse::AcpErrorResponse(AcpErrorResponse { code, .. })
                            if code == "agent_interaction_cancelled" =>
                        {
                            return Ok(());
                        }
                        other => {
                            return Err(SidecarError::InvalidState(format!(
                                "cancelled ACP prompt produced an unexpected response while draining {process_id}: {other:?}"
                            )));
                        }
                    }
                }
                Some(AgentOutput::Stderr(_)) | None => {}
                Some(AgentOutput::Exited(exit_code)) => {
                    return self
                        .recover_interrupted_prompt_adapter(
                            core,
                            ctx,
                            owner_id,
                            session_id,
                            process_id,
                            exit_code,
                            broker_events,
                        )
                        .await;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn recover_interrupted_prompt_adapter(
        &self,
        core: &Arc<std::sync::Mutex<AcpCore>>,
        ctx: &mut ExtensionContext<'_>,
        owner_id: &str,
        session_id: &str,
        process_id: &str,
        exit_code: Option<i32>,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<(), SidecarError> {
        let mut response = self
            .run_core_transition(
                core,
                ctx,
                owner_id,
                AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                    process_id: process_id.to_string(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code,
                }),
                broker_events,
            )
            .await
            .map_err(|error| SidecarError::Execution(error.to_string()))?;
        while let AcpResponse::AcpPendingResponse(pending) = response {
            response = self
                .drive_native_pending(core, ctx, owner_id, pending, broker_events)
                .await
                .map_err(|error| SidecarError::Execution(error.to_string()))?;
        }

        let session_is_live = match core.lock() {
            Ok(core) => core.session_is_owned_by(owner_id, session_id),
            Err(poisoned) => {
                tracing::error!(
                    "native ACP core mutex was poisoned after cancelled-prompt recovery"
                );
                poisoned
                    .into_inner()
                    .session_is_owned_by(owner_id, session_id)
            }
        };
        if session_is_live {
            Ok(())
        } else {
            Err(SidecarError::Execution(format!(
                "ACP adapter could not recover after cancelled prompt {process_id}: {response:?}"
            )))
        }
    }

    async fn drive_native_core_broker(
        &self,
        ctx: &mut ExtensionContext<'_>,
        mut receiver: mpsc::Receiver<NativeCoreCommand>,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) {
        while let Some(command) = receiver.recv().await {
            match command {
                NativeCoreCommand::Finished => return,
                command => {
                    self.handle_native_core_command(ctx, command, broker_events)
                        .await
                }
            }
        }
    }

    async fn handle_native_core_command(
        &self,
        ctx: &mut ExtensionContext<'_>,
        command: NativeCoreCommand,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) {
        match command {
            NativeCoreCommand::ResolveAgent { id, reply } => {
                let result = ctx
                    .projected_agents()
                    .await
                    .map_err(sidecar_to_core_error)
                    .map(|agents| {
                        agents
                            .into_iter()
                            .find(|agent| agent.id == id)
                            .filter(|agent| !agent.acp_entrypoint.is_empty())
                            .map(|agent| ProjectedAgentLaunch {
                                id: agent.id,
                                adapter_entrypoint: agent.adapter_entrypoint,
                                env: agent.env,
                                launch_args: agent.launch_args,
                            })
                    });
                send_native_core_reply(reply, result, "resolve agent");
            }
            NativeCoreCommand::ListAgents { reply } => {
                let result = ctx
                    .projected_agents()
                    .await
                    .map_err(sidecar_to_core_error)
                    .map(|agents| {
                        agents
                            .into_iter()
                            .filter(|agent| !agent.acp_entrypoint.is_empty())
                            .map(|agent| ProjectedAgentLaunch {
                                id: agent.id,
                                adapter_entrypoint: agent.adapter_entrypoint,
                                env: agent.env,
                                launch_args: agent.launch_args,
                            })
                            .collect()
                    });
                send_native_core_reply(reply, result, "list agents");
            }
            NativeCoreCommand::HostToolReference { reply } => {
                let result = ctx
                    .registered_host_tool_reference()
                    .await
                    .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "host tool reference");
            }
            NativeCoreCommand::SpawnAgent { request, reply } => {
                let process_id = request.process_id.clone();
                let result = ctx
                    .spawn_process_wire(ExecuteRequest {
                        process_id: Some(request.process_id),
                        command: request.command,
                        shell_command: None,
                        runtime: Some(convert_runtime(request.runtime)),
                        entrypoint: request.entrypoint,
                        args: request.args,
                        env: Some(request.env.into_iter().collect()),
                        cwd: request.cwd,
                        wasm_permission_tier: None,
                        pty: None,
                        keep_stdin_open: Some(true),
                        timeout_ms: None,
                        capture_output: None,
                    })
                    .await;
                let result = match result {
                    Ok(started) => match ctx
                        .start_buffering_process_output(&started.process_id)
                        .await
                    {
                        Ok(()) => Ok(SpawnedAgent {
                            process_id: started.process_id,
                            pid: started.pid,
                        }),
                        Err(buffer_error) => {
                            let cleanup = ctx
                                .kill_process_wire(KillProcessRequest {
                                    process_id: started.process_id.clone(),
                                    signal: String::from("SIGKILL"),
                                })
                                .await;
                            Err(sidecar_to_core_error(match cleanup {
                                Ok(_) => buffer_error,
                                Err(cleanup_error) => SidecarError::Execution(format!(
                                    "failed to start buffered output for ACP adapter {}: {buffer_error}; cleanup failed: {cleanup_error}",
                                    started.process_id
                                )),
                            }))
                        }
                    },
                    Err(error) => Err(sidecar_to_core_error(error)),
                };
                if result.is_ok() {
                    let owner_id = ownership_owner_id(ctx.ownership());
                    self.core_processes.lock().await.insert(
                        (owner_id.clone(), process_id),
                        Arc::new(Mutex::new(NativeCoreProcess {
                            owner_id,
                            session_id: None,
                            pending_output: VecDeque::new(),
                            exit_observed: false,
                            pending_cleanup_events: VecDeque::new(),
                            cleanup: NativeRouteCleanupProgress::default(),
                        })),
                    );
                }
                send_native_core_reply(reply, result, "spawn agent");
            }
            NativeCoreCommand::BindSession {
                session_id,
                process_id,
                reply,
            } => {
                let result = ctx
                    .bind_process_to_session(&session_id, &process_id)
                    .await
                    .map_err(sidecar_to_core_error);
                if result.is_ok() {
                    let owner_id = ownership_owner_id(ctx.ownership());
                    let process = self
                        .core_processes
                        .lock()
                        .await
                        .get(&(owner_id, process_id))
                        .cloned();
                    if let Some(process) = process {
                        process.lock().await.session_id = Some(session_id);
                    }
                }
                send_native_core_reply(reply, result, "bind session");
            }
            NativeCoreCommand::WriteStdin {
                process_id,
                chunk,
                reply,
            } => {
                let result = ctx
                    .write_stdin_wire(WriteStdinRequest {
                        process_id: process_id.clone(),
                        chunk,
                    })
                    .await;
                let result = result.map(|_| ()).map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "write stdin");
            }
            NativeCoreCommand::CloseStdin { process_id, reply } => {
                let result = ctx
                    .close_stdin_wire(CloseStdinRequest { process_id })
                    .await
                    .map(|_| ())
                    .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "close stdin");
            }
            NativeCoreCommand::PollOutput { process_id, reply } => {
                let result = self
                    .poll_native_core_output(ctx, &process_id, ACP_TERMINAL_OUTPUT_POLL_INTERVAL)
                    .await;
                send_native_core_reply(reply, result, "poll output");
            }
            NativeCoreCommand::KillAgent {
                process_id,
                signal,
                reply,
            } => {
                let result = ctx
                    .kill_process_wire(KillProcessRequest { process_id, signal })
                    .await
                    .map(|_| ())
                    .or_else(|error| {
                        if is_process_already_gone(&error) {
                            Ok(())
                        } else {
                            Err(error)
                        }
                    })
                    .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "kill agent");
            }
            NativeCoreCommand::AbortAgent { process_id, reply } => {
                let mut errors = Vec::new();
                let owner_id = ownership_owner_id(ctx.ownership());
                let route = self
                    .core_processes
                    .lock()
                    .await
                    .get(&(owner_id, process_id.clone()))
                    .cloned();
                let exit_observed = match route {
                    Some(route) => route.lock().await.exit_observed,
                    None => false,
                };
                let wait_for_exit = if exit_observed {
                    false
                } else {
                    match ctx
                        .kill_process_wire(KillProcessRequest {
                            process_id: process_id.clone(),
                            signal: String::from("SIGKILL"),
                        })
                        .await
                    {
                        Ok(_) => true,
                        Err(error) if is_process_already_gone(&error) => false,
                        Err(error) => {
                            errors.push(sidecar_to_core_error(error));
                            false
                        }
                    }
                };
                if wait_for_exit {
                    let deadline = Instant::now() + ACP_CANCEL_DRAIN_TIMEOUT;
                    loop {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            errors.push(AcpCoreError::Execution(format!(
                                "timed out after {} ms waiting for aborted native ACP adapter {process_id} to exit",
                                ACP_CANCEL_DRAIN_TIMEOUT.as_millis()
                            )));
                            break;
                        }
                        match self
                            .poll_native_core_output(
                                ctx,
                                &process_id,
                                remaining.min(ACP_TERMINAL_OUTPUT_POLL_INTERVAL),
                            )
                            .await
                        {
                            Ok(Some(AgentOutput::Exited(_))) => break,
                            Ok(_) => {}
                            Err(error) => {
                                errors.push(error);
                                break;
                            }
                        }
                    }
                }
                let cleanup = self
                    .cleanup_native_agent_route(ctx, None, &process_id, broker_events)
                    .await;
                if let Err(error) = cleanup {
                    errors.push(sidecar_to_core_error(error));
                }
                let result = if errors.is_empty() {
                    Ok(())
                } else {
                    Err(AcpCoreError::Cleanup {
                        context: "failed to abort native ACP adapter completely",
                        errors,
                    })
                };
                send_native_core_reply(reply, result, "abort agent");
            }
            NativeCoreCommand::FinalizeSessionCleanup {
                session_id,
                process_id,
                reply,
            } => {
                let result = self
                    .finalize_native_session_cleanup(ctx, &session_id, &process_id, broker_events)
                    .await
                    .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "finalize session cleanup");
            }
            NativeCoreCommand::WaitForExit {
                process_id,
                timeout_ms,
                reply,
            } => {
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                let result = loop {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break Ok(None);
                    }
                    match self
                        .poll_native_core_output(
                            ctx,
                            &process_id,
                            remaining.min(ACP_TERMINAL_OUTPUT_POLL_INTERVAL),
                        )
                        .await
                    {
                        Ok(Some(AgentOutput::Exited(code))) => break Ok(code),
                        Ok(_) => {}
                        Err(error) => break Err(error),
                    }
                };
                send_native_core_reply(reply, result, "wait for exit");
            }
            NativeCoreCommand::WriteFile {
                path,
                contents,
                reply,
            } => {
                let result = ctx
                    .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                        operation: GuestFilesystemOperation::WriteFile,
                        path,
                        destination_path: None,
                        target: None,
                        content: Some(base64::engine::general_purpose::STANDARD.encode(contents)),
                        encoding: Some(RootFilesystemEntryEncoding::Base64),
                        recursive: None,
                        max_depth: None,
                        mode: None,
                        uid: None,
                        gid: None,
                        atime_ms: None,
                        mtime_ms: None,
                        len: None,
                        offset: None,
                    })
                    .await
                    .map(|_| ())
                    .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "write file");
            }
            NativeCoreCommand::ReadFile { path, reply } => {
                let result = ctx
                    .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                        operation: GuestFilesystemOperation::ReadFile,
                        path,
                        destination_path: None,
                        target: None,
                        content: None,
                        encoding: None,
                        recursive: None,
                        max_depth: None,
                        mode: None,
                        uid: None,
                        gid: None,
                        atime_ms: None,
                        mtime_ms: None,
                        len: None,
                        offset: None,
                    })
                    .await
                    .map_err(sidecar_to_core_error)
                    .and_then(decode_guest_file_response);
                send_native_core_reply(reply, result, "read file");
            }
            NativeCoreCommand::InboundRequest {
                process_id,
                request,
                reply,
            } => {
                let owner_id = ownership_owner_id(ctx.ownership());
                let process = self
                    .core_processes
                    .lock()
                    .await
                    .get(&(owner_id, process_id.clone()))
                    .cloned();
                let process = match process {
                    Some(process) => Some(process.lock().await.clone()),
                    None => None,
                };
                let result =
                    build_inbound_response(self, ctx, &process_id, process.as_ref(), &request)
                        .await
                        .map_err(sidecar_to_core_error);
                send_native_core_reply(reply, result, "inbound request");
            }
            NativeCoreCommand::Finished => {
                unreachable!("finished commands are handled by the broker loop")
            }
        }
    }

    async fn poll_native_core_output(
        &self,
        ctx: &mut ExtensionContext<'_>,
        process_id: &str,
        timeout: Duration,
    ) -> Result<Option<AgentOutput>, AcpCoreError> {
        let owner_id = ownership_owner_id(ctx.ownership());
        let route_key = (owner_id, process_id.to_string());
        let route = self.core_processes.lock().await.get(&route_key).cloned();
        if let Some(route) = route {
            let mut process = route.lock().await;
            if let Some(output) = process.pending_output.pop_front() {
                if matches!(output, AgentOutput::Exited(_)) {
                    process.exit_observed = true;
                }
                return Ok(Some(output));
            }
        }

        let buffered = ctx
            .drain_buffered_process_output(process_id, timeout)
            .await
            .map_err(sidecar_to_core_error)?;
        if buffered.stdout_truncated {
            tracing::warn!(
                process_id,
                limit = DEFAULT_ACP_TERMINAL_OUTPUT_BYTE_LIMIT,
                "ACP adapter stdout exceeded the buffered-output limit and was truncated"
            );
        }
        if buffered.stderr_truncated {
            tracing::warn!(
                process_id,
                limit = DEFAULT_ACP_TERMINAL_OUTPUT_BYTE_LIMIT,
                "ACP adapter stderr exceeded the buffered-output limit and was truncated"
            );
        }
        let route = self
            .core_processes
            .lock()
            .await
            .get(&route_key)
            .cloned()
            .ok_or_else(|| {
                AcpCoreError::InvalidState(format!(
                    "native ACP adapter route disappeared while polling {process_id}"
                ))
            })?;
        let mut process = route.lock().await;
        if !buffered.stdout.is_empty() {
            process
                .pending_output
                .push_back(AgentOutput::Stdout(buffered.stdout));
        }
        if !buffered.stderr.is_empty() {
            process
                .pending_output
                .push_back(AgentOutput::Stderr(buffered.stderr));
        }
        if buffered.exit_code.is_some() {
            process.exit_observed = true;
            process
                .pending_output
                .push_back(AgentOutput::Exited(buffered.exit_code));
        }
        Ok(process.pending_output.pop_front())
    }

    async fn cleanup_session_terminals(
        &self,
        ctx: &mut ExtensionContext<'_>,
        session_id: &str,
    ) -> Result<(), SidecarError> {
        let ownership = ctx.ownership().clone();
        let terminals = {
            let terminals = self.terminals.lock().await;
            terminals
                .iter()
                .filter(|(_, terminal)| {
                    terminal.ownership == ownership && terminal.session_id == session_id
                })
                .map(|(terminal_id, terminal)| (terminal_id.clone(), terminal.process_id.clone()))
                .collect::<Vec<_>>()
        };
        let mut errors = Vec::new();
        for (terminal_id, process_id) in terminals {
            let kill = ctx
                .kill_process_wire(KillProcessRequest {
                    process_id: process_id.clone(),
                    signal: String::from("SIGKILL"),
                })
                .await;
            let stop = ctx.stop_buffering_process_output(&process_id).await;
            let kill_succeeded = match &kill {
                Ok(_) => true,
                Err(error) => is_process_already_gone(error),
            };
            if let Err(error) = &kill {
                if !kill_succeeded {
                    tracing::error!(terminal_id, process_id, error = %error, "failed to kill ACP terminal during cleanup");
                    errors.push(SidecarError::Context {
                        context: format!("terminal {terminal_id} process {process_id} kill"),
                        source: Box::new(error.clone()),
                    });
                }
            }
            if let Err(error) = &stop {
                tracing::error!(terminal_id, process_id, error = %error, "failed to stop ACP terminal output buffering during cleanup");
                errors.push(SidecarError::Context {
                    context: format!("terminal {terminal_id} process {process_id} output cleanup"),
                    source: Box::new(error.clone()),
                });
            }
            if kill_succeeded && stop.is_ok() {
                self.terminals.lock().await.remove(&terminal_id);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SidecarError::Cleanup {
                context: "failed to clean up ACP session terminals",
                errors,
            })
        }
    }

    async fn finalize_native_session_cleanup(
        &self,
        ctx: &mut ExtensionContext<'_>,
        session_id: &str,
        process_id: &str,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<(), SidecarError> {
        self.cleanup_native_agent_route(ctx, Some(session_id), process_id, broker_events)
            .await
    }

    async fn cleanup_native_agent_route(
        &self,
        ctx: &mut ExtensionContext<'_>,
        expected_session_id: Option<&str>,
        process_id: &str,
        broker_events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    ) -> Result<(), SidecarError> {
        let owner_id = ownership_owner_id(ctx.ownership());
        let route_key = (owner_id, process_id.to_string());
        let Some(route) = self.core_processes.lock().await.get(&route_key).cloned() else {
            return Ok(());
        };
        // Lock only this route across destructive host calls. The returned
        // events and phase bits are persisted synchronously after each await,
        // while unrelated owners can still access their independent routes.
        let mut process = route.lock().await;
        if let Some(expected_session_id) = expected_session_id {
            if process.session_id.as_deref() != Some(expected_session_id) {
                return Err(SidecarError::InvalidState(format!(
                    "native ACP process {process_id} is not bound to session {expected_session_id}"
                )));
            }
        }
        // Drain already committed events before asking the native host to do
        // more cleanup. If delivery is backpressured, retry only delivery; this
        // keeps the route queue bounded by one native cleanup batch and avoids
        // repeating destructive phases merely to append another batch.
        if let Err(error) = drain_committed_cleanup_events(
            process_id,
            &mut process.pending_cleanup_events,
            |event| deliver_event(ctx, broker_events, event),
        ) {
            return Err(SidecarError::Cleanup {
                context: "failed to clean up native ACP agent route completely",
                errors: vec![error],
            });
        }
        let session_id = process.session_id.clone();
        let mut errors = Vec::new();
        if !process.cleanup.output_buffer_stopped {
            match ctx.stop_buffering_process_output(process_id).await {
                Ok(()) => {
                    process.cleanup.output_buffer_stopped = true;
                }
                Err(error) => errors.push(error),
            }
        }
        if let Some(session_id) = session_id.as_deref() {
            if !process.cleanup.terminals_cleaned {
                match self.cleanup_session_terminals(ctx, session_id).await {
                    Ok(()) => {
                        process.cleanup.terminals_cleaned = true;
                    }
                    Err(error) => errors.push(error),
                }
            }
            // Resource disposal can destroy the VM/process handles required by
            // output-buffer and terminal retries, so this phase is deliberately
            // dependent on both earlier phases. The earlier two are independent
            // and are always attempted together above.
            if process.cleanup.output_buffer_stopped
                && process.cleanup.terminals_cleaned
                && !process.cleanup.session_resources_disposed
            {
                match ctx.dispose_session_resources_wire(session_id).await {
                    Ok(outcome) => {
                        process.pending_cleanup_events.extend(outcome.events);
                        if let Some(error) = outcome.error {
                            errors.push(error);
                        } else {
                            process.cleanup.session_resources_disposed = true;
                        }
                    }
                    Err(error) => errors.push(error),
                }
            }
        } else {
            process.cleanup.terminals_cleaned = true;
            process.cleanup.session_resources_disposed = true;
        }
        if let Err(error) = drain_committed_cleanup_events(
            process_id,
            &mut process.pending_cleanup_events,
            |event| deliver_event(ctx, broker_events, event),
        ) {
            errors.push(error);
        }
        let complete = errors.is_empty()
            && process.cleanup.output_buffer_stopped
            && process.cleanup.terminals_cleaned
            && process.cleanup.session_resources_disposed
            && process.pending_cleanup_events.is_empty();
        drop(process);
        if complete {
            let mut routes = self.core_processes.lock().await;
            if routes
                .get(&route_key)
                .is_some_and(|current| Arc::ptr_eq(current, &route))
            {
                routes.remove(&route_key);
            }
            Ok(())
        } else {
            Err(SidecarError::Cleanup {
                context: "failed to clean up native ACP agent route completely",
                errors,
            })
        }
    }

    fn allocate_process_id(&self, prefix: &str) -> String {
        let id = self.next_process_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("{prefix}-{id}")
    }
}

impl Extension for AcpExtension {
    fn namespace(&self) -> &str {
        ACP_EXTENSION_NAMESPACE
    }

    fn handle_request<'a>(
        &'a self,
        ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            let response = self.handle_payload(ctx, &payload).await?;
            Ok(response)
        })
    }

    fn is_blocking_request(&self, payload: &[u8]) -> bool {
        matches!(
            decode_request(payload),
            Ok(AcpRequest::AcpSessionRequest(request)) if request.method == "session/prompt"
        )
    }

    fn on_dispose<'a>(&'a self) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            self.cores.lock().await.clear();
            self.core_processes.lock().await.clear();
            self.core_owners.lock().await.clear();
            let mut permission_waits = self.permission_waits.lock().await;
            for cancellation in permission_waits.values() {
                cancellation.cancel();
            }
            permission_waits.clear();
            drop(permission_waits);
            self.terminals.lock().await.clear();
            Ok(())
        })
    }

    fn on_session_disposed<'a>(&'a self, ctx: ExtensionSnapshot) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let (connection_id, wire_session_id) = ownership_session_identity(ctx.ownership());
            let owner_ids = {
                let owners = self.core_owners.lock().await;
                registered_owner_ids_for_session(
                    &owners,
                    &connection_id,
                    wire_session_id.as_deref(),
                )
            };
            let removed_cores = {
                let mut cores = self.cores.lock().await;
                owner_ids
                    .iter()
                    .filter_map(|owner_id| cores.remove(owner_id).map(|core| (owner_id, core)))
                    .collect::<Vec<_>>()
            };
            for (owner_id, core) in removed_cores {
                let mut core = match core.lock() {
                    Ok(core) => core,
                    Err(poisoned) => {
                        tracing::error!("native ACP owner core mutex was poisoned during disposal");
                        poisoned.into_inner()
                    }
                };
                core.drop_owner_state(owner_id);
            }
            self.core_processes
                .lock()
                .await
                .retain(|(owner_id, _), _| !owner_ids.contains(owner_id));
            self.core_owners
                .lock()
                .await
                .retain(|owner_id, _| !owner_ids.contains(owner_id));
            let mut permission_waits = self.permission_waits.lock().await;
            permission_waits.retain(|key, cancellation| {
                let retain = !owner_ids.contains(&key.owner_id);
                if !retain {
                    cancellation.cancel();
                }
                retain
            });
            drop(permission_waits);
            self.terminals.lock().await.retain(|_, terminal| {
                ownership_session_identity(&terminal.ownership)
                    != (connection_id.clone(), wire_session_id.clone())
            });
            Ok(())
        })
    }

    fn interrupt_blocking_request(
        &self,
        blocking_payload: &[u8],
        interrupt: ExtensionInterruptRequest<'_>,
    ) -> Option<ExtensionInterruptResponse> {
        let AcpRequest::AcpSessionRequest(blocking_request) =
            decode_request(blocking_payload).ok()?
        else {
            return None;
        };
        if blocking_request.method != "session/prompt" {
            return None;
        }

        let interrupted_response_payload =
            encode_interrupted_session_response(&blocking_request.session_id)?;
        match interrupt {
            ExtensionInterruptRequest::KillProcess | ExtensionInterruptRequest::CloseSession => {
                Some(ExtensionInterruptResponse {
                    interrupted_response_payload,
                    interrupting_response_payload: None,
                })
            }
            ExtensionInterruptRequest::ExtensionPayload(payload) => {
                let request = decode_request(payload).ok()?;
                match request {
                    AcpRequest::AcpCloseSessionRequest(request)
                        if request.session_id == blocking_request.session_id =>
                    {
                        Some(ExtensionInterruptResponse {
                            interrupted_response_payload,
                            interrupting_response_payload: None,
                        })
                    }
                    AcpRequest::AcpSessionRequest(request)
                        if request.session_id == blocking_request.session_id
                            && request.method == ACP_CANCEL_METHOD =>
                    {
                        Some(ExtensionInterruptResponse {
                            interrupted_response_payload,
                            interrupting_response_payload: Some(
                                encode_interrupted_cancel_response(&request.session_id)?,
                            ),
                        })
                    }
                    AcpRequest::AcpCreateSessionRequest(_)
                    | AcpRequest::AcpGetSessionStateRequest(_)
                    | AcpRequest::AcpListSessionsRequest(_)
                    | AcpRequest::AcpCloseSessionRequest(_)
                    | AcpRequest::AcpResumeSessionRequest(_)
                    | AcpRequest::AcpSessionRequest(_)
                    | AcpRequest::AcpSetSessionConfigRequest(_)
                    | AcpRequest::AcpListAgentsRequest(_)
                    | AcpRequest::AcpDeliverAgentOutputRequest(_)
                    | AcpRequest::AcpDeliverAgentStderrRequest(_)
                    | AcpRequest::AcpAbortPendingRequest(_) => None,
                }
            }
        }
    }

    fn on_blocking_request_interrupted<'a>(
        &'a self,
        mut ctx: ExtensionContext<'a>,
        blocking_payload: Vec<u8>,
        interrupt: ExtensionInterrupt,
    ) -> ExtensionFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let AcpRequest::AcpSessionRequest(blocking_request) =
                decode_request(&blocking_payload)?
            else {
                return Err(SidecarError::InvalidState(String::from(
                    "native ACP interrupt target is not a session request",
                )));
            };
            if blocking_request.method != "session/prompt" {
                return Err(SidecarError::InvalidState(String::from(
                    "native ACP interrupt target is not a prompt",
                )));
            }

            let owner_id = ownership_owner_id(ctx.ownership());
            let mut broker_events = Vec::new();
            let owner_routes = {
                let processes = self.core_processes.lock().await;
                processes
                    .iter()
                    .filter(|((route_owner_id, _), _)| route_owner_id == &owner_id)
                    .map(|((_, process_id), process)| (process_id.clone(), Arc::clone(process)))
                    .collect::<Vec<_>>()
            };
            let process_id = native_prompt_interrupt_target(
                &owner_routes,
                &owner_id,
                &blocking_request.session_id,
            )
            .await;
            let process_id = match process_id {
                Ok(process_id) => process_id,
                Err(error) => {
                    tracing::error!(
                        owner_id,
                        session_id = blocking_request.session_id,
                        error = %error,
                        "failed to route native ACP prompt interrupt"
                    );
                    return Ok(Some(encode_response(error_response(error))?));
                }
            };

            let wait_key = NativePermissionWaitKey {
                owner_id: owner_id.clone(),
                session_id: blocking_request.session_id.clone(),
                process_id: process_id.clone(),
            };
            cancel_native_permission_wait(self, &wait_key).await;

            let explicit_cancel = matches!(
                &interrupt,
                ExtensionInterrupt::ExtensionPayload(payload)
                    if matches!(
                        decode_request(payload),
                        Ok(AcpRequest::AcpSessionRequest(request))
                            if request.session_id == blocking_request.session_id
                                && request.method == ACP_CANCEL_METHOD
                    )
            );
            if !explicit_cancel {
                if let Err(error) = self
                    .release_interrupted_prompt_state(&owner_id, &process_id)
                    .await
                {
                    tracing::error!(
                        owner_id,
                        session_id = blocking_request.session_id,
                        process_id,
                        error = %error,
                        "failed to release native ACP prompt continuation after transport interrupt"
                    );
                }
                return Ok(None);
            }

            if let Err(delivery_error) = deliver_native_prompt_cancel(
                &mut ctx,
                &owner_id,
                &blocking_request.session_id,
                &process_id,
            )
            .await
            {
                let core = self.core_for_owner(&owner_id).await;
                let cleanup = self
                    .run_core_transition(
                        &core,
                        &mut ctx,
                        &owner_id,
                        AcpRequest::AcpAbortPendingRequest(AcpAbortPendingRequest {
                            process_id: process_id.clone(),
                            reason: AcpPendingAbortReason::CallerCancelled,
                            exit_code: None,
                        }),
                        &mut broker_events,
                    )
                    .await;
                let error = match cleanup {
                    Ok(_) => delivery_error,
                    Err(cleanup_error) => SidecarError::Execution(format!(
                        "{delivery_error}; interrupted prompt cleanup failed: {cleanup_error}"
                    )),
                };
                return Ok(Some(encode_response(error_response(error))?));
            }

            if let Err(error) = self
                .mark_interrupted_prompt_for_drain(&owner_id, &process_id)
                .await
            {
                return Ok(Some(encode_response(error_response(error))?));
            }
            let core = self.core_for_owner(&owner_id).await;
            match self
                .drain_interrupted_prompt_boundary(
                    &core,
                    &mut ctx,
                    &owner_id,
                    &blocking_request.session_id,
                    &process_id,
                    &mut broker_events,
                )
                .await
            {
                Ok(()) => Ok(None),
                Err(error) => Ok(Some(encode_response(error_response(error))?)),
            }
        })
    }
}

trait NativePromptInterruptIo {
    fn write_stdin<'a>(&'a mut self, request: WriteStdinRequest) -> ExtensionFuture<'a, ()>;
}

impl NativePromptInterruptIo for ExtensionContext<'_> {
    fn write_stdin<'a>(&'a mut self, request: WriteStdinRequest) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            self.write_stdin_wire(request).await?;
            Ok(())
        })
    }
}

async fn native_prompt_interrupt_target(
    processes: &[(String, Arc<Mutex<NativeCoreProcess>>)],
    owner_id: &str,
    session_id: &str,
) -> Result<String, SidecarError> {
    let mut matches = Vec::new();
    for (process_id, process) in processes {
        let process = process.lock().await;
        if process.owner_id == owner_id && process.session_id.as_deref() == Some(session_id) {
            matches.push(process_id.clone());
        }
    }
    let Some(process_id) = matches.first() else {
        return Err(SidecarError::InvalidState(format!(
            "cannot interrupt ACP session {session_id}: its owner has no live adapter route"
        )));
    };
    if matches.len() > 1 {
        return Err(SidecarError::Conflict(format!(
            "cannot interrupt ACP session {session_id}: its owner has multiple live adapter routes"
        )));
    }
    Ok(process_id.clone())
}

fn encode_cancel_notification_line(session_id: &str) -> Result<Vec<u8>, SidecarError> {
    let mut chunk = serde_json::to_vec(&cancel_notification(session_id)).map_err(|error| {
        SidecarError::InvalidState(format!(
            "failed to serialize ACP cancel notification: {error}"
        ))
    })?;
    chunk.push(b'\n');
    Ok(chunk)
}

async fn cancel_native_permission_wait(
    extension: &AcpExtension,
    wait_key: &NativePermissionWaitKey,
) -> bool {
    let Some(cancellation) = extension.permission_waits.lock().await.remove(wait_key) else {
        return false;
    };
    cancellation.cancel();
    true
}

async fn deliver_native_prompt_cancel(
    io: &mut dyn NativePromptInterruptIo,
    owner_id: &str,
    session_id: &str,
    process_id: &str,
) -> Result<(), SidecarError> {
    let request = WriteStdinRequest {
        process_id: process_id.to_owned(),
        chunk: encode_cancel_notification_line(session_id)?,
    };
    io.write_stdin(request).await.map_err(|error| {
        tracing::error!(
            owner_id,
            session_id,
            process_id,
            error = %error,
            "failed to deliver native ACP session/cancel notification"
        );
        SidecarError::Execution(format!(
            "failed to deliver session/cancel to adapter process {process_id}: {error}"
        ))
    })
}

fn send_native_core_reply<T>(
    reply: NativeCoreReply<T>,
    result: Result<T, AcpCoreError>,
    operation: &'static str,
) {
    if reply.send(result).is_err() {
        tracing::error!(operation, "native ACP core dropped a host broker reply");
    }
}

fn sidecar_to_core_error(error: SidecarError) -> AcpCoreError {
    match error {
        SidecarError::Conflict(message) => AcpCoreError::Conflict(message),
        SidecarError::Unauthorized(message) => AcpCoreError::Unauthorized(message),
        SidecarError::Unsupported(message) => AcpCoreError::Unsupported(message),
        SidecarError::FrameTooLarge(message) => AcpCoreError::LimitExceeded(message),
        SidecarError::LimitExceeded {
            limit,
            capacity,
            how_to_raise,
        } => AcpCoreError::LimitExceeded(format!(
            "{limit} limit exceeded (capacity {capacity}); raise {how_to_raise}"
        )),
        SidecarError::Execution(message)
        | SidecarError::Timeout(message)
        | SidecarError::Kernel(message)
        | SidecarError::Plugin(message)
        | SidecarError::Bridge(message)
        | SidecarError::Io(message) => AcpCoreError::Execution(message),
        SidecarError::Cleanup { context, errors } => AcpCoreError::Cleanup {
            context,
            errors: errors.into_iter().map(sidecar_to_core_error).collect(),
        },
        SidecarError::Context { context, source } => AcpCoreError::Context {
            context,
            source: Box::new(sidecar_to_core_error(*source)),
        },
        SidecarError::SessionNotFound(session_id) => {
            AcpCoreError::SessionNotFound(format!("unknown ACP session {session_id}"))
        }
        SidecarError::InvalidState(message)
        | SidecarError::ProtocolVersionMismatch(message)
        | SidecarError::BridgeVersionMismatch(message) => AcpCoreError::InvalidState(message),
    }
}

fn is_process_already_gone(error: &SidecarError) -> bool {
    let message = error.to_string();
    message.contains("has no active process")
        || message.contains("process already exited")
        || message.contains("process not found")
        || message.contains("ESRCH: no such process")
}

fn decode_guest_file_response(
    response: agentos_native_sidecar::wire::GuestFilesystemResultResponse,
) -> Result<Vec<u8>, AcpCoreError> {
    match response.encoding {
        Some(RootFilesystemEntryEncoding::Base64) => base64::engine::general_purpose::STANDARD
            .decode(response.content.unwrap_or_default())
            .map_err(|error| {
                AcpCoreError::InvalidState(format!(
                    "invalid base64 guest filesystem response: {error}"
                ))
            }),
        _ => Ok(response.content.unwrap_or_default().into_bytes()),
    }
}

struct AcpHandlerOutput {
    response: Result<AcpResponse, SidecarError>,
    events: Vec<agentos_native_sidecar::wire::EventFrame>,
}

impl AcpHandlerOutput {
    fn response(response: Result<AcpResponse, SidecarError>) -> Self {
        Self {
            response,
            events: Vec::new(),
        }
    }
}

fn deliver_event(
    ctx: &ExtensionContext<'_>,
    events: &mut Vec<agentos_native_sidecar::wire::EventFrame>,
    frame: agentos_native_sidecar::wire::EventFrame,
) -> Result<(), SidecarError> {
    if let Some(frame) = ctx.emit_event_wire(frame)? {
        events.push(frame);
    }
    Ok(())
}

fn drain_committed_cleanup_events<T, F>(
    process_id: &str,
    events: &mut VecDeque<T>,
    mut deliver: F,
) -> Result<(), SidecarError>
where
    T: Clone,
    F: FnMut(T) -> Result<(), SidecarError>,
{
    while let Some(event) = events.front().cloned() {
        deliver(event).map_err(|error| SidecarError::Context {
            context: format!("native ACP process {process_id} committed cleanup event delivery"),
            source: Box::new(error),
        })?;
        events.pop_front();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_interrupted_session_response(session_id: &str) -> Option<Vec<u8>> {
    encode_session_rpc_response(
        session_id,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {
                "stopReason": "cancelled",
            },
        }),
        Some(String::new()),
    )
}

fn encode_interrupted_cancel_response(session_id: &str) -> Option<Vec<u8>> {
    encode_session_rpc_response(
        session_id,
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "result": {
                "cancelled": true,
                "requested": true,
                "via": "prompt-interrupt",
            },
        }),
        None,
    )
}

fn encode_session_rpc_response(
    session_id: &str,
    response: Value,
    text: Option<String>,
) -> Option<Vec<u8>> {
    let response = AcpResponse::AcpSessionRpcResponse(
        agentos_protocol::generated::v1::AcpSessionRpcResponse {
            session_id: session_id.to_string(),
            response: serde_json::to_string(&response).ok()?,
            text,
        },
    );
    encode_response(response).ok()
}

async fn build_inbound_response(
    extension: &AcpExtension,
    ctx: &mut ExtensionContext<'_>,
    process_id: &str,
    process: Option<&NativeCoreProcess>,
    message: &Value,
) -> Result<Value, SidecarError> {
    let id = message.get("id").cloned().ok_or_else(|| {
        SidecarError::InvalidState(String::from("ACP inbound request missing id"))
    })?;
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(unsupported_inbound_request_response(message));
    };
    let response = match process.and_then(|process| process.session_id.as_deref()) {
        None => unsupported_inbound_request_response(message),
        Some(session_id) => match method {
            "session/request_permission" => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let permission_id = to_record(params.clone())
                    .get("permissionId")
                    .and_then(Value::as_str)
                    .unwrap_or("permission")
                    .to_string();
                let callback = AcpCallback::AcpPermissionCallback(AcpPermissionCallback {
                    session_id: session_id.to_string(),
                    permission_id: permission_id.clone(),
                    params: serde_json::to_string(&params).map_err(|error| {
                        SidecarError::InvalidState(format!(
                            "failed to serialize ACP permission params: {error}"
                        ))
                    })?,
                    cleanup_after_ms: u64::try_from(
                        (PERMISSION_CALLBACK_TIMEOUT + PERMISSION_CALLBACK_CLEANUP_GRACE)
                            .as_millis(),
                    )
                    .expect("permission callback cleanup deadline must fit u64 milliseconds"),
                });
                let process = process.expect("permission request has session process metadata");
                let wait_key = NativePermissionWaitKey {
                    owner_id: process.owner_id.clone(),
                    session_id: session_id.to_string(),
                    process_id: process_id.to_string(),
                };
                let cancellation = ExtensionCallbackCancellation::default();
                if extension
                    .permission_waits
                    .lock()
                    .await
                    .insert(wait_key.clone(), cancellation.clone())
                    .is_some()
                {
                    return Err(SidecarError::Conflict(format!(
                        "ACP permission callback already pending for session {session_id} process {process_id}"
                    )));
                }
                let snapshot = ctx.snapshot();
                let callback_payload = encode_callback(callback)?;
                let cancellation_for_wait = cancellation.clone();
                let callback_worker = tokio::task::spawn_blocking(move || {
                    snapshot.invoke_callback_cancellable(
                        callback_payload,
                        PERMISSION_CALLBACK_TIMEOUT,
                        &cancellation_for_wait,
                    )
                })
                .await;
                let mut waits = extension.permission_waits.lock().await;
                if waits
                    .get(&wait_key)
                    .is_some_and(|active| active.same_instance(&cancellation))
                {
                    waits.remove(&wait_key);
                }
                drop(waits);
                let callback_result = callback_worker.map_err(|error| {
                    SidecarError::Execution(format!(
                        "ACP permission callback worker failed: {error}"
                    ))
                })?;
                let reply = permission_callback_reply_from_result(callback_result)?;
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": permission_result(&reply, &params),
                })
            }
            "fs/read" | "fs/read_text_file" | "fs/write" | "fs/write_text_file" | "fs/readDir"
            | "fs/read_dir" => handle_native_filesystem_request(ctx, message, &id, method).await,
            "terminal/create"
            | "terminal/write"
            | "terminal/output"
            | "terminal/read"
            | "terminal/wait_for_exit"
            | "terminal/waitForExit"
            | "terminal/kill"
            | "terminal/release"
            | "terminal/close"
            | "terminal/resize" => {
                handle_native_terminal_request(extension, ctx, session_id, message, &id, method)
                    .await
            }
            _ => unsupported_inbound_request_response(message),
        },
    };
    Ok(response)
}

async fn handle_native_filesystem_request(
    ctx: &mut ExtensionContext<'_>,
    message: &Value,
    id: &Value,
    method: &str,
) -> Value {
    let empty_params = Map::new();
    let params = match message.get("params") {
        None | Some(Value::Null) => &empty_params,
        Some(Value::Object(params)) => params,
        Some(_) => {
            return json_rpc_error(
                id.clone(),
                -32602,
                format!("{method} requires object params"),
                None,
            );
        }
    };
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        return json_rpc_error(
            id.clone(),
            -32602,
            format!("{method} requires a string path"),
            None,
        );
    };
    let encoding = match optional_string_param(params, "encoding", method) {
        Ok(encoding) => encoding,
        Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
    };

    let result = match method {
        "fs/read" | "fs/read_text_file" => {
            let line = match optional_f64_param(params, "line", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let limit = match optional_f64_param(params, "limit", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            match ctx
                .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                    operation: GuestFilesystemOperation::ReadFile,
                    path: path.to_owned(),
                    destination_path: None,
                    target: None,
                    content: None,
                    encoding: None,
                    recursive: None,
                    max_depth: None,
                    mode: None,
                    uid: None,
                    gid: None,
                    atime_ms: None,
                    mtime_ms: None,
                    len: None,
                    offset: None,
                })
                .await
            {
                Ok(response) => {
                    let bytes = match response.encoding {
                        Some(RootFilesystemEntryEncoding::Base64) => {
                            match base64::engine::general_purpose::STANDARD
                                .decode(response.content.unwrap_or_default())
                            {
                                Ok(bytes) => bytes,
                                Err(error) => {
                                    return json_rpc_error(
                                        id.clone(),
                                        -32603,
                                        format!("invalid base64 filesystem response: {error}"),
                                        None,
                                    );
                                }
                            }
                        }
                        _ => response.content.unwrap_or_default().into_bytes(),
                    };
                    let content = if encoding.as_deref() == Some("base64") {
                        base64::engine::general_purpose::STANDARD.encode(bytes)
                    } else {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        slice_text_lines(text, line, limit)
                    };
                    json!({ "content": content })
                }
                Err(error) => return filesystem_error_response(id, error),
            }
        }
        "fs/write" | "fs/write_text_file" => {
            let Some(content) = params.get("content").and_then(Value::as_str) else {
                return json_rpc_error(
                    id.clone(),
                    -32602,
                    format!("{method} requires a string content"),
                    None,
                );
            };
            let wire_encoding = if encoding.as_deref() == Some("base64") {
                RootFilesystemEntryEncoding::Base64
            } else {
                RootFilesystemEntryEncoding::Utf8
            };
            match ctx
                .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                    operation: GuestFilesystemOperation::WriteFile,
                    path: path.to_owned(),
                    destination_path: None,
                    target: None,
                    content: Some(content.to_owned()),
                    encoding: Some(wire_encoding),
                    recursive: None,
                    max_depth: None,
                    mode: None,
                    uid: None,
                    gid: None,
                    atime_ms: None,
                    mtime_ms: None,
                    len: None,
                    offset: None,
                })
                .await
            {
                Ok(_) => Value::Null,
                Err(error) => return filesystem_error_response(id, error),
            }
        }
        "fs/readDir" | "fs/read_dir" => match ctx
            .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::ReadDir,
                path: path.to_owned(),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: None,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            })
            .await
        {
            Ok(response) => json!({
                "entries": response.entries.unwrap_or_default().into_iter()
                    .filter(|entry| entry.name != "." && entry.name != "..")
                    .map(|entry| json!({
                        "name": entry.name,
                        "path": entry.path,
                        "type": if entry.is_symbolic_link {
                            "symlink"
                        } else if entry.is_directory {
                            "directory"
                        } else {
                            "file"
                        },
                    }))
                    .collect::<Vec<_>>(),
            }),
            Err(error) => return filesystem_error_response(id, error),
        },
        _ => unreachable!("filesystem method matched by caller"),
    };

    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

async fn handle_native_terminal_request(
    extension: &AcpExtension,
    ctx: &mut ExtensionContext<'_>,
    session_id: &str,
    message: &Value,
    id: &Value,
    method: &str,
) -> Value {
    let empty_params = Map::new();
    let params = match message.get("params") {
        None | Some(Value::Null) => &empty_params,
        Some(Value::Object(params)) => params,
        Some(_) => {
            return json_rpc_error(
                id.clone(),
                -32602,
                format!("{method} requires object params"),
                None,
            );
        }
    };

    let result = match method {
        "terminal/create" => {
            let Some(command) = params.get("command").and_then(Value::as_str) else {
                return json_rpc_error(
                    id.clone(),
                    -32602,
                    String::from("terminal/create requires a string command"),
                    None,
                );
            };
            let args = match optional_string_array_param(params, "args", method) {
                Ok(value) => value.unwrap_or_default(),
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let env = match optional_string_map_param(params, "env", method) {
                Ok(value) => value.unwrap_or_default(),
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let cwd = match optional_string_param(params, "cwd", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let cols = match optional_positive_u16_param(params, "cols", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let rows = match optional_positive_u16_param(params, "rows", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let output_byte_limit = match optional_nonnegative_usize_param(
                params,
                "outputByteLimit",
                method,
            ) {
                Ok(Some(value)) if value <= MAX_ACP_TERMINAL_OUTPUT_BYTE_LIMIT => value,
                Ok(Some(value)) => {
                    return json_rpc_error(
                        id.clone(),
                        -32602,
                        format!(
                            "terminal/create outputByteLimit {value} exceeds the sidecar limit {MAX_ACP_TERMINAL_OUTPUT_BYTE_LIMIT}; lower outputByteLimit"
                        ),
                        None,
                    );
                }
                Ok(None) => DEFAULT_ACP_TERMINAL_OUTPUT_BYTE_LIMIT,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };

            let process_id = extension.allocate_process_id("acp-terminal");
            if let Err(error) = ctx
                .spawn_process_wire(ExecuteRequest {
                    process_id: Some(process_id.clone()),
                    command: Some(command.to_owned()),
                    shell_command: None,
                    runtime: None,
                    entrypoint: None,
                    args,
                    env: Some(env.into_iter().collect()),
                    cwd,
                    wasm_permission_tier: None,
                    pty: Some(agentos_native_sidecar::wire::PtyOptions { cols, rows }),
                    keep_stdin_open: Some(true),
                    timeout_ms: None,
                    capture_output: None,
                })
                .await
            {
                return json_rpc_error(id.clone(), -32603, error.to_string(), None);
            }
            if let Err(error) = ctx.start_buffering_process_output(&process_id).await {
                kill_process_best_effort(ctx, &process_id).await;
                return json_rpc_error(id.clone(), -32603, error.to_string(), None);
            }
            if let Err(error) = ctx.bind_process_to_session(session_id, &process_id).await {
                if let Err(cleanup_error) = ctx.stop_buffering_process_output(&process_id).await {
                    tracing::warn!(
                        process_id,
                        %cleanup_error,
                        "failed to stop ACP terminal output buffering after bind failure"
                    );
                }
                kill_process_best_effort(ctx, &process_id).await;
                return json_rpc_error(id.clone(), -32603, error.to_string(), None);
            }

            let terminal_number = extension.next_terminal_id.fetch_add(1, Ordering::SeqCst) + 1;
            let terminal_id = format!("acp-terminal-{terminal_number}");
            extension.terminals.lock().await.insert(
                terminal_id.clone(),
                NativeAcpTerminal {
                    ownership: ctx.ownership().clone(),
                    session_id: session_id.to_owned(),
                    process_id,
                    output: Vec::new(),
                    truncated: false,
                    output_byte_limit,
                    exit_code: None,
                },
            );
            json!({ "terminalId": terminal_id })
        }
        "terminal/write" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let process_id = match terminal_process_id(
                extension,
                ctx.ownership(),
                session_id,
                terminal_id,
            )
            .await
            {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let Some(data) = params.get("data").and_then(Value::as_str) else {
                return json_rpc_error(
                    id.clone(),
                    -32602,
                    String::from("terminal/write requires string data"),
                    None,
                );
            };
            let encoding = match optional_string_param(params, "encoding", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let chunk = if encoding.as_deref() == Some("base64") {
                match base64::engine::general_purpose::STANDARD.decode(data) {
                    Ok(value) => value,
                    Err(error) => {
                        return json_rpc_error(
                            id.clone(),
                            -32602,
                            format!("terminal/write received invalid base64 data: {error}"),
                            None,
                        );
                    }
                }
            } else {
                data.as_bytes().to_vec()
            };
            match ctx
                .write_stdin_wire(WriteStdinRequest { process_id, chunk })
                .await
            {
                Ok(_) => Value::Null,
                Err(error) => return json_rpc_error(id.clone(), -32603, error.to_string(), None),
            }
        }
        "terminal/output" | "terminal/read" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            if let Err(error) =
                refresh_native_terminal(extension, ctx, session_id, terminal_id, Duration::ZERO)
                    .await
            {
                return json_rpc_error(id.clone(), error.0, error.1, None);
            }
            let terminals = extension.terminals.lock().await;
            let terminal = terminals
                .get(terminal_id)
                .expect("refreshed terminal remains registered");
            let mut output = json!({
                "output": String::from_utf8_lossy(&terminal.output),
                "truncated": terminal.truncated,
            });
            if let Some(exit_code) = terminal.exit_code {
                output["exitStatus"] = json!({ "exitCode": exit_code, "signal": Value::Null });
            }
            output
        }
        "terminal/wait_for_exit" | "terminal/waitForExit" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value.to_owned(),
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            loop {
                if let Err(error) = refresh_native_terminal(
                    extension,
                    ctx,
                    session_id,
                    &terminal_id,
                    ACP_TERMINAL_OUTPUT_POLL_INTERVAL,
                )
                .await
                {
                    return json_rpc_error(id.clone(), error.0, error.1, None);
                }
                let exit_code = extension
                    .terminals
                    .lock()
                    .await
                    .get(&terminal_id)
                    .and_then(|terminal| terminal.exit_code);
                if let Some(exit_code) = exit_code {
                    break json!({ "exitCode": exit_code, "signal": Value::Null });
                }
            }
        }
        "terminal/kill" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let process_id = match terminal_process_id(
                extension,
                ctx.ownership(),
                session_id,
                terminal_id,
            )
            .await
            {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let signal = match optional_f64_param(params, "signal", method) {
                Ok(Some(value)) => value.trunc() as i32,
                Ok(None) => 15,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            if !(0..=31).contains(&signal) {
                return json_rpc_error(
                    id.clone(),
                    -32602,
                    format!("terminal/kill does not support signal {signal}"),
                    None,
                );
            }
            match ctx
                .kill_process_wire(KillProcessRequest {
                    process_id,
                    signal: signal.to_string(),
                })
                .await
            {
                Ok(_) => Value::Null,
                Err(error) => return json_rpc_error(id.clone(), -32603, error.to_string(), None),
            }
        }
        "terminal/resize" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let process_id = match terminal_process_id(
                extension,
                ctx.ownership(),
                session_id,
                terminal_id,
            )
            .await
            {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let cols = match required_positive_u16_param(params, "cols", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let rows = match required_positive_u16_param(params, "rows", method) {
                Ok(value) => value,
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            match ctx
                .resize_pty_wire(ResizePtyRequest {
                    process_id,
                    cols,
                    rows,
                })
                .await
            {
                Ok(_) => Value::Null,
                Err(error) => return json_rpc_error(id.clone(), -32603, error.to_string(), None),
            }
        }
        "terminal/release" | "terminal/close" => {
            let terminal_id = match required_terminal_id(params, method) {
                Ok(value) => value.to_owned(),
                Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
            };
            let process_id =
                match terminal_process_id(extension, ctx.ownership(), session_id, &terminal_id)
                    .await
                {
                    Ok(value) => value,
                    Err(message) => return json_rpc_error(id.clone(), -32602, message, None),
                };
            let running = extension
                .terminals
                .lock()
                .await
                .get(&terminal_id)
                .is_some_and(|terminal| terminal.exit_code.is_none());
            if running {
                if let Err(error) = ctx
                    .kill_process_wire(KillProcessRequest {
                        process_id: process_id.clone(),
                        signal: String::from("SIGTERM"),
                    })
                    .await
                {
                    if !is_process_already_gone(&error) {
                        return json_rpc_error(id.clone(), -32603, error.to_string(), None);
                    }
                }
            }
            if let Err(error) = ctx.stop_buffering_process_output(&process_id).await {
                return json_rpc_error(id.clone(), -32603, error.to_string(), None);
            }
            extension.terminals.lock().await.remove(&terminal_id);
            Value::Null
        }
        _ => unreachable!("terminal method matched by caller"),
    };

    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

async fn terminal_process_id(
    extension: &AcpExtension,
    ownership: &OwnershipScope,
    session_id: &str,
    terminal_id: &str,
) -> Result<String, String> {
    extension
        .terminals
        .lock()
        .await
        .get(terminal_id)
        .filter(|terminal| terminal.ownership == *ownership && terminal.session_id == session_id)
        .map(|terminal| terminal.process_id.clone())
        .ok_or_else(|| format!("ACP terminal not found: {terminal_id}"))
}

async fn refresh_native_terminal(
    extension: &AcpExtension,
    ctx: &mut ExtensionContext<'_>,
    session_id: &str,
    terminal_id: &str,
    timeout: Duration,
) -> Result<(), (i64, String)> {
    let process_id = terminal_process_id(extension, ctx.ownership(), session_id, terminal_id)
        .await
        .map_err(|message| (-32602, message))?;
    let drained = ctx
        .drain_buffered_process_output(&process_id, timeout)
        .await
        .map_err(|error| (-32603, error.to_string()))?;
    let mut terminals = extension.terminals.lock().await;
    let terminal = terminals
        .get_mut(terminal_id)
        .ok_or_else(|| (-32602, format!("ACP terminal not found: {terminal_id}")))?;
    append_native_terminal_output(terminal, &drained.stdout);
    append_native_terminal_output(terminal, &drained.stderr);
    terminal.truncated |= drained.stdout_truncated || drained.stderr_truncated;
    if drained.exit_code.is_some() {
        terminal.exit_code = drained.exit_code;
    }
    Ok(())
}

fn append_native_terminal_output(terminal: &mut NativeAcpTerminal, chunk: &[u8]) {
    if chunk.is_empty() {
        return;
    }
    terminal.output.extend_from_slice(chunk);
    if terminal.output.len() > terminal.output_byte_limit {
        let remove_len = terminal.output.len() - terminal.output_byte_limit;
        terminal.output.drain(..remove_len);
        terminal.truncated = true;
    }
}

fn required_terminal_id<'a>(
    params: &'a Map<String, Value>,
    method: &str,
) -> Result<&'a str, String> {
    params
        .get("terminalId")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{method} requires a string terminalId"))
}

fn optional_string_array_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<Vec<String>>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{method} requires {name} entries to be strings"))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(format!("{method} requires {name} to be an array")),
    }
}

fn optional_string_map_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<BTreeMap<String, String>>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(values)) => values
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or_else(|| format!("{method} requires {name} values to be strings"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(Some),
        Some(_) => Err(format!("{method} requires {name} to be an object")),
    }
}

fn optional_nonnegative_usize_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<usize>, String> {
    let value = optional_f64_param(params, name, method)?;
    value
        .map(|value| {
            if value < 0.0 || value > usize::MAX as f64 {
                Err(format!(
                    "{method} requires {name} to be a non-negative integer"
                ))
            } else {
                Ok(value.trunc() as usize)
            }
        })
        .transpose()
}

fn optional_positive_u16_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<u16>, String> {
    let value = optional_f64_param(params, name, method)?;
    value
        .map(|value| {
            if value < 1.0 || value > f64::from(u16::MAX) {
                Err(format!(
                    "{method} requires {name} to be between 1 and {}",
                    u16::MAX
                ))
            } else {
                Ok(value.trunc() as u16)
            }
        })
        .transpose()
}

fn required_positive_u16_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<u16, String> {
    optional_positive_u16_param(params, name, method)?
        .ok_or_else(|| format!("{method} requires numeric {name}"))
}

fn optional_string_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<String>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!(
            "{method} requires {name} to be a string when provided"
        )),
    }
}

fn optional_f64_param(
    params: &Map<String, Value>,
    name: &str,
    method: &str,
) -> Result<Option<f64>, String> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or_else(|| format!("{method} requires {name} to be a number when provided")),
        Some(_) => Err(format!(
            "{method} requires {name} to be a number when provided"
        )),
    }
}

fn slice_text_lines(text: String, line: Option<f64>, limit: Option<f64>) -> String {
    if line.is_none() && limit.is_none() {
        return text;
    }
    let start = line.unwrap_or(1.0).trunc().max(1.0) as usize - 1;
    let limit = limit
        .map(|value| value.trunc().max(0.0) as usize)
        .unwrap_or(usize::MAX);
    text.split('\n')
        .skip(start)
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

fn filesystem_error_response(id: &Value, error: SidecarError) -> Value {
    let message = error.to_string();
    let code = message
        .split(|ch: char| ch == ':' || ch.is_whitespace())
        .find(|part| {
            part.len() >= 2
                && part.starts_with('E')
                && part[1..]
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .map(ToOwned::to_owned);
    json_rpc_error(
        id.clone(),
        -32603,
        message,
        code.map(|code| json!({ "code": code })),
    )
}

fn json_rpc_error(id: Value, code: i64, message: String, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn permission_result(reply: &str, params: &Value) -> Value {
    let option_id = match resolve_permission_option_id(params, reply) {
        Some(option_id) => option_id,
        None => match reply {
            "always" | "allow_always" => String::from("allow_always"),
            "once" | "allow_once" => String::from("allow_once"),
            "reject" | "reject_once" => String::from("reject_once"),
            _ => return json!({ "outcome": { "outcome": "cancelled" } }),
        },
    };
    json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
}

fn permission_callback_reply(response: AcpCallbackResponse) -> String {
    let AcpCallbackResponse::AcpPermissionCallbackResponse(response) = response;
    response.reply.unwrap_or_else(|| String::from("reject"))
}

fn permission_callback_reply_from_result(
    response: Result<Vec<u8>, SidecarError>,
) -> Result<String, SidecarError> {
    let response = match response {
        Ok(response) => response,
        Err(SidecarError::Timeout(message)) => {
            tracing::warn!(%message, "ACP permission callback timed out; applying sidecar default");
            return Ok(String::from("reject"));
        }
        Err(error) => return Err(error),
    };
    let response: AcpCallbackResponse = serde_bare::from_slice(&response).map_err(|error| {
        SidecarError::InvalidState(format!("invalid ACP callback response: {error}"))
    })?;
    Ok(permission_callback_reply(response))
}

fn resolve_permission_option_id(params: &Value, reply: &str) -> Option<String> {
    let targets = match reply {
        "always" | "allow_always" => (&["always", "allow_always"][..], "allow_always"),
        "once" | "allow_once" => (&["once", "allow_once"][..], "allow_once"),
        "reject" | "reject_once" => (&["reject", "reject_once"][..], "reject_once"),
        _ => return None,
    };
    let options = params.get("options")?.as_array()?;
    let matched = options.iter().find(|option| {
        let option_id_matches = option
            .get("optionId")
            .and_then(Value::as_str)
            .map(|value| targets.0.contains(&value))
            .unwrap_or(false);
        let kind_matches = option
            .get("kind")
            .and_then(Value::as_str)
            .map(|value| value == targets.1)
            .unwrap_or(false);
        option_id_matches || kind_matches
    })?;
    matched
        .get("optionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn convert_runtime(runtime: AcpRuntimeKind) -> GuestRuntimeKind {
    match runtime {
        AcpRuntimeKind::JavaScript => GuestRuntimeKind::JavaScript,
        AcpRuntimeKind::Python => GuestRuntimeKind::Python,
        AcpRuntimeKind::WebAssembly => GuestRuntimeKind::WebAssembly,
    }
}

fn ownership_owner_id(ownership: &OwnershipScope) -> String {
    match ownership {
        OwnershipScope::ConnectionOwnership(inner) => {
            format!(
                "native:{}:{}",
                inner.connection_id.len(),
                inner.connection_id
            )
        }
        OwnershipScope::SessionOwnership(inner) => format!(
            "native:{}:{}:{}:{}",
            inner.connection_id.len(),
            inner.connection_id,
            inner.session_id.len(),
            inner.session_id,
        ),
        OwnershipScope::VmOwnership(inner) => format!(
            "native:{}:{}:{}:{}:{}:{}",
            inner.connection_id.len(),
            inner.connection_id,
            inner.session_id.len(),
            inner.session_id,
            inner.vm_id.len(),
            inner.vm_id,
        ),
    }
}

fn ownership_session_identity(ownership: &OwnershipScope) -> (String, Option<String>) {
    match ownership {
        OwnershipScope::ConnectionOwnership(inner) => (inner.connection_id.clone(), None),
        OwnershipScope::SessionOwnership(inner) => {
            (inner.connection_id.clone(), Some(inner.session_id.clone()))
        }
        OwnershipScope::VmOwnership(inner) => {
            (inner.connection_id.clone(), Some(inner.session_id.clone()))
        }
    }
}

fn registered_owner_ids_for_session(
    owners: &BTreeMap<String, NativeCoreOwner>,
    connection_id: &str,
    wire_session_id: Option<&str>,
) -> std::collections::BTreeSet<String> {
    owners
        .iter()
        .filter(|(_, owner)| {
            owner.connection_id == connection_id
                && owner.wire_session_id.as_deref() == wire_session_id
        })
        .map(|(owner_id, _)| owner_id.clone())
        .collect()
}

fn to_record(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => Map::from_iter([(String::from("value"), other)]),
    }
}

async fn kill_process_best_effort(ctx: &mut ExtensionContext<'_>, process_id: &str) {
    if let Err(error) = ctx
        .kill_process_wire(KillProcessRequest {
            process_id: process_id.to_owned(),
            signal: String::from("SIGTERM"),
        })
        .await
    {
        if !is_process_already_gone(&error) {
            tracing::warn!(process_id, %error, "failed to clean up ACP process");
        }
    }
}

/// Return the adapter native-resume RPC method from re-probed
/// `agentCapabilities`. Prefer ACP `loadSession`/`session/load`; fall back to the
/// non-standard `resume`/`session/resume` capability some adapters expose.
fn decode_request(payload: &[u8]) -> Result<AcpRequest, SidecarError> {
    serde_bare::from_slice(payload)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP request: {error}")))
}

fn encode_response(response: AcpResponse) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&response)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP response: {error}")))
}

fn encode_event(event: AcpEvent) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&event)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP event: {error}")))
}

fn encode_callback(callback: AcpCallback) -> Result<Vec<u8>, SidecarError> {
    serde_bare::to_vec(&callback)
        .map_err(|error| SidecarError::InvalidState(format!("invalid ACP callback: {error}")))
}

fn error_response(error: SidecarError) -> AcpResponse {
    AcpResponse::AcpErrorResponse(AcpErrorResponse {
        code: error_code(&error),
        message: error.to_string(),
    })
}

fn error_code(error: &SidecarError) -> String {
    let code = match error {
        SidecarError::InvalidState(_) => "invalid_state",
        SidecarError::SessionNotFound(_) => "session_not_found",
        SidecarError::ProtocolVersionMismatch(_) => "protocol_version_mismatch",
        SidecarError::BridgeVersionMismatch(_) => "bridge_version_mismatch",
        SidecarError::Conflict(_) => "conflict",
        SidecarError::Unauthorized(_) => "unauthorized",
        SidecarError::Unsupported(_) => "unsupported",
        SidecarError::FrameTooLarge(_) => "frame_too_large",
        SidecarError::LimitExceeded { .. } => "limit_exceeded",
        SidecarError::Timeout(_) => "timeout",
        SidecarError::Kernel(_) => "kernel",
        SidecarError::Plugin(_) => "plugin",
        SidecarError::Execution(_) => "execution",
        SidecarError::Bridge(_) => "bridge",
        SidecarError::Io(_) => "io",
        SidecarError::Cleanup { .. } => "cleanup_failed",
        SidecarError::Context { source, .. } => return error_code(source),
    };
    String::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingInterruptIo {
        writes: Vec<WriteStdinRequest>,
        fail_write: bool,
    }

    impl NativePromptInterruptIo for RecordingInterruptIo {
        fn write_stdin<'a>(&'a mut self, request: WriteStdinRequest) -> ExtensionFuture<'a, ()> {
            Box::pin(async move {
                self.writes.push(request);
                if self.fail_write {
                    Err(SidecarError::Io(String::from("adapter stdin closed")))
                } else {
                    Ok(())
                }
            })
        }
    }

    fn test_core_process(owner_id: &str, session_id: &str) -> Arc<Mutex<NativeCoreProcess>> {
        Arc::new(Mutex::new(NativeCoreProcess {
            owner_id: owner_id.to_owned(),
            session_id: Some(session_id.to_owned()),
            pending_output: VecDeque::new(),
            exit_observed: false,
            pending_cleanup_events: VecDeque::new(),
            cleanup: NativeRouteCleanupProgress::default(),
        }))
    }

    #[test]
    fn acp_extension_uses_agent_os_namespace() {
        assert_eq!(AcpExtension::new().namespace(), ACP_EXTENSION_NAMESPACE);
    }

    #[test]
    fn classifies_kernel_esrch_as_an_already_exited_process() {
        assert!(is_process_already_gone(&SidecarError::Kernel(
            String::from("ESRCH: no such process 4"),
        )));
    }

    #[tokio::test]
    async fn stalled_native_owner_does_not_lock_unrelated_owner_core() {
        let extension = AcpExtension::new();
        let owner_a = extension.core_for_owner("owner-a").await;
        let owner_b = extension.core_for_owner("owner-b").await;
        assert!(!Arc::ptr_eq(&owner_a, &owner_b));

        let _stalled_owner = owner_a.lock().expect("lock owner A core");
        let response = owner_b
            .lock()
            .expect("owner B core remains independently available")
            .list_sessions("owner-b");
        let AcpResponse::AcpListSessionsResponse(response) = response else {
            panic!("unexpected owner B response");
        };
        assert!(response.sessions.is_empty());
    }

    #[test]
    fn session_disposal_uses_authoritative_owner_registry_without_process_routes() {
        let owners = BTreeMap::from([
            (
                String::from("owner-a"),
                NativeCoreOwner {
                    connection_id: String::from("connection-a"),
                    wire_session_id: Some(String::from("wire-session-a")),
                },
            ),
            (
                String::from("owner-b"),
                NativeCoreOwner {
                    connection_id: String::from("connection-a"),
                    wire_session_id: Some(String::from("wire-session-b")),
                },
            ),
        ]);

        assert_eq!(
            registered_owner_ids_for_session(&owners, "connection-a", Some("wire-session-a")),
            std::collections::BTreeSet::from([String::from("owner-a")])
        );
    }

    #[test]
    fn missing_client_permission_reply_uses_sidecar_default() {
        assert_eq!(
            permission_callback_reply(AcpCallbackResponse::AcpPermissionCallbackResponse(
                agentos_protocol::generated::v1::AcpPermissionCallbackResponse {
                    permission_id: String::from("permission-1"),
                    reply: None,
                },
            )),
            "reject"
        );
        assert_eq!(
            permission_callback_reply(AcpCallbackResponse::AcpPermissionCallbackResponse(
                agentos_protocol::generated::v1::AcpPermissionCallbackResponse {
                    permission_id: String::from("permission-2"),
                    reply: Some(String::from("once")),
                },
            )),
            "once"
        );
    }

    #[test]
    fn permission_results_preserve_adapter_option_ids_for_all_reply_aliases() {
        let params = json!({
            "options": [
                { "optionId": "once", "kind": "allow_once" },
                { "optionId": "always", "kind": "allow_always" },
                { "optionId": "reject", "kind": "reject_once" },
            ],
        });

        for (reply, expected_option_id) in [
            ("once", "once"),
            ("allow_once", "once"),
            ("always", "always"),
            ("allow_always", "always"),
            ("reject", "reject"),
            ("reject_once", "reject"),
        ] {
            assert_eq!(
                permission_result(reply, &params),
                json!({
                    "outcome": {
                        "outcome": "selected",
                        "optionId": expected_option_id,
                    },
                }),
                "reply alias {reply} must select the adapter-provided option id"
            );
        }

        assert_eq!(
            permission_result("unknown", &params),
            json!({ "outcome": { "outcome": "cancelled" } })
        );
    }

    #[test]
    fn only_callback_timeout_uses_sidecar_permission_default() {
        assert_eq!(
            permission_callback_reply_from_result(Err(SidecarError::Timeout(String::from(
                "host permission callback deadline elapsed",
            ))))
            .expect("timeout uses the sidecar default"),
            "reject"
        );
        assert!(matches!(
            permission_callback_reply_from_result(Err(SidecarError::Io(String::from(
                "host callback transport failed",
            )))),
            Err(SidecarError::Io(message)) if message == "host callback transport failed"
        ));
    }

    #[tokio::test]
    async fn cancel_notification_is_idless_and_targets_exact_live_adapter() {
        let extension = AcpExtension::new();
        let owner_id = "owner-a";
        let session_id = "agent-session";
        {
            let mut processes = extension.core_processes.lock().await;
            processes.insert(
                (String::from("owner-b"), String::from("wrong-owner-process")),
                test_core_process("owner-b", session_id),
            );
            processes.insert(
                (String::from(owner_id), String::from("exact-process")),
                test_core_process(owner_id, session_id),
            );
        }
        let process_id = {
            let processes = extension.core_processes.lock().await;
            processes
                .iter()
                .filter(|((route_owner_id, _), _)| route_owner_id == owner_id)
                .map(|((_, process_id), process)| (process_id.clone(), Arc::clone(process)))
                .collect::<Vec<_>>()
        };
        let process_id = native_prompt_interrupt_target(&process_id, owner_id, session_id)
            .await
            .expect("exact owner and ACP session route");
        assert_eq!(process_id, "exact-process");

        let wait_key = NativePermissionWaitKey {
            owner_id: owner_id.to_owned(),
            session_id: session_id.to_owned(),
            process_id: process_id.clone(),
        };
        let cancellation = ExtensionCallbackCancellation::default();
        extension
            .permission_waits
            .lock()
            .await
            .insert(wait_key.clone(), cancellation.clone());
        assert!(cancel_native_permission_wait(&extension, &wait_key).await);
        assert!(cancellation.is_cancelled());
        assert!(extension.permission_waits.lock().await.is_empty());

        let mut io = RecordingInterruptIo::default();
        deliver_native_prompt_cancel(&mut io, owner_id, session_id, &process_id)
            .await
            .expect("cancel delivery");
        assert_eq!(io.writes.len(), 1);
        assert_eq!(io.writes[0].process_id, "exact-process");
        assert_eq!(io.writes[0].chunk.last(), Some(&b'\n'));
        let notification: Value =
            serde_json::from_slice(&io.writes[0].chunk[..io.writes[0].chunk.len() - 1])
                .expect("JSON-RPC notification line");
        assert_eq!(notification.get("jsonrpc"), Some(&json!("2.0")));
        assert_eq!(notification.get("method"), Some(&json!("session/cancel")));
        assert_eq!(
            notification.pointer("/params/sessionId"),
            Some(&json!(session_id))
        );
        assert!(notification.get("id").is_none());
    }

    #[tokio::test]
    async fn ambiguous_adapter_routes_fail_closed() {
        let extension = AcpExtension::new();
        let mut processes = extension.core_processes.lock().await;
        processes.insert(
            (String::from("owner-a"), String::from("process-a")),
            test_core_process("owner-a", "agent-session"),
        );
        processes.insert(
            (String::from("owner-a"), String::from("process-b")),
            test_core_process("owner-a", "agent-session"),
        );
        let owner_routes = processes
            .iter()
            .map(|((_, process_id), process)| (process_id.clone(), Arc::clone(process)))
            .collect::<Vec<_>>();
        drop(processes);
        assert!(matches!(
            native_prompt_interrupt_target(&owner_routes, "owner-a", "agent-session").await,
            Err(SidecarError::Conflict(message)) if message.contains("multiple live adapter routes")
        ));
    }

    #[tokio::test]
    async fn stalled_route_cleanup_does_not_lock_an_unrelated_owner_route() {
        let extension = AcpExtension::new();
        let route_a = test_core_process("owner-a", "session-a");
        let route_b = test_core_process("owner-b", "session-b");
        {
            let mut processes = extension.core_processes.lock().await;
            processes.insert(
                (String::from("owner-a"), String::from("process-a")),
                Arc::clone(&route_a),
            );
            processes.insert(
                (String::from("owner-b"), String::from("process-b")),
                Arc::clone(&route_b),
            );
        }

        let _stalled_cleanup = route_a.lock().await;
        let owner_b = tokio::time::timeout(Duration::from_millis(50), async {
            let route = extension
                .core_processes
                .lock()
                .await
                .get(&(String::from("owner-b"), String::from("process-b")))
                .cloned()
                .expect("owner B route remains registered");
            let owner_id = route.lock().await.owner_id.clone();
            owner_id
        })
        .await
        .expect("owner A route cleanup must not hold the global route registry");
        assert_eq!(owner_b, "owner-b");
    }

    #[test]
    fn committed_cleanup_events_backpressure_without_growth_and_deliver_once() {
        let mut pending = VecDeque::from([1_u8, 2_u8]);
        let error = drain_committed_cleanup_events("process-a", &mut pending, |_| {
            Err(SidecarError::LimitExceeded {
                limit: "test_event_sink",
                capacity: 0,
                how_to_raise: "recover the test sink",
            })
        })
        .expect_err("backpressure keeps the committed batch for retry");
        assert!(error.to_string().contains("process-a"));
        assert_eq!(pending, VecDeque::from([1, 2]));

        let mut delivered = Vec::new();
        drain_committed_cleanup_events("process-a", &mut pending, |event| {
            delivered.push(event);
            Ok(())
        })
        .expect("recovered sink drains the retained batch");
        assert_eq!(delivered, vec![1, 2]);
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn cancel_write_failure_is_propagated_for_shared_core_cleanup() {
        let mut io = RecordingInterruptIo {
            fail_write: true,
            ..RecordingInterruptIo::default()
        };
        let error =
            deliver_native_prompt_cancel(&mut io, "owner-a", "agent-session", "exact-process")
                .await
                .expect_err("delivery failure must reach shared-core cleanup");

        assert_eq!(io.writes.len(), 1);
        assert!(
            matches!(error, SidecarError::Execution(message) if message.contains("adapter stdin closed"))
        );
    }
}
