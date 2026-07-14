//! The host-free ACP engine: owns session state and dispatches ACP requests,
//! driving host-coupled work through the synchronous [`AcpHost`] seam.
//!
//! Ported from `agentos-sidecar::acp_extension` (async) to synchronous, host-free
//! form. All five ACP requests are ported onto the [`AcpHost`] seam: the
//! pure-state ones (`get_session_state`, `close_session`), the bootstrap one
//! (`create_session`), the in-session RPC one (`session_request`, i.e.
//! `session/prompt` + other in-session methods), and `resume_session` (native
//! `session/load` tier with the universal `session/new` fallback). Adapter
//! notifications are queued as ACP events for the embedding sidecar to drain.

use std::collections::{BTreeMap, BTreeSet};

use agentos_protocol::generated::v1::{
    AcpAbortPendingRequest, AcpAgentEntry, AcpAgentExitedEvent, AcpAgentStderrDeliveredResponse,
    AcpAgentStderrEvent, AcpCloseSessionRequest, AcpCreateSessionRequest,
    AcpDeliverAgentOutputRequest, AcpDeliverAgentStderrRequest, AcpErrorResponse, AcpEvent,
    AcpGetSessionStateRequest, AcpListAgentsResponse, AcpListSessionsResponse,
    AcpPendingAbortReason, AcpPendingResponse, AcpRequest, AcpResponse, AcpResumeSessionRequest,
    AcpRuntimeKind, AcpSessionClosedResponse, AcpSessionEntry, AcpSessionEvent, AcpSessionRequest,
    AcpSessionResumedResponse, AcpSessionRpcResponse, AcpSetSessionConfigRequest,
};
use agentos_protocol::{
    read_only_config_message, select_config_by_category, AcpPromptTextAccumulator,
    ResolvedAcpCreateSessionRequest, ResolvedAcpResumeSessionRequest,
    DEFAULT_ACP_CLIENT_CAPABILITIES, DEFAULT_ACP_PROTOCOL_VERSION,
};
use serde_json::{json, Map, Value};

use crate::behavior::{
    apply_successful_session_request, cancel_fallback_decision, cancel_notification,
    cancel_notification_fallback_response, classify_json_rpc_message, derive_bootstrap_fields,
    plan_acp_launch_prompt, AcpCancelFallbackDecision, AcpJsonLineAccumulator,
    AcpJsonRpcMessageKind, AcpLaunchPromptPlan, AcpSessionNotification, AGENTOS_SYSTEM_PROMPT,
    DEFAULT_ACP_MAX_READ_LINE_BYTES,
};
use crate::host::{AcpHost, ProjectedAgentLaunch, SpawnAgentRequest};
use crate::json_rpc::send_json_rpc_exchange;
use crate::session::{AcpAdapterRestartState, AcpSessionRecord};
use crate::AcpCoreError;

/// Matches the native sidecar's `SESSION_CLOSE_TIMEOUT` (5s).
const SESSION_CLOSE_TIMEOUT_MS: u64 = 5_000;
/// Matches the native `INITIALIZE_TIMEOUT` (10s) and `SESSION_NEW_TIMEOUT` (30s).
const INITIALIZE_TIMEOUT_MS: u64 = 10_000;
const SESSION_NEW_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_ACP_PENDING_EVENT_LIMIT: usize = 4_096;
const DEFAULT_ACP_PENDING_EVENT_BYTES_LIMIT: usize = 32 * 1024 * 1024;
const DEFAULT_ACP_PROCESS_ROUTE_LIMIT: usize = 256;
const MAX_ADAPTER_RESTARTS: u32 = 3;

/// Transcript-continuation preamble armed by the resume fallback tier; matches the
/// native `CONTINUATION_PREAMBLE`.
const CONTINUATION_PREAMBLE: &str = "You are continuing an earlier session. The full prior transcript is at `{path}`. Read it with your file tools if you need context before answering.";

enum PreparedSessionConfig {
    Immediate(AcpResponse),
    Forward(AcpSessionRequest),
}

enum AdapterRestartError {
    Unsupported,
    Failed(AcpCoreError),
}

enum OwnerCleanupAction {
    Abort {
        process_id: String,
    },
    Finalize {
        session_id: String,
        process_id: String,
    },
}

impl From<AcpCoreError> for AdapterRestartError {
    fn from(error: AcpCoreError) -> Self {
        Self::Failed(error)
    }
}

/// Resolve an agent name from the host's sidecar-owned projected-package state.
/// Missing metadata maps to the stable unknown-agent error; failures reading the
/// authoritative host state propagate unchanged.
fn resolve_agent<H: AcpHost>(
    host: &mut H,
    agent_type: &str,
) -> Result<ProjectedAgentLaunch, AcpCoreError> {
    let unknown = || {
        AcpCoreError::InvalidState(format!(
            "unknown agent type \"{agent_type}\": no projected /opt/agentos/pkgs/{agent_type} package \
             with an agent.acpEntrypoint — pass its package to AgentOs software"
        ))
    };
    let agent = host
        .resolve_projected_agent(agent_type)?
        .ok_or_else(&unknown)?;
    if agent.adapter_entrypoint.is_empty() {
        return Err(unknown());
    }
    Ok(agent)
}

fn session_storage_key(owner_id: &str, session_id: &str) -> String {
    // Length-prefixing keeps the internal key unambiguous even when caller-owned
    // ids contain separators. The external ACP session id remains unchanged.
    format!("{}:{owner_id}{session_id}", owner_id.len())
}

fn prepare_agent_launch<H: AcpHost>(
    host: &mut H,
    agent_type: &str,
    resolved: &ProjectedAgentLaunch,
    request_args: &[String],
    request_env: &std::collections::HashMap<String, String>,
    skip_base_instructions: bool,
    additional_instructions: Option<&str>,
) -> Result<(Vec<String>, BTreeMap<String, String>), AcpCoreError> {
    let mut env: BTreeMap<String, String> = request_env.clone().into_iter().collect();
    env.insert(String::from("AGENTOS_KEEP_STDIN_OPEN"), String::from("1"));
    for (key, value) in &resolved.env {
        env.entry(key.clone()).or_insert_with(|| value.clone());
    }
    let mut args = resolved.launch_args.clone();
    args.extend(request_args.iter().cloned());
    let tool_reference = host.registered_host_tool_reference()?;
    match plan_acp_launch_prompt(
        agent_type,
        AGENTOS_SYSTEM_PROMPT,
        skip_base_instructions,
        additional_instructions,
        &tool_reference,
        &env,
    )? {
        AcpLaunchPromptPlan::None => {}
        AcpLaunchPromptPlan::AppendArgument { flag, prompt } => {
            args.extend([flag, prompt]);
        }
        AcpLaunchPromptPlan::OpenCodeContext {
            env_key,
            env_value,
            file,
        } => {
            host.write_file(&file.path, file.contents.as_bytes())?;
            env.insert(env_key, env_value);
        }
    }
    Ok((args, env))
}

/// Host-free ACP session engine. The native and browser sidecars each hold one of
/// these and feed it decoded requests plus the caller's connection id (for the
/// per-connection ownership checks) and an [`AcpHost`] for the host-coupled steps.
#[derive(Debug)]
pub struct AcpCore {
    sessions: BTreeMap<String, AcpSessionRecord>,
    next_process_id: u64,
    /// In-flight RESUMABLE create_session handshakes (browser path). The synchronous
    /// `create_session` blocks; the resumable path (begin_create_session +
    /// feed_agent_output) never does, so the single-threaded kernel worker can
    /// release the wasm borrow between steps and service the agent's own syscalls on
    /// fresh, non-nested calls (see AGENTOS-WEB-ASYNC-AGENTS.md §3.2 + the
    /// pushFrame-re-entrancy constraint). Native keeps using the blocking path.
    pending_creates: BTreeMap<String, PendingCreate>,
    /// In-flight RESUMABLE session/prompt (and other in-session RPC) requests,
    /// keyed by the agent's process id.
    pending_prompts: BTreeMap<String, PendingPrompt>,
    /// In-flight RESUMABLE `resume_session` handshakes, keyed by the freshly
    /// spawned adapter's process id.
    pending_resumes: BTreeMap<String, PendingResume>,
    /// In-flight RESUMABLE adapter restarts. Browser hosts cannot synchronously
    /// wait for the replacement adapter because its syscalls must cross a fresh
    /// push-frame boundary, so restart bootstrap uses the same core-owned
    /// continuation model as create/resume.
    pending_restarts: BTreeMap<String, PendingRestart>,
    /// In-flight orderly closes. Embedding adapters drive teardown waits so the
    /// shared core is never locked while an agent exits.
    pending_closes: BTreeMap<String, PendingClose>,
    /// Non-routable process handles retained only while abort cleanup is
    /// retryable. Each entry replaces one pending interaction, so admission is
    /// covered by the process-route limit.
    pending_cleanups: BTreeMap<(String, String), Option<AcpResponse>>,
    pending_restart_cleanups: BTreeMap<(String, String), PendingRestartCleanup>,
    /// Cleanup completions that also evict a close-owned session tombstone.
    cleanup_session_removals: BTreeSet<(String, String)>,
    /// Orderly-close finalizers are distinct from process aborts: signals and
    /// waits have already completed and must not repeat when host resource
    /// cleanup is retried.
    pending_finalizations: BTreeMap<(String, String), String>,
    pending_events: Vec<PendingAcpEvent>,
    default_process_route_limit: usize,
    process_route_limits: BTreeMap<String, usize>,
    process_route_limit_warned: BTreeSet<String>,
    default_pending_event_limit: usize,
    default_pending_event_bytes_limit: usize,
    pending_event_limits: BTreeMap<String, (usize, usize)>,
    pending_event_limit_warned: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct PendingAcpEvent {
    owner_connection_id: String,
    event: AcpEvent,
    encoded_bytes: usize,
}

impl Default for AcpCore {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            next_process_id: 0,
            pending_creates: BTreeMap::new(),
            pending_prompts: BTreeMap::new(),
            pending_resumes: BTreeMap::new(),
            pending_restarts: BTreeMap::new(),
            pending_closes: BTreeMap::new(),
            pending_cleanups: BTreeMap::new(),
            pending_restart_cleanups: BTreeMap::new(),
            cleanup_session_removals: BTreeSet::new(),
            pending_finalizations: BTreeMap::new(),
            pending_events: Vec::new(),
            default_process_route_limit: DEFAULT_ACP_PROCESS_ROUTE_LIMIT,
            process_route_limits: BTreeMap::new(),
            process_route_limit_warned: BTreeSet::new(),
            default_pending_event_limit: DEFAULT_ACP_PENDING_EVENT_LIMIT,
            default_pending_event_bytes_limit: DEFAULT_ACP_PENDING_EVENT_BYTES_LIMIT,
            pending_event_limits: BTreeMap::new(),
            pending_event_limit_warned: BTreeSet::new(),
        }
    }
}

/// State of one in-flight resumable `session/prompt` (or other in-session RPC).
#[derive(Debug)]
struct PendingPrompt {
    owner_connection_id: String,
    session_id: String,
    method: String,
    params: Map<String, Value>,
    rpc_id: i64,
    stdout_buffer: String,
    prompt_text: Option<AcpPromptTextAccumulator>,
    notifications: Vec<AcpSessionNotification>,
    notification_bytes: usize,
    /// One-shot resume preamble consumed when this prompt was written. Restored
    /// if the interaction aborts before the adapter response commits it.
    pending_preamble: Option<String>,
    /// The host cancelled this prompt and is draining through its exact RPC
    /// response boundary. Notifications and prompt text from the cancelled turn
    /// are discarded so they cannot contaminate a later prompt on this session.
    cancelled: bool,
    /// Once an adapter exit is reported, the prompt route becomes cleanup-only.
    /// The nested option preserves an unknown exit status for restart reporting.
    cleanup_restart_exit_code: Option<Option<i32>>,
}

/// State of one in-flight resumable `create_session` handshake.
#[derive(Debug)]
struct PendingCreate {
    owner_connection_id: String,
    agent_type: String,
    pid: Option<u32>,
    protocol_version: i32,
    cwd: String,
    mcp_servers: Value,
    step: CreateStep,
    stdout_buffer: String,
    init_result: Option<Map<String, Value>>,
    notifications: Vec<Value>,
    notification_bytes: usize,
    restart: AcpAdapterRestartState,
}

/// State of one in-flight resumable `resume_session` handshake.
#[derive(Debug)]
struct PendingResume {
    owner_connection_id: String,
    requested_session_id: String,
    agent_type: String,
    pid: Option<u32>,
    cwd: String,
    transcript_path: Option<String>,
    step: PendingResumeStep,
    stdout_buffer: String,
    init_result: Option<Map<String, Value>>,
    agent_capabilities: Option<Value>,
    notifications: Vec<Value>,
    notification_bytes: usize,
    restart: AcpAdapterRestartState,
}

#[derive(Debug, Clone)]
struct PendingRestart {
    owner_connection_id: String,
    session_id: String,
    agent_type: String,
    dead_process_id: String,
    exit_code: Option<i32>,
    pid: Option<u32>,
    step: PendingRestartStep,
    stdout_buffer: String,
    init_result: Option<Map<String, Value>>,
    agent_capabilities: Option<Value>,
    restart: AcpAdapterRestartState,
    cleanup_only: bool,
}

#[derive(Debug, Clone)]
struct PendingRestartCleanup {
    pending: PendingRestart,
    outcome: &'static str,
    detail: String,
    include_exit_error: bool,
    /// The host abort committed. A later event-capacity failure must retry only
    /// the canonical completion, never signal the replacement process twice.
    host_cleanup_complete: bool,
}

#[derive(Debug, Clone)]
struct PendingClose {
    owner_connection_id: String,
    session_id: String,
    step: PendingCloseStep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingCloseStep {
    AwaitingSigterm,
    AwaitingSigkill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingRestartStep {
    AwaitingInitialize,
    AwaitingNative(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingResumeStep {
    AwaitingInitialize,
    AwaitingNative(&'static str),
    AwaitingFallbackSessionNew,
}

struct CompletedResume {
    session_id: String,
    mode: String,
    pending_preamble: Option<String>,
    session_result: Map<String, Value>,
}

#[derive(Debug, PartialEq, Eq)]
enum CreateStep {
    AwaitingInitialize,
    AwaitingSessionNew,
}

/// Outcome of feeding agent output into a resumable handshake.
#[derive(Debug)]
pub enum ResumeStep {
    /// More agent output is needed; the interaction is still in flight.
    Pending,
    /// The interaction completed; deliver this response as the (deferred) result.
    Done(AcpResponse),
}

/// Result of the ACP bootstrap handshake (initialize + session/new).
struct SessionBootstrap {
    session_id: String,
    modes: Option<String>,
    config_options: Vec<String>,
    agent_capabilities: Option<String>,
    agent_info: Option<String>,
    stdout_buffer: String,
    notifications: Vec<Value>,
}

impl AcpCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with a per-owner bound for live and cleanup-only ACP process
    /// routes. A route remains charged until its host cleanup succeeds.
    pub fn with_process_route_limit(limit: usize) -> Result<Self, AcpCoreError> {
        if limit == 0 {
            return Err(AcpCoreError::InvalidState(String::from(
                "ACP process route limit must be greater than zero",
            )));
        }
        Ok(Self {
            default_process_route_limit: limit,
            ..Self::default()
        })
    }

    pub fn set_process_route_limit(
        &mut self,
        owner_connection_id: &str,
        limit: usize,
    ) -> Result<(), AcpCoreError> {
        if limit == 0 {
            return Err(AcpCoreError::InvalidState(String::from(
                "ACP process route limit must be greater than zero",
            )));
        }
        let observed = self.owned_process_ids(owner_connection_id).len();
        if limit < observed {
            return Err(AcpCoreError::InvalidState(format!(
                "ACP process route limit {limit} is below this owner's current usage {observed}; finish or dispose sessions before lowering the limit",
            )));
        }
        self.process_route_limits
            .insert(owner_connection_id.to_string(), limit);
        self.process_route_limit_warned.remove(owner_connection_id);
        Ok(())
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn process_route_limit(&self, owner_connection_id: &str) -> usize {
        self.process_route_limits
            .get(owner_connection_id)
            .copied()
            .unwrap_or(self.default_process_route_limit)
    }

    fn owned_process_ids(&self, owner_connection_id: &str) -> BTreeSet<String> {
        let mut process_ids = BTreeSet::new();
        process_ids.extend(
            self.sessions
                .values()
                .filter(|session| {
                    session.owner_connection_id == owner_connection_id && !session.closed
                })
                .map(|session| session.process_id.clone()),
        );
        process_ids.extend(
            self.pending_creates
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_connection_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_resumes
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_connection_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_restarts
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_connection_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_prompts
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_connection_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_closes
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_connection_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_cleanups
                .keys()
                .filter(|(owner, _)| owner == owner_connection_id)
                .map(|(_, process_id)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_finalizations
                .keys()
                .filter(|(owner, _)| owner == owner_connection_id)
                .map(|(_, process_id)| process_id.clone()),
        );
        process_ids
    }

    fn ensure_process_route_capacity(
        &mut self,
        owner_connection_id: &str,
        additional: usize,
    ) -> Result<(), AcpCoreError> {
        let current = self.owned_process_ids(owner_connection_id).len();
        let limit = self.process_route_limit(owner_connection_id);
        if current.saturating_add(additional) > limit {
            return Err(AcpCoreError::LimitExceeded(format!(
                "ACP process route limit exceeded for one owner: {current} routes remain owned and the limit is {limit}; retry retained cleanup or raise AcpCore::with_process_route_limit",
            )));
        }
        if current.saturating_add(additional) >= limit.saturating_mul(3) / 4
            && self
                .process_route_limit_warned
                .insert(owner_connection_id.to_string())
        {
            tracing::warn!(
                owner_connection_id,
                current,
                additional,
                limit,
                "ACP process route usage is near its configured limit"
            );
        }
        Ok(())
    }

    /// Whether one exact owner currently owns a live session. Embedding wrappers
    /// use this only to decide whether an idempotent close needs host cleanup.
    pub fn session_is_owned_by(&self, owner_id: &str, session_id: &str) -> bool {
        self.session(owner_id, session_id).is_some()
    }

    /// Construct with an explicit transient event bound. Adapters drain events
    /// after every dispatch; this protects embedders that fail to do so.
    pub fn with_pending_event_limit(limit: usize) -> Self {
        Self {
            default_pending_event_limit: limit.max(1),
            ..Self::default()
        }
    }

    pub fn with_pending_event_limits(count: usize, bytes: usize) -> Self {
        Self {
            default_pending_event_limit: count.max(1),
            default_pending_event_bytes_limit: bytes.max(1),
            ..Self::default()
        }
    }

    /// Update the transient event bound used by the embedding adapter. Zero is
    /// never a valid bound, and queued events cannot be made unrepresentable by
    /// lowering the limit beneath the current queue depth.
    pub fn set_pending_event_limit(
        &mut self,
        owner_connection_id: &str,
        limit: usize,
    ) -> Result<(), AcpCoreError> {
        let bytes = self.event_limits(owner_connection_id).1;
        self.set_pending_event_limits(owner_connection_id, limit, bytes)
    }

    pub fn set_pending_event_limits(
        &mut self,
        owner_connection_id: &str,
        count: usize,
        bytes: usize,
    ) -> Result<(), AcpCoreError> {
        if count == 0 || bytes == 0 {
            return Err(AcpCoreError::InvalidState(String::from(
                "ACP pending event count and byte limits must be greater than zero",
            )));
        }
        let (observed_count, observed_bytes) = self.event_usage(owner_connection_id);
        if count < observed_count || bytes < observed_bytes {
            return Err(AcpCoreError::InvalidState(format!(
                "ACP pending event limits ({count} events, {bytes} bytes) are below this owner's current usage ({observed_count} events, {observed_bytes} bytes); drain AcpCore::take_events before lowering the limits",
            )));
        }
        self.pending_event_limits
            .insert(owner_connection_id.to_string(), (count, bytes));
        self.refresh_owner_event_warning(owner_connection_id);
        Ok(())
    }

    /// Drain adapter notifications and shared synthetic updates produced for one
    /// exact owner. Native/browser wrappers add their transport ownership; the
    /// behavior kernel must therefore never expose another owner's queued event.
    pub fn take_events(&mut self, owner_connection_id: &str) -> Vec<AcpEvent> {
        self.pending_event_limit_warned.remove(owner_connection_id);
        let mut events = Vec::new();
        self.pending_events.retain(|pending| {
            if pending.owner_connection_id == owner_connection_id {
                events.push(pending.event.clone());
                false
            } else {
                true
            }
        });
        self.refresh_owner_event_warning(owner_connection_id);
        events
    }

    /// Snapshot events awaiting adapter delivery without removing them. Wrappers
    /// should acknowledge each successfully delivered prefix with
    /// [`Self::acknowledge_delivered_events`]. This keeps a committed state
    /// transition observable when encoding or the host event sink fails.
    pub fn events_for_delivery(&self, owner_connection_id: &str) -> Vec<AcpEvent> {
        self.pending_events
            .iter()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .map(|pending| pending.event.clone())
            .collect()
    }

    /// Remove an exact prefix that the embedding wrapper has delivered. Events
    /// after that prefix remain queued in their original order for a later
    /// dispatch to retry.
    pub fn acknowledge_delivered_events(
        &mut self,
        owner_connection_id: &str,
        count: usize,
    ) -> Result<(), AcpCoreError> {
        let available = self
            .pending_events
            .iter()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .count();
        if count > available {
            return Err(AcpCoreError::InvalidState(format!(
                "cannot acknowledge {count} ACP events for owner when only {available} await delivery",
            )));
        }
        let mut removed = 0;
        self.pending_events.retain(|pending| {
            if removed < count && pending.owner_connection_id == owner_connection_id {
                removed += 1;
                false
            } else {
                true
            }
        });
        self.pending_event_limit_warned.remove(owner_connection_id);
        self.refresh_owner_event_warning(owner_connection_id);
        Ok(())
    }

    pub fn pending_event_count(&self) -> usize {
        self.pending_events.len()
    }

    fn event_limits(&self, owner_connection_id: &str) -> (usize, usize) {
        self.pending_event_limits
            .get(owner_connection_id)
            .copied()
            .unwrap_or((
                self.default_pending_event_limit,
                self.default_pending_event_bytes_limit,
            ))
    }

    fn queued_event_usage(&self, owner_connection_id: &str) -> (usize, usize) {
        self.pending_events
            .iter()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .fold((0usize, 0usize), |(count, bytes), pending| {
                (
                    count.saturating_add(1),
                    bytes.saturating_add(pending.encoded_bytes),
                )
            })
    }

    fn pending_interaction_event_usage(&self, owner_connection_id: &str) -> (usize, usize) {
        let creates = self
            .pending_creates
            .values()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .map(|pending| (pending.notifications.len(), pending.notification_bytes));
        let resumes = self
            .pending_resumes
            .values()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .map(|pending| (pending.notifications.len(), pending.notification_bytes));
        let prompts = self
            .pending_prompts
            .values()
            .filter(|pending| pending.owner_connection_id == owner_connection_id)
            .map(|pending| (pending.notifications.len(), pending.notification_bytes));
        creates.chain(resumes).chain(prompts).fold(
            (0usize, 0usize),
            |(count, bytes), (next_count, next_bytes)| {
                (
                    count.saturating_add(next_count),
                    bytes.saturating_add(next_bytes),
                )
            },
        )
    }

    fn event_usage(&self, owner_connection_id: &str) -> (usize, usize) {
        let queued = self.queued_event_usage(owner_connection_id);
        let pending = self.pending_interaction_event_usage(owner_connection_id);
        (
            queued.0.saturating_add(pending.0),
            queued.1.saturating_add(pending.1),
        )
    }

    fn available_event_capacity(&self, owner_connection_id: &str) -> usize {
        let (count_limit, _) = self.event_limits(owner_connection_id);
        count_limit.saturating_sub(self.event_usage(owner_connection_id).0)
    }

    fn available_event_bytes_capacity(&self, owner_connection_id: &str) -> usize {
        let (_, bytes_limit) = self.event_limits(owner_connection_id);
        bytes_limit.saturating_sub(self.event_usage(owner_connection_id).1)
    }

    fn ensure_event_capacity(
        &mut self,
        owner_connection_id: &str,
        additional_count: usize,
        additional_bytes: usize,
    ) -> Result<(), AcpCoreError> {
        let (count_limit, bytes_limit) = self.event_limits(owner_connection_id);
        let (count, bytes) = self.event_usage(owner_connection_id);
        let projected_count = count.saturating_add(additional_count);
        let projected_bytes = bytes.saturating_add(additional_bytes);
        if projected_count > count_limit || projected_bytes > bytes_limit {
            return Err(AcpCoreError::LimitExceeded(format!(
                "ACP pending event limit exceeded for one owner: at most {count_limit} events / {bytes_limit} bytes may await adapter delivery; drain AcpCore::take_events after every dispatch or raise AcpCore::with_pending_event_limits",
            )));
        }
        self.warn_if_event_usage_near_capacity(
            owner_connection_id,
            projected_count,
            projected_bytes,
        );
        Ok(())
    }

    fn ensure_pending_notification_capacity(
        &mut self,
        owner_connection_id: &str,
        current_count: usize,
        current_bytes: usize,
        additional_bytes: usize,
        phase: &str,
    ) -> Result<(), AcpCoreError> {
        let (count_limit, bytes_limit) = self.event_limits(owner_connection_id);
        let (other_count, other_bytes) = self.event_usage(owner_connection_id);
        let projected_count = other_count.saturating_add(current_count).saturating_add(1);
        let projected_bytes = other_bytes
            .saturating_add(current_bytes)
            .saturating_add(additional_bytes);
        if projected_count > count_limit || projected_bytes > bytes_limit {
            return Err(AcpCoreError::LimitExceeded(format!(
                "ACP pending event limit exceeded {phase}: at most {count_limit} events / {bytes_limit} bytes may await adapter delivery for one owner; drain AcpCore::take_events after every dispatch or raise AcpCore::with_pending_event_limits",
            )));
        }
        self.warn_if_event_usage_near_capacity(
            owner_connection_id,
            projected_count,
            projected_bytes,
        );
        Ok(())
    }

    fn warn_if_event_usage_near_capacity(
        &mut self,
        owner_connection_id: &str,
        observed_count: usize,
        observed_bytes: usize,
    ) {
        let (count_limit, bytes_limit) = self.event_limits(owner_connection_id);
        if !self
            .pending_event_limit_warned
            .contains(owner_connection_id)
            && (observed_count.saturating_mul(100) >= count_limit.saturating_mul(80)
                || observed_bytes.saturating_mul(100) >= bytes_limit.saturating_mul(80))
        {
            self.pending_event_limit_warned
                .insert(owner_connection_id.to_string());
            tracing::warn!(
                observed_count,
                count_capacity = count_limit,
                observed_bytes,
                byte_capacity = bytes_limit,
                "ACP pending event buffer is near capacity"
            );
        }
    }

    fn refresh_owner_event_warning(&mut self, owner_connection_id: &str) {
        self.pending_event_limit_warned.remove(owner_connection_id);
        let (count, bytes) = self.event_usage(owner_connection_id);
        self.warn_if_event_usage_near_capacity(owner_connection_id, count, bytes);
    }

    fn encoded_event_len(event: &AcpEvent) -> Result<usize, AcpCoreError> {
        serde_bare::to_vec(event)
            .map(|bytes| bytes.len())
            .map_err(|error| {
                AcpCoreError::InvalidState(format!(
                    "failed to size encoded ACP event before queueing it: {error}"
                ))
            })
    }

    fn json_value_len(value: &Value) -> Result<usize, AcpCoreError> {
        serde_json::to_vec(value)
            .map(|bytes| bytes.len())
            .map_err(|error| {
                AcpCoreError::InvalidState(format!(
                    "failed to size ACP notification before retaining it: {error}"
                ))
            })
    }

    fn encode_session_notification(
        notification: &AcpSessionNotification,
    ) -> Result<AcpEvent, AcpCoreError> {
        let session_id = notification.session_id.clone();
        let notification = serde_json::to_string(&notification.notification).map_err(|error| {
            AcpCoreError::InvalidState(format!(
                "failed to serialize ACP session notification: {error}"
            ))
        })?;
        Ok(AcpEvent::AcpSessionEvent(AcpSessionEvent {
            session_id,
            notification,
        }))
    }

    fn push_session_notification_batch(
        &mut self,
        owner_connection_id: &str,
        notifications: &[AcpSessionNotification],
    ) -> Result<(), AcpCoreError> {
        let events = notifications
            .iter()
            .map(Self::encode_session_notification)
            .collect::<Result<Vec<_>, _>>()?;
        let events = events
            .into_iter()
            .map(|event| {
                Self::encoded_event_len(&event).map(|encoded_bytes| (event, encoded_bytes))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bytes = events
            .iter()
            .fold(0usize, |total, (_, bytes)| total.saturating_add(*bytes));
        self.ensure_event_capacity(owner_connection_id, events.len(), bytes)?;
        self.pending_events
            .extend(
                events
                    .into_iter()
                    .map(|(event, encoded_bytes)| PendingAcpEvent {
                        owner_connection_id: owner_connection_id.to_string(),
                        event,
                        encoded_bytes,
                    }),
            );
        Ok(())
    }

    pub fn deliver_agent_stderr(
        &mut self,
        owner_connection_id: &str,
        request: &AcpDeliverAgentStderrRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let identity = self
            .sessions
            .values()
            .find(|session| {
                session.owner_connection_id == owner_connection_id
                    && session.process_id == request.process_id
                    && !session.closed
            })
            .map(|session| (session.session_id.clone(), session.agent_type.clone()))
            .or_else(|| {
                self.pending_creates
                    .get(&request.process_id)
                    .and_then(|pending| {
                        (pending.owner_connection_id == owner_connection_id)
                            .then(|| (String::new(), pending.agent_type.clone()))
                    })
            })
            .or_else(|| {
                self.pending_resumes
                    .get(&request.process_id)
                    .and_then(|pending| {
                        (pending.owner_connection_id == owner_connection_id).then(|| {
                            (
                                pending.requested_session_id.clone(),
                                pending.agent_type.clone(),
                            )
                        })
                    })
            })
            .or_else(|| {
                self.pending_restarts
                    .get(&request.process_id)
                    .and_then(|pending| {
                        (pending.owner_connection_id == owner_connection_id
                            && !pending.cleanup_only)
                            .then(|| (pending.session_id.clone(), pending.agent_type.clone()))
                    })
            })
            .or_else(|| {
                self.pending_prompts
                    .get(&request.process_id)
                    .and_then(|pending| {
                        if pending.owner_connection_id != owner_connection_id {
                            return None;
                        }
                        let session = self.session(owner_connection_id, &pending.session_id)?;
                        if pending.cleanup_restart_exit_code.is_some() || session.closed {
                            return None;
                        }
                        Some((pending.session_id.clone(), session.agent_type.clone()))
                    })
            })
            .or_else(|| {
                self.pending_closes
                    .get(&request.process_id)
                    .and_then(|pending| {
                        if pending.owner_connection_id != owner_connection_id {
                            return None;
                        }
                        let session = self.session(owner_connection_id, &pending.session_id)?;
                        if session.closed {
                            return None;
                        }
                        Some((pending.session_id.clone(), session.agent_type.clone()))
                    })
            })
            .ok_or_else(|| {
                AcpCoreError::InvalidState(format!(
                    "no owned ACP adapter route for {}",
                    request.process_id
                ))
            })?;
        let event = AcpEvent::AcpAgentStderrEvent(AcpAgentStderrEvent {
            session_id: identity.0,
            agent_type: identity.1,
            process_id: request.process_id.clone(),
            chunk: request.chunk.clone(),
        });
        let encoded_bytes = Self::encoded_event_len(&event)?;
        self.ensure_event_capacity(owner_connection_id, 1, encoded_bytes)?;
        self.pending_events.push(PendingAcpEvent {
            owner_connection_id: owner_connection_id.to_string(),
            event,
            encoded_bytes,
        });
        Ok(AcpResponse::AcpAgentStderrDeliveredResponse(
            AcpAgentStderrDeliveredResponse {
                process_id: request.process_id.clone(),
            },
        ))
    }

    fn allocate_process_id(&mut self, prefix: &str) -> String {
        let id = self.next_process_id;
        self.next_process_id += 1;
        format!("{prefix}-{id}")
    }

    /// Insert/replace a session record (used by the create/resume handlers once a
    /// process is live).
    pub fn insert_session(&mut self, record: AcpSessionRecord) {
        let key = session_storage_key(&record.owner_connection_id, &record.session_id);
        self.sessions.insert(key, record);
    }

    fn session(&self, owner_id: &str, session_id: &str) -> Option<&AcpSessionRecord> {
        self.sessions
            .get(&session_storage_key(owner_id, session_id))
    }

    fn session_mut(&mut self, owner_id: &str, session_id: &str) -> Option<&mut AcpSessionRecord> {
        self.sessions
            .get_mut(&session_storage_key(owner_id, session_id))
    }

    fn remove_session(&mut self, owner_id: &str, session_id: &str) -> Option<AcpSessionRecord> {
        self.sessions
            .remove(&session_storage_key(owner_id, session_id))
    }

    /// `session/state`: pure state lookup with per-connection ownership. A non-owner
    /// (or missing session) fails closed with the SAME error so another connection's
    /// session is not revealed across the tenant boundary.
    pub fn get_session_state(
        &self,
        caller_connection_id: &str,
        request: &AcpGetSessionStateRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let unknown =
            || AcpCoreError::InvalidState(format!("unknown ACP session {}", request.session_id));
        let session = self
            .session(caller_connection_id, &request.session_id)
            .ok_or_else(unknown)?;
        Ok(AcpResponse::AcpSessionStateResponse(
            session.state_response(),
        ))
    }

    /// List the caller's live sessions without exposing other connections.
    pub fn list_sessions(&self, caller_connection_id: &str) -> AcpResponse {
        AcpResponse::AcpListSessionsResponse(AcpListSessionsResponse {
            sessions: self
                .sessions
                .values()
                .filter(|session| {
                    session.owner_connection_id == caller_connection_id && !session.closed
                })
                .map(|session| AcpSessionEntry {
                    session_id: session.session_id.clone(),
                    agent_type: session.agent_type.clone(),
                })
                .collect(),
        })
    }

    /// `session/close`: owner-only teardown. The authoritative session remains
    /// present until all fallible cleanup succeeds, so a failed close can be
    /// retried instead of being manufactured into success by an early removal.
    pub fn close_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpCloseSessionRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        if self.finish_replacement_cleanup_before_close(
            host,
            caller_connection_id,
            &request.session_id,
        )? {
            return Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: request.session_id.clone(),
                },
            ));
        }
        let Some(session) = self
            .session(caller_connection_id, &request.session_id)
            .cloned()
        else {
            return Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: request.session_id.clone(),
                },
            ));
        };
        if let Some(response) =
            self.finish_retained_abort_before_close(host, caller_connection_id, &session)?
        {
            return Ok(response);
        }
        // Once an authorized close starts, no in-flight request may continue to
        // mutate this session even if process cleanup has to be retried.
        self.pending_prompts.remove(&session.process_id);

        if !session.closed {
            if let Err(error) = host.close_stdin(&session.process_id) {
                if !is_process_already_gone_error(&error) {
                    return Err(error);
                }
            }
        }
        // Mirror of the native `close_session` short-circuit: an adapter that
        // already exited (crash / OOM / idle eviction, recorded on the session
        // as `closed`) has no future exit to wait for, and signalling its
        // reaped PID fails with a process-gone error. Skip the SIGTERM → wait
        // → SIGKILL → wait dance (~2× `SESSION_CLOSE_TIMEOUT_MS` of dead
        // waiting) in that case.
        let adapter_already_gone = if session.closed {
            true
        } else {
            match host.kill_agent(&session.process_id, "SIGTERM") {
                Ok(()) => false,
                Err(error) if is_process_already_gone_error(&error) => true,
                Err(error) => return Err(error),
            }
        };
        if !adapter_already_gone
            && host
                .wait_for_exit(&session.process_id, SESSION_CLOSE_TIMEOUT_MS)?
                .is_none()
        {
            match host.kill_agent(&session.process_id, "SIGKILL") {
                Ok(()) => {
                    host.wait_for_exit(&session.process_id, SESSION_CLOSE_TIMEOUT_MS)?;
                }
                Err(error) if is_process_already_gone_error(&error) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(session) = self.session_mut(caller_connection_id, &request.session_id) {
            session.closed = true;
        }
        self.finish_host_finalization(host, caller_connection_id, &session)
    }

    /// Begin an orderly close without waiting inside the core. The embedding
    /// adapter reports exit/timeout through the ordinary pending continuation.
    pub fn begin_close_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpCloseSessionRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        if self.finish_replacement_cleanup_before_close(
            host,
            caller_connection_id,
            &request.session_id,
        )? {
            return Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: request.session_id.clone(),
                },
            ));
        }
        let Some(session) = self
            .session(caller_connection_id, &request.session_id)
            .cloned()
        else {
            return Ok(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: request.session_id.clone(),
                },
            ));
        };
        if let Some(response) =
            self.finish_retained_abort_before_close(host, caller_connection_id, &session)?
        {
            return Ok(response);
        }
        if self.pending_closes.contains_key(&session.process_id) {
            return self.pending_response(session.process_id);
        }
        self.pending_prompts.remove(&session.process_id);
        if !session.closed {
            if let Err(error) = host.close_stdin(&session.process_id) {
                if !is_process_already_gone_error(&error) {
                    return Err(error);
                }
            }
        }
        let adapter_already_gone = if session.closed {
            true
        } else {
            match host.kill_agent(&session.process_id, "SIGTERM") {
                Ok(()) => false,
                Err(error) if is_process_already_gone_error(&error) => true,
                Err(error) => return Err(error),
            }
        };
        if adapter_already_gone {
            return self.finish_resumable_close(host, &session);
        }
        self.pending_closes.insert(
            session.process_id.clone(),
            PendingClose {
                owner_connection_id: caller_connection_id.to_string(),
                session_id: request.session_id.clone(),
                step: PendingCloseStep::AwaitingSigterm,
            },
        );
        self.pending_response(session.process_id)
    }

    fn finish_retained_abort_before_close<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session: &AcpSessionRecord,
    ) -> Result<Option<AcpResponse>, AcpCoreError> {
        let cleanup_key = (owner_id.to_string(), session.process_id.clone());
        if self.pending_cleanups.contains_key(&cleanup_key) {
            self.drive_pending_cleanup(host, &cleanup_key)?;
            self.remove_session(owner_id, &session.session_id);
            return Ok(Some(AcpResponse::AcpSessionClosedResponse(
                AcpSessionClosedResponse {
                    session_id: session.session_id.clone(),
                },
            )));
        }
        let prompt_cleanup = self
            .pending_prompts
            .get(&session.process_id)
            .is_some_and(|pending| pending.cleanup_restart_exit_code.is_some());
        let restart_cleanup = self
            .pending_restarts
            .get(&session.process_id)
            .is_some_and(|pending| pending.cleanup_only);
        if !prompt_cleanup && !restart_cleanup {
            return Ok(None);
        }
        if let Err(error) = host.abort_agent(&session.process_id) {
            return Err(AcpCoreError::Cleanup {
                context: "failed to finish retained ACP abort before session close",
                errors: vec![error],
            });
        }
        self.pending_prompts.remove(&session.process_id);
        self.pending_restarts.remove(&session.process_id);
        self.remove_session(owner_id, &session.session_id);
        self.process_route_limit_warned.remove(owner_id);
        Ok(Some(AcpResponse::AcpSessionClosedResponse(
            AcpSessionClosedResponse {
                session_id: session.session_id.clone(),
            },
        )))
    }

    /// Replacement adapters can outlive removal of the old session record while
    /// their failed abort is retained. Close is keyed by session id, so discover
    /// those exact cleanup tombstones before treating an absent session as an
    /// idempotent success.
    fn finish_replacement_cleanup_before_close<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session_id: &str,
    ) -> Result<bool, AcpCoreError> {
        let cleanup_only_processes = self
            .pending_restarts
            .iter()
            .filter(|(_, pending)| {
                pending.cleanup_only
                    && pending.owner_connection_id == owner_id
                    && pending.session_id == session_id
            })
            .map(|(process_id, _)| process_id.clone())
            .collect::<Vec<_>>();
        for process_id in cleanup_only_processes {
            self.promote_cleanup_only_restart(&process_id)
                .expect("cleanup-only replacement was selected for promotion");
        }
        let cleanup_keys = self
            .pending_restart_cleanups
            .iter()
            .filter(|((owner, _), completion)| {
                owner == owner_id && completion.pending.session_id == session_id
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if cleanup_keys.is_empty() {
            return Ok(false);
        }
        for cleanup_key in cleanup_keys {
            self.drive_pending_cleanup(host, &cleanup_key)?;
        }
        self.remove_session(owner_id, session_id);
        self.process_route_limit_warned.remove(owner_id);
        Ok(true)
    }

    fn finish_resumable_close<H: AcpHost>(
        &mut self,
        host: &mut H,
        session: &AcpSessionRecord,
    ) -> Result<AcpResponse, AcpCoreError> {
        if let Some(session) = self.session_mut(&session.owner_connection_id, &session.session_id) {
            session.closed = true;
        }
        self.finish_host_finalization(host, &session.owner_connection_id, session)
    }

    fn finish_host_finalization<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session: &AcpSessionRecord,
    ) -> Result<AcpResponse, AcpCoreError> {
        let cleanup_key = (owner_id.to_string(), session.process_id.clone());
        self.pending_finalizations
            .insert(cleanup_key.clone(), session.session_id.clone());
        host.finalize_session_cleanup(&session.session_id, &session.process_id)?;
        self.pending_finalizations.remove(&cleanup_key);
        self.pending_closes.remove(&session.process_id);
        self.remove_session(owner_id, &session.session_id);
        self.process_route_limit_warned.remove(owner_id);
        Ok(AcpResponse::AcpSessionClosedResponse(
            AcpSessionClosedResponse {
                session_id: session.session_id.clone(),
            },
        ))
    }

    /// `session/create`: launch the agent adapter, run the ACP bootstrap handshake
    /// (`initialize` then `session/new`) over the sync seam, and record the session.
    pub fn create_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpCreateSessionRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let request = ResolvedAcpCreateSessionRequest::from(request.clone());
        let resolved = resolve_agent(host, &request.agent_type)?;
        self.ensure_process_route_capacity(caller_connection_id, 1)?;
        let process_id = self.allocate_process_id("acp-agent");
        let (args, env) = prepare_agent_launch(
            host,
            &request.agent_type,
            &resolved,
            &request.args,
            &request.env,
            request.skip_os_instructions,
            request.additional_instructions.as_deref(),
        )?;
        let restart = AcpAdapterRestartState {
            runtime: request.runtime.clone(),
            entrypoint: resolved.adapter_entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: request.cwd.clone(),
            protocol_version: request.protocol_version,
            client_capabilities: request.client_capabilities.clone(),
            count: 0,
        };

        let spawned = host.spawn_agent(SpawnAgentRequest {
            process_id: process_id.clone(),
            runtime: request.runtime.clone(),
            entrypoint: Some(resolved.adapter_entrypoint.clone()),
            command: None,
            args,
            env,
            cwd: Some(request.cwd.clone()),
        })?;

        let bootstrap =
            match self.bootstrap_session(host, caller_connection_id, &request, &process_id) {
                Ok(bootstrap) => bootstrap,
                Err(error) => {
                    return Err(self.with_abort_cleanup(
                        host,
                        caller_connection_id,
                        &process_id,
                        "blocking create bootstrap failure",
                        error,
                    ));
                }
            };

        let notifications = bootstrap.notifications.clone();
        let session = AcpSessionRecord {
            session_id: bootstrap.session_id.clone(),
            owner_connection_id: caller_connection_id.to_string(),
            agent_type: request.agent_type.clone(),
            process_id: process_id.clone(),
            pid: spawned.pid,
            modes: bootstrap.modes,
            config_options: bootstrap.config_options,
            agent_capabilities: bootstrap.agent_capabilities,
            agent_info: bootstrap.agent_info,
            stdout_buffer: bootstrap.stdout_buffer,
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: Some(restart),
        };

        if self
            .session(caller_connection_id, &session.session_id)
            .is_some()
        {
            let error =
                AcpCoreError::InvalidState(format!("session id collision: {}", session.session_id));
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking create session collision",
                error,
            ));
        }
        if let Err(error) = host.bind_session(&session.session_id, &process_id) {
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking create bind failure",
                error,
            ));
        }
        let response = AcpResponse::AcpSessionCreatedResponse(session.created_response());
        let session_id = session.session_id.clone();
        self.insert_session(session);
        let notifications = notifications
            .into_iter()
            .map(|notification| AcpSessionNotification {
                session_id: session_id.clone(),
                notification,
            })
            .collect::<Vec<_>>();
        if let Err(error) =
            self.push_session_notification_batch(caller_connection_id, &notifications)
        {
            self.remove_session(caller_connection_id, &session_id);
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking create event commit failure",
                error,
            ));
        }
        Ok(response)
    }

    /// RESUMABLE `create_session` — start (browser path). Spawns the agent and writes
    /// the `initialize` request, then RETURNS without waiting (no `poll_output`). The
    /// caller feeds the agent's stdout back via [`feed_agent_output`]; between calls
    /// the kernel worker is free (the wasm borrow is released), so it can service the
    /// agent's own syscalls on fresh, non-nested calls. Returns the process id used
    /// as the handshake handle.
    pub fn begin_create_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpCreateSessionRequest,
    ) -> Result<String, AcpCoreError> {
        let request = ResolvedAcpCreateSessionRequest::from(request.clone());
        let resolved = resolve_agent(host, &request.agent_type)?;
        self.ensure_process_route_capacity(caller_connection_id, 1)?;
        let client_capabilities =
            parse_json_text(&request.client_capabilities, "clientCapabilities")?;
        let mcp_servers = parse_json_text(&request.mcp_servers, "mcpServers")?;
        let process_id = self.allocate_process_id("acp-agent");
        let (args, env) = prepare_agent_launch(
            host,
            &request.agent_type,
            &resolved,
            &request.args,
            &request.env,
            request.skip_os_instructions,
            request.additional_instructions.as_deref(),
        )?;
        let restart = AcpAdapterRestartState {
            runtime: request.runtime.clone(),
            entrypoint: resolved.adapter_entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: request.cwd.clone(),
            protocol_version: request.protocol_version,
            client_capabilities: request.client_capabilities.clone(),
            count: 0,
        };

        let spawned = host.spawn_agent(SpawnAgentRequest {
            process_id: process_id.clone(),
            runtime: request.runtime.clone(),
            entrypoint: Some(resolved.adapter_entrypoint.clone()),
            command: None,
            args,
            env,
            cwd: Some(request.cwd.clone()),
        })?;

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": request.protocol_version,
                "clientCapabilities": client_capabilities,
            },
        });
        if let Err(error) = write_json_line(host, &process_id, &initialize) {
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "resumable create initial write failure",
                error,
            ));
        }

        self.pending_creates.insert(
            process_id.clone(),
            PendingCreate {
                owner_connection_id: caller_connection_id.to_string(),
                agent_type: request.agent_type.clone(),
                pid: spawned.pid,
                protocol_version: request.protocol_version,
                cwd: request.cwd.clone(),
                mcp_servers,
                step: CreateStep::AwaitingInitialize,
                stdout_buffer: String::new(),
                init_result: None,
                notifications: Vec::new(),
                notification_bytes: 0,
                restart,
            },
        );
        Ok(process_id)
    }

    /// RESUMABLE `resume_session` — launch the adapter and write `initialize`,
    /// then return immediately. The remaining native-load/fallback handshake is
    /// advanced exclusively by [`feed_agent_output`].
    pub fn begin_resume_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpResumeSessionRequest,
    ) -> Result<String, AcpCoreError> {
        let request = ResolvedAcpResumeSessionRequest::from(request.clone());
        let resolved = resolve_agent(host, &request.agent_type)?;
        self.ensure_process_route_capacity(caller_connection_id, 1)?;
        let client_capabilities =
            parse_json_text(DEFAULT_ACP_CLIENT_CAPABILITIES, "clientCapabilities")?;
        let process_id = self.allocate_process_id("acp-agent");
        let (args, env) = prepare_agent_launch(
            host,
            &request.agent_type,
            &resolved,
            &[],
            &request.env,
            true,
            None,
        )?;
        let restart = AcpAdapterRestartState {
            runtime: AcpRuntimeKind::JavaScript,
            entrypoint: resolved.adapter_entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: request.cwd.clone(),
            protocol_version: DEFAULT_ACP_PROTOCOL_VERSION,
            client_capabilities: DEFAULT_ACP_CLIENT_CAPABILITIES.to_string(),
            count: 0,
        };
        let spawned = host.spawn_agent(SpawnAgentRequest {
            process_id: process_id.clone(),
            runtime: AcpRuntimeKind::JavaScript,
            entrypoint: Some(resolved.adapter_entrypoint),
            command: None,
            args,
            env,
            cwd: Some(request.cwd.clone()),
        })?;
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": DEFAULT_ACP_PROTOCOL_VERSION,
                "clientCapabilities": client_capabilities,
            },
        });
        if let Err(error) = write_json_line(host, &process_id, &initialize) {
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "resumable resume initial write failure",
                error,
            ));
        }

        self.pending_resumes.insert(
            process_id.clone(),
            PendingResume {
                owner_connection_id: caller_connection_id.to_string(),
                requested_session_id: request.session_id,
                agent_type: request.agent_type,
                pid: spawned.pid,
                cwd: request.cwd,
                transcript_path: request.transcript_path,
                step: PendingResumeStep::AwaitingInitialize,
                stdout_buffer: String::new(),
                init_result: None,
                agent_capabilities: None,
                notifications: Vec::new(),
                notification_bytes: 0,
                restart,
            },
        );
        Ok(process_id)
    }

    /// RESUMABLE — feed agent stdout into whatever interaction is in flight for this
    /// process (a `create_session` handshake or a `session/prompt`), advancing it
    /// without ever blocking. Returns [`ResumeStep::Done`] with the response when the
    /// interaction completes, else [`ResumeStep::Pending`]. The kernel worker calls
    /// this across separate `pushFrame`s and services the agent's syscalls in between
    /// (legal — not nested).
    pub fn feed_agent_output<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        process_id: &str,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let owner = self
            .pending_creates
            .get(process_id)
            .map(|pending| pending.owner_connection_id.as_str())
            .or_else(|| {
                self.pending_resumes
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_prompts
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_restarts
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_closes
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            });
        if owner != Some(caller_connection_id) {
            return Err(AcpCoreError::InvalidState(format!(
                "no pending ACP interaction for {process_id}"
            )));
        }
        if self.pending_creates.contains_key(process_id) {
            self.feed_create(host, process_id, chunk)
        } else if self.pending_resumes.contains_key(process_id) {
            self.feed_resume(host, process_id, chunk)
        } else if self.pending_prompts.contains_key(process_id) {
            self.feed_prompt(host, process_id, chunk)
        } else if self.pending_restarts.contains_key(process_id) {
            self.feed_restart(host, process_id, chunk)
        } else if self.pending_closes.contains_key(process_id) {
            Ok(ResumeStep::Pending)
        } else {
            Err(AcpCoreError::InvalidState(format!(
                "no pending ACP interaction for {process_id}"
            )))
        }
    }

    fn feed_create<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let pending = self.pending_creates.remove(process_id).ok_or_else(|| {
            AcpCoreError::InvalidState(format!("no pending create_session for {process_id}"))
        })?;
        let owner_connection_id = pending.owner_connection_id.clone();
        let result = self.advance_create(host, process_id, pending, chunk);
        match result {
            Ok(step) => Ok(step),
            Err(error) => Err(self.with_abort_cleanup(
                host,
                &owner_connection_id,
                process_id,
                "resumable create failure",
                error,
            )),
        }
    }

    fn advance_create<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        mut pending: PendingCreate,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let mut session_result: Option<Map<String, Value>> = None;
        let mut lines =
            AcpJsonLineAccumulator::with_buffer(std::mem::take(&mut pending.stdout_buffer));
        let messages = lines.push_json(chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES)?;
        pending.stdout_buffer = lines.into_retained();

        for message in messages {
            match classify_json_rpc_message(&message) {
                AcpJsonRpcMessageKind::InboundRequest => {
                    answer_inbound_request(host, process_id, &message)?;
                    continue;
                }
                AcpJsonRpcMessageKind::Notification => {
                    let message_bytes = Self::json_value_len(&message)?;
                    self.ensure_pending_notification_capacity(
                        &pending.owner_connection_id,
                        pending.notifications.len(),
                        pending.notification_bytes,
                        message_bytes,
                        "while creating a session",
                    )?;
                    pending.notification_bytes =
                        pending.notification_bytes.saturating_add(message_bytes);
                    pending.notifications.push(message);
                    continue;
                }
                AcpJsonRpcMessageKind::Response | AcpJsonRpcMessageKind::Unknown => {}
            }
            match pending.step {
                CreateStep::AwaitingInitialize => {
                    if message.get("id").and_then(Value::as_i64) != Some(1) {
                        continue;
                    }
                    let init = response_result(message, "ACP initialize")?;
                    validate_initialize_result(&init, pending.protocol_version)?;
                    pending.init_result = Some(init);
                    let session_new = json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "session/new",
                        "params": { "cwd": pending.cwd.clone(), "mcpServers": pending.mcp_servers.clone() },
                    });
                    write_json_line(host, process_id, &session_new)?;
                    pending.step = CreateStep::AwaitingSessionNew;
                }
                CreateStep::AwaitingSessionNew => {
                    if message.get("id").and_then(Value::as_i64) != Some(2) {
                        continue;
                    }
                    session_result = Some(response_result(message, "ACP session/new")?);
                    break;
                }
            }
        }

        let Some(session_result) = session_result else {
            self.pending_creates.insert(process_id.to_string(), pending);
            return Ok(ResumeStep::Pending);
        };

        let init_result = pending.init_result.clone().unwrap_or_default();
        let session_id = session_id_from_session_result(&session_result, process_id);
        if self
            .session(&pending.owner_connection_id, &session_id)
            .is_some()
        {
            return Err(AcpCoreError::InvalidState(format!(
                "session id collision: {session_id}"
            )));
        }
        let fields = derive_bootstrap_fields(
            &pending.agent_type,
            &init_result,
            &session_result,
            init_result.get("agentCapabilities"),
        )?;
        let notifications = pending.notifications;
        let record = AcpSessionRecord {
            session_id: session_id.clone(),
            owner_connection_id: pending.owner_connection_id.clone(),
            agent_type: pending.agent_type.clone(),
            process_id: process_id.to_string(),
            pid: pending.pid,
            modes: fields.modes,
            config_options: fields.config_options,
            agent_capabilities: fields.agent_capabilities,
            agent_info: fields.agent_info,
            stdout_buffer: pending.stdout_buffer,
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: Some(pending.restart),
        };
        host.bind_session(&session_id, process_id)?;
        let response = AcpResponse::AcpSessionCreatedResponse(record.created_response());
        self.insert_session(record);
        let notifications = notifications
            .into_iter()
            .map(|notification| AcpSessionNotification {
                session_id: session_id.clone(),
                notification,
            })
            .collect::<Vec<_>>();
        if let Err(error) =
            self.push_session_notification_batch(&pending.owner_connection_id, &notifications)
        {
            self.remove_session(&pending.owner_connection_id, &session_id);
            return Err(error);
        }
        Ok(ResumeStep::Done(response))
    }

    fn feed_resume<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        // Remove first so every terminal branch, including malformed output and
        // host-write failures, clears the pending state. Only a genuinely pending
        // transition is reinserted below.
        let pending = self.pending_resumes.remove(process_id).ok_or_else(|| {
            AcpCoreError::InvalidState(format!("no pending resume_session for {process_id}"))
        })?;
        let owner_connection_id = pending.owner_connection_id.clone();
        let result = self.advance_resume(host, process_id, pending, chunk);
        match result {
            Ok(step) => Ok(step),
            Err(error) => Err(self.with_abort_cleanup(
                host,
                &owner_connection_id,
                process_id,
                "resumable resume failure",
                error,
            )),
        }
    }

    fn advance_resume<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        mut pending: PendingResume,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let mut lines =
            AcpJsonLineAccumulator::with_buffer(std::mem::take(&mut pending.stdout_buffer));
        let messages = lines.push_json(chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES)?;
        pending.stdout_buffer = lines.into_retained();
        let mut completed = None;

        for mut message in messages {
            match classify_json_rpc_message(&message) {
                AcpJsonRpcMessageKind::InboundRequest => {
                    answer_inbound_request(host, process_id, &message)?;
                    continue;
                }
                AcpJsonRpcMessageKind::Notification => {
                    let message_bytes = Self::json_value_len(&message)?;
                    self.ensure_pending_notification_capacity(
                        &pending.owner_connection_id,
                        pending.notifications.len(),
                        pending.notification_bytes,
                        message_bytes,
                        "while resuming a session",
                    )?;
                    pending.notification_bytes =
                        pending.notification_bytes.saturating_add(message_bytes);
                    pending.notifications.push(message);
                    continue;
                }
                AcpJsonRpcMessageKind::Response | AcpJsonRpcMessageKind::Unknown => {}
            }

            match pending.step {
                PendingResumeStep::AwaitingInitialize => {
                    if message.get("id").and_then(Value::as_i64) != Some(1) {
                        continue;
                    }
                    let init_result = response_result(message, "ACP initialize")?;
                    validate_initialize_result(&init_result, DEFAULT_ACP_PROTOCOL_VERSION)?;
                    let agent_capabilities = init_result.get("agentCapabilities").cloned();
                    pending.init_result = Some(init_result);
                    pending.agent_capabilities = agent_capabilities.clone();

                    if let Some(method) = native_resume_method(agent_capabilities.as_ref()) {
                        let load = json!({
                            "jsonrpc": "2.0",
                            "id": 2,
                            "method": method,
                            "params": {
                                "sessionId": pending.requested_session_id,
                                "cwd": pending.cwd,
                                "mcpServers": [],
                            },
                        });
                        write_json_line(host, process_id, &load)?;
                        pending.step = PendingResumeStep::AwaitingNative(method);
                    } else {
                        write_resume_session_new(host, process_id, &pending.cwd)?;
                        pending.step = PendingResumeStep::AwaitingFallbackSessionNew;
                    }
                }
                PendingResumeStep::AwaitingNative(method) => {
                    if message.get("id").and_then(Value::as_i64) != Some(2) {
                        continue;
                    }
                    normalize_unknown_session_error(&mut message);
                    if message.get("error").is_none() {
                        completed = Some(CompletedResume {
                            session_id: pending.requested_session_id.clone(),
                            mode: String::from("native"),
                            pending_preamble: None,
                            session_result: response_result(message, &format!("ACP {method}"))?,
                        });
                        break;
                    }
                    if !is_unknown_session_error(&message) {
                        return Err(response_result(message, &format!("ACP {method}"))
                            .expect_err("native resume error must map to AcpCoreError"));
                    }
                    write_resume_session_new(host, process_id, &pending.cwd)?;
                    pending.step = PendingResumeStep::AwaitingFallbackSessionNew;
                }
                PendingResumeStep::AwaitingFallbackSessionNew => {
                    if message.get("id").and_then(Value::as_i64) != Some(2) {
                        continue;
                    }
                    let session_result = response_result(message, "ACP session/new")?;
                    completed = Some(CompletedResume {
                        session_id: session_id_from_session_result(&session_result, process_id),
                        mode: String::from("fallback"),
                        pending_preamble: pending
                            .transcript_path
                            .as_deref()
                            .filter(|path| !path.is_empty())
                            .map(|path| CONTINUATION_PREAMBLE.replace("{path}", path)),
                        session_result,
                    });
                    break;
                }
            }
        }

        let Some(completed) = completed else {
            self.pending_resumes.insert(process_id.to_string(), pending);
            return Ok(ResumeStep::Pending);
        };

        let init_result = pending.init_result.as_ref().ok_or_else(|| {
            AcpCoreError::InvalidState(String::from(
                "ACP resume completed without an initialize result",
            ))
        })?;
        let fields = derive_bootstrap_fields(
            &pending.agent_type,
            init_result,
            &completed.session_result,
            pending.agent_capabilities.as_ref(),
        )?;
        if self
            .session(&pending.owner_connection_id, &completed.session_id)
            .is_some()
        {
            return Err(AcpCoreError::InvalidState(format!(
                "session id collision: {}",
                completed.session_id
            )));
        }
        let session_id = completed.session_id;
        let record = AcpSessionRecord {
            session_id: session_id.clone(),
            owner_connection_id: pending.owner_connection_id,
            agent_type: pending.agent_type,
            process_id: process_id.to_string(),
            pid: pending.pid,
            modes: fields.modes,
            config_options: fields.config_options,
            agent_capabilities: fields.agent_capabilities,
            agent_info: fields.agent_info,
            stdout_buffer: pending.stdout_buffer,
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: completed.pending_preamble,
            restart: Some(pending.restart),
        };
        host.bind_session(&session_id, process_id)?;
        let response = AcpResponse::AcpSessionResumedResponse(AcpSessionResumedResponse {
            session_id: session_id.clone(),
            mode: completed.mode,
            agent_type: record.agent_type.clone(),
            process_id: process_id.to_string(),
            pid: record.pid,
        });
        let owner_connection_id = record.owner_connection_id.clone();
        self.insert_session(record);
        let notifications = pending
            .notifications
            .into_iter()
            .map(|notification| AcpSessionNotification {
                session_id: session_id.clone(),
                notification,
            })
            .collect::<Vec<_>>();
        if let Err(error) =
            self.push_session_notification_batch(&owner_connection_id, &notifications)
        {
            self.remove_session(&owner_connection_id, &session_id);
            return Err(error);
        }
        Ok(ResumeStep::Done(response))
    }

    /// RESUMABLE `session/prompt` (and other in-session RPC) — start. Owner-only;
    /// allocates the rpc id, injects `sessionId`, consumes any armed preamble, writes
    /// the request, and RETURNS without waiting. The agent's reply (and its mid-turn
    /// syscalls — pi's inference is a `net` call here) are handled via
    /// `feed_agent_output` across separate, non-nested `pushFrame`s.
    pub fn begin_session_request<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpSessionRequest,
    ) -> Result<String, AcpCoreError> {
        let mut outbound_params = match request.params.as_deref() {
            Some(params) => to_record(parse_json_text(params, "session request params")?),
            None => Map::new(),
        };
        outbound_params.insert(
            String::from("sessionId"),
            Value::String(request.session_id.clone()),
        );

        let unknown =
            || AcpCoreError::InvalidState(format!("unknown ACP session {}", request.session_id));
        let session = self
            .session(caller_connection_id, &request.session_id)
            .ok_or_else(unknown)?;
        if session.closed {
            return Err(unknown());
        }
        let process_id = session.process_id.clone();
        if self.pending_prompts.contains_key(&process_id) {
            return Err(AcpCoreError::Conflict(format!(
                "ACP session {} already has an in-flight request; wait for it to complete before sending another",
                request.session_id
            )));
        }
        let rpc_id = session.next_request_id;
        let pending_preamble = if request.method == "session/prompt" {
            session.pending_preamble.clone()
        } else {
            None
        };
        if let Some(preamble) = pending_preamble.as_deref() {
            prepend_prompt_preamble(&mut outbound_params, preamble);
        }

        let outbound = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": request.method,
            "params": Value::Object(outbound_params.clone()),
        });
        write_json_line(host, &process_id, &outbound)?;

        let session = self
            .session_mut(caller_connection_id, &request.session_id)
            .ok_or_else(unknown)?;
        session.next_request_id = rpc_id.saturating_add(1);
        if pending_preamble.is_some() {
            session.pending_preamble = None;
        }

        self.pending_prompts.insert(
            process_id.clone(),
            PendingPrompt {
                owner_connection_id: caller_connection_id.to_string(),
                session_id: request.session_id.clone(),
                method: request.method.clone(),
                params: outbound_params,
                rpc_id,
                stdout_buffer: String::new(),
                prompt_text: (request.method == "session/prompt")
                    .then(AcpPromptTextAccumulator::default),
                notifications: Vec::new(),
                notification_bytes: 0,
                pending_preamble,
                cancelled: false,
                cleanup_restart_exit_code: None,
            },
        );
        Ok(process_id)
    }

    fn feed_prompt<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        if self
            .pending_prompts
            .get(process_id)
            .is_some_and(|pending| pending.cleanup_restart_exit_code.is_some())
        {
            return Err(AcpCoreError::InvalidState(format!(
                "ACP process {process_id} is retained for cleanup and is no longer routable"
            )));
        }
        let pending = self.pending_prompts.remove(process_id).ok_or_else(|| {
            AcpCoreError::InvalidState(format!("no pending session request for {process_id}"))
        })?;
        let session_id = pending.session_id.clone();
        let owner_connection_id = pending.owner_connection_id.clone();
        let pending_preamble = pending.pending_preamble.clone();
        match self.advance_prompt(host, process_id, pending, chunk) {
            Ok(step) => Ok(step),
            Err(error) => {
                if let Some(session) = self.session_mut(&owner_connection_id, &session_id) {
                    session.closed = true;
                    if session.pending_preamble.is_none() {
                        session.pending_preamble = pending_preamble;
                    }
                }
                Err(self.with_abort_cleanup(
                    host,
                    &owner_connection_id,
                    process_id,
                    "resumable session request failure",
                    error,
                ))
            }
        }
    }

    fn feed_restart<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        if self
            .pending_restarts
            .get(process_id)
            .is_some_and(|pending| pending.cleanup_only)
        {
            return Err(AcpCoreError::InvalidState(format!(
                "ACP process {process_id} is retained for cleanup and is no longer routable"
            )));
        }
        let pending = self.pending_restarts.remove(process_id).ok_or_else(|| {
            AcpCoreError::InvalidState(format!("no pending adapter restart for {process_id}"))
        })?;
        let session_id = pending.session_id.clone();
        let owner_connection_id = pending.owner_connection_id.clone();
        let failure = pending.clone();
        match self.advance_restart(host, process_id, pending, chunk) {
            Ok(step) => Ok(step),
            Err(error) => {
                let detail = error.to_string();
                let error = self.with_abort_cleanup(
                    host,
                    &owner_connection_id,
                    process_id,
                    "resumable adapter restart failure",
                    error,
                );
                self.remove_session(&owner_connection_id, &session_id);
                if matches!(error, AcpCoreError::Cleanup { .. }) {
                    let cleanup_key = (owner_connection_id.clone(), process_id.to_string());
                    if !self.pending_restart_cleanups.contains_key(&cleanup_key) {
                        self.retain_restart_cleanup_completion(
                            process_id, failure, "failed", detail, true,
                        );
                    }
                    return Err(error);
                }
                self.finish_pending_restart_failure(&failure, &error.to_string())
                    .map(ResumeStep::Done)
            }
        }
    }

    fn finish_pending_restart_failure(
        &mut self,
        pending: &PendingRestart,
        detail: &str,
    ) -> Result<AcpResponse, AcpCoreError> {
        let exit_error = AcpCoreError::InvalidState(format!(
            "agent exited before completing the ACP interaction ({})",
            pending.dead_process_id
        ));
        let terminal = self.finish_adapter_exit(
            &pending.owner_connection_id,
            &pending.session_id,
            &pending.agent_type,
            &pending.dead_process_id,
            pending.exit_code,
            &pending.restart,
            "failed",
            Some(detail),
            Some(&exit_error),
        )?;
        Ok(crate::error_response(&terminal))
    }

    fn advance_restart<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        mut pending: PendingRestart,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let mut lines =
            AcpJsonLineAccumulator::with_buffer(std::mem::take(&mut pending.stdout_buffer));
        let messages = lines.push_json(chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES)?;
        pending.stdout_buffer = lines.into_retained();

        for message in messages {
            match classify_json_rpc_message(&message) {
                AcpJsonRpcMessageKind::InboundRequest => {
                    answer_inbound_request(host, process_id, &message)?;
                    continue;
                }
                AcpJsonRpcMessageKind::Notification => {
                    // Restart notifications are intentionally not committed before
                    // the replacement has successfully rebound the session.
                    continue;
                }
                AcpJsonRpcMessageKind::Response | AcpJsonRpcMessageKind::Unknown => {}
            }

            match pending.step {
                PendingRestartStep::AwaitingInitialize => {
                    if message.get("id").and_then(Value::as_i64) != Some(1) {
                        continue;
                    }
                    let init_result = response_result(message, "ACP initialize")?;
                    validate_initialize_result(&init_result, pending.restart.protocol_version)?;
                    let agent_capabilities = init_result.get("agentCapabilities").cloned();
                    let Some(method) = native_resume_method(agent_capabilities.as_ref()) else {
                        let cleanup = self.with_abort_cleanup(
                            host,
                            &pending.owner_connection_id,
                            process_id,
                            "resumable adapter restart unsupported",
                            AcpCoreError::Unsupported(String::from(
                                "adapter does not advertise loadSession/resume",
                            )),
                        );
                        self.remove_session(&pending.owner_connection_id, &pending.session_id);
                        if matches!(cleanup, AcpCoreError::Cleanup { .. }) {
                            self.retain_restart_cleanup_completion(
                                process_id,
                                pending.clone(),
                                "unsupported",
                                String::from("adapter does not advertise loadSession/resume"),
                                false,
                            );
                            return Err(cleanup);
                        }
                        let error = self.finish_adapter_exit(
                            &pending.owner_connection_id,
                            &pending.session_id,
                            &pending.agent_type,
                            &pending.dead_process_id,
                            pending.exit_code,
                            &pending.restart,
                            "unsupported",
                            Some("adapter does not advertise loadSession/resume"),
                            None,
                        )?;
                        return Ok(ResumeStep::Done(crate::error_response(&error)));
                    };
                    let load = json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": method,
                        "params": {
                            "sessionId": pending.session_id,
                            "cwd": pending.restart.cwd,
                            "mcpServers": [],
                        },
                    });
                    write_json_line(host, process_id, &load)?;
                    pending.init_result = Some(init_result);
                    pending.agent_capabilities = agent_capabilities;
                    pending.step = PendingRestartStep::AwaitingNative(method);
                }
                PendingRestartStep::AwaitingNative(method) => {
                    if message.get("id").and_then(Value::as_i64) != Some(2) {
                        continue;
                    }
                    let load_result = response_result(message, &format!("ACP {method}"))?;
                    let init_result = pending.init_result.as_ref().ok_or_else(|| {
                        AcpCoreError::InvalidState(String::from(
                            "ACP restart completed without an initialize result",
                        ))
                    })?;
                    let fields = derive_bootstrap_fields(
                        &pending.agent_type,
                        init_result,
                        &load_result,
                        pending.agent_capabilities.as_ref(),
                    )?;
                    host.bind_session(&pending.session_id, process_id)?;
                    let session = self
                        .session_mut(&pending.owner_connection_id, &pending.session_id)
                        .ok_or_else(|| {
                            AcpCoreError::InvalidState(format!(
                                "ACP session {} was removed during adapter restart",
                                pending.session_id
                            ))
                        })?;
                    session.process_id = process_id.to_string();
                    session.pid = pending.pid;
                    session.modes = fields.modes;
                    session.config_options = fields.config_options;
                    session.agent_capabilities = fields.agent_capabilities;
                    session.agent_info = fields.agent_info;
                    session.stdout_buffer = pending.stdout_buffer;
                    session.next_request_id = 3;
                    session.closed = false;
                    session.exit_code = None;
                    session.restart = Some(pending.restart.clone());
                    let error = self.finish_adapter_exit(
                        &pending.owner_connection_id,
                        &pending.session_id,
                        &pending.agent_type,
                        &pending.dead_process_id,
                        pending.exit_code,
                        &pending.restart,
                        "restarted",
                        None,
                        None,
                    )?;
                    return Ok(ResumeStep::Done(crate::error_response(&error)));
                }
            }
        }

        self.pending_restarts
            .insert(process_id.to_string(), pending);
        Ok(ResumeStep::Pending)
    }

    fn advance_prompt<H: AcpHost>(
        &mut self,
        host: &mut H,
        process_id: &str,
        mut pending: PendingPrompt,
        chunk: &[u8],
    ) -> Result<ResumeStep, AcpCoreError> {
        let mut completed = None;
        let mut lines =
            AcpJsonLineAccumulator::with_buffer(std::mem::take(&mut pending.stdout_buffer));
        let messages = lines.push_json(chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES)?;
        pending.stdout_buffer = lines.into_retained();
        for message in messages {
            match classify_json_rpc_message(&message) {
                AcpJsonRpcMessageKind::InboundRequest => {
                    answer_inbound_request(host, process_id, &message)?;
                    continue;
                }
                AcpJsonRpcMessageKind::Notification => {
                    if pending.cancelled {
                        continue;
                    }
                    let message_bytes = Self::json_value_len(&message)?;
                    self.ensure_pending_notification_capacity(
                        &pending.owner_connection_id,
                        pending.notifications.len(),
                        pending.notification_bytes,
                        message_bytes,
                        "during a session request",
                    )?;
                    pending.notification_bytes =
                        pending.notification_bytes.saturating_add(message_bytes);
                    if let Some(capture) = pending.prompt_text.as_mut() {
                        if capture
                            .push_notification(&message)
                            .map_err(AcpCoreError::LimitExceeded)?
                        {
                            tracing::warn!(
                                session_id = pending.session_id,
                                "ACP prompt text capture is near its configured limit"
                            );
                        }
                    }
                    pending.notifications.push(AcpSessionNotification {
                        session_id: pending.session_id.clone(),
                        notification: message,
                    });
                    continue;
                }
                AcpJsonRpcMessageKind::Response | AcpJsonRpcMessageKind::Unknown => {}
            }
            if message.get("id").and_then(Value::as_i64) != Some(pending.rpc_id) {
                continue;
            }
            completed = Some((
                message,
                pending
                    .prompt_text
                    .take()
                    .map(AcpPromptTextAccumulator::into_text),
            ));
            break;
        }

        let Some((mut response, text)) = completed else {
            self.pending_prompts.insert(process_id.to_string(), pending);
            return Ok(ResumeStep::Pending);
        };
        if pending.cancelled {
            return Ok(ResumeStep::Done(AcpResponse::AcpErrorResponse(
                AcpErrorResponse {
                    code: String::from("agent_interaction_cancelled"),
                    message: format!("agent interaction was cancelled and drained ({process_id})"),
                },
            )));
        }
        let session_id = pending.session_id;
        let method = pending.method;
        let params = pending.params;
        let mut notifications = pending.notifications;
        if cancel_fallback_decision(&method, &response)
            == AcpCancelFallbackDecision::SendNotification
        {
            write_json_line(host, process_id, &cancel_notification(&session_id))?;
            let id = response.get("id").cloned().unwrap_or(Value::Null);
            response = cancel_notification_fallback_response(id);
        }
        let mut updated_session = if response.get("error").is_none() {
            let mut session = self
                .session(&pending.owner_connection_id, &session_id)
                .cloned()
                .ok_or_else(|| {
                    AcpCoreError::InvalidState(format!("unknown ACP session {session_id}"))
                })?;
            if let Some(synthetic) =
                apply_successful_session_request(&mut session, &method, &params, &notifications)?
            {
                notifications.push(AcpSessionNotification {
                    session_id: session_id.clone(),
                    notification: synthetic.notification(),
                });
            }
            Some(session)
        } else {
            None
        };
        let response = serde_json::to_string(&response).map_err(|error| {
            AcpCoreError::InvalidState(format!("failed to serialize ACP session response: {error}"))
        })?;
        self.push_session_notification_batch(&pending.owner_connection_id, &notifications)?;
        if let Some(session) = updated_session.take() {
            self.insert_session(session);
        }
        Ok(ResumeStep::Done(AcpResponse::AcpSessionRpcResponse(
            AcpSessionRpcResponse {
                session_id,
                response,
                text,
            },
        )))
    }

    /// In-flight resumable interactions, for diagnostics/tests.
    pub fn pending_create_count(&self) -> usize {
        self.pending_creates.len()
    }
    pub fn pending_prompt_count(&self) -> usize {
        self.pending_prompts.len()
    }
    pub fn pending_resume_count(&self) -> usize {
        self.pending_resumes.len()
    }
    pub fn pending_restart_count(&self) -> usize {
        self.pending_restarts.len()
    }
    pub fn pending_close_count(&self) -> usize {
        self.pending_closes.len()
    }
    pub fn pending_cleanup_count(&self) -> usize {
        self.pending_cleanups.len()
            + self.pending_finalizations.len()
            + self
                .pending_prompts
                .values()
                .filter(|pending| pending.cleanup_restart_exit_code.is_some())
                .count()
            + self
                .pending_restarts
                .values()
                .filter(|pending| pending.cleanup_only)
                .count()
    }
    pub fn pending_interaction_count(&self) -> usize {
        self.pending_creates.len()
            + self
                .pending_prompts
                .values()
                .filter(|pending| pending.cleanup_restart_exit_code.is_none())
                .count()
            + self.pending_resumes.len()
            + self
                .pending_restarts
                .values()
                .filter(|pending| !pending.cleanup_only)
                .count()
            + self.pending_closes.len()
    }

    /// Mark an in-flight prompt as cancelled while retaining its exact response
    /// boundary. The native transport must keep feeding adapter output until the
    /// old RPC response arrives; cancelled notifications and prompt text are then
    /// discarded before the session can accept a later prompt.
    pub fn interrupt_pending_prompt(
        &mut self,
        owner_id: &str,
        process_id: &str,
    ) -> Result<String, AcpCoreError> {
        let (session_id, pending_preamble) = {
            let pending = self.pending_prompts.get_mut(process_id).ok_or_else(|| {
                AcpCoreError::InvalidState(format!(
                    "no pending ACP prompt interaction for {process_id}"
                ))
            })?;
            if pending.owner_connection_id != owner_id {
                return Err(AcpCoreError::InvalidState(format!(
                    "no pending ACP prompt interaction for {process_id}"
                )));
            }
            if pending.cleanup_restart_exit_code.is_some() {
                return Err(AcpCoreError::InvalidState(format!(
                    "ACP prompt {process_id} is retained for cleanup and cannot be interrupted"
                )));
            }
            pending.cancelled = true;
            pending.notifications.clear();
            pending.notification_bytes = 0;
            pending.prompt_text = None;
            (pending.session_id.clone(), pending.pending_preamble.take())
        };
        if let Some(session) = self.session_mut(owner_id, &session_id) {
            if session.pending_preamble.is_none() {
                session.pending_preamble = pending_preamble;
            }
        }
        Ok(session_id)
    }

    /// Abandon a prompt whose interrupting operation will immediately close the
    /// session or terminate the adapter. Unlike explicit `session/cancel`, those
    /// operations provide their own terminal boundary, so no adapter response
    /// drain is required before removing the continuation.
    pub fn abandon_pending_prompt(
        &mut self,
        owner_id: &str,
        process_id: &str,
    ) -> Result<String, AcpCoreError> {
        let pending = self.pending_prompts.get(process_id).ok_or_else(|| {
            AcpCoreError::InvalidState(format!(
                "no pending ACP prompt interaction for {process_id}"
            ))
        })?;
        if pending.owner_connection_id != owner_id {
            return Err(AcpCoreError::InvalidState(format!(
                "no pending ACP prompt interaction for {process_id}"
            )));
        }
        if pending.cleanup_restart_exit_code.is_some() {
            return Err(AcpCoreError::InvalidState(format!(
                "ACP prompt {process_id} is retained for cleanup and cannot be abandoned"
            )));
        }
        let pending = self
            .pending_prompts
            .remove(process_id)
            .expect("pending prompt was checked above");
        if let Some(session) = self.session_mut(owner_id, &pending.session_id) {
            if session.pending_preamble.is_none() {
                session.pending_preamble = pending.pending_preamble;
            }
        }
        Ok(pending.session_id)
    }

    fn pending_response(&self, process_id: String) -> Result<AcpResponse, AcpCoreError> {
        let (timeout_ms, timeout_phase) =
            if let Some(pending) = self.pending_creates.get(&process_id) {
                match pending.step {
                    CreateStep::AwaitingInitialize => {
                        (INITIALIZE_TIMEOUT_MS, "create.initialize".to_string())
                    }
                    CreateStep::AwaitingSessionNew => {
                        (SESSION_NEW_TIMEOUT_MS, "create.session_new".to_string())
                    }
                }
            } else if let Some(pending) = self.pending_resumes.get(&process_id) {
                match pending.step {
                    PendingResumeStep::AwaitingInitialize => {
                        (INITIALIZE_TIMEOUT_MS, "resume.initialize".to_string())
                    }
                    PendingResumeStep::AwaitingNative(method) => {
                        (request_timeout_ms(method), format!("resume.{method}"))
                    }
                    PendingResumeStep::AwaitingFallbackSessionNew => {
                        (SESSION_NEW_TIMEOUT_MS, "resume.session_new".to_string())
                    }
                }
            } else if let Some(pending) = self.pending_prompts.get(&process_id) {
                (
                    request_timeout_ms(&pending.method),
                    format!("session.{}", pending.method),
                )
            } else if let Some(pending) = self.pending_restarts.get(&process_id) {
                match pending.step {
                    PendingRestartStep::AwaitingInitialize => {
                        (INITIALIZE_TIMEOUT_MS, "restart.initialize".to_string())
                    }
                    PendingRestartStep::AwaitingNative(method) => {
                        (request_timeout_ms(method), format!("restart.{method}"))
                    }
                }
            } else if let Some(pending) = self.pending_closes.get(&process_id) {
                match pending.step {
                    PendingCloseStep::AwaitingSigterm => {
                        (SESSION_CLOSE_TIMEOUT_MS, "close.sigterm".to_string())
                    }
                    PendingCloseStep::AwaitingSigkill => {
                        (SESSION_CLOSE_TIMEOUT_MS, "close.sigkill".to_string())
                    }
                }
            } else {
                return Err(AcpCoreError::InvalidState(format!(
                    "no pending ACP interaction for {process_id}"
                )));
            };
        let timeout_ms = u32::try_from(timeout_ms)
            .expect("sidecar-owned ACP request timeouts fit in protocol u32 milliseconds");
        Ok(AcpResponse::AcpPendingResponse(AcpPendingResponse {
            process_id,
            timeout_ms,
            timeout_phase,
        }))
    }

    /// Abort one exact owner-scoped resumable interaction and return the stable
    /// terminal response that replaces its original `AcpPendingResponse`.
    pub fn abort_pending<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        request: &AcpAbortPendingRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let cleanup_key = (owner_id.to_string(), request.process_id.clone());
        if self.pending_cleanups.contains_key(&cleanup_key) {
            return self.drive_pending_cleanup(host, &cleanup_key);
        }
        if let Some(pending) = self.pending_closes.get(&request.process_id).cloned() {
            if pending.owner_connection_id != owner_id {
                return Err(AcpCoreError::InvalidState(format!(
                    "no pending ACP interaction for {}",
                    request.process_id
                )));
            }
            let session = self
                .session(owner_id, &pending.session_id)
                .cloned()
                .ok_or_else(|| {
                    AcpCoreError::InvalidState(format!(
                        "no ACP session {} for pending close",
                        pending.session_id
                    ))
                })?;
            match request.reason {
                AcpPendingAbortReason::AgentExited => {
                    return self.finish_resumable_close(host, &session);
                }
                AcpPendingAbortReason::InteractionTimeout
                    if pending.step == PendingCloseStep::AwaitingSigterm =>
                {
                    match host.kill_agent(&request.process_id, "SIGKILL") {
                        Ok(()) => {
                            self.pending_closes
                                .get_mut(&request.process_id)
                                .expect("pending close was checked")
                                .step = PendingCloseStep::AwaitingSigkill;
                            return self.pending_response(request.process_id.clone());
                        }
                        Err(error) if is_process_already_gone_error(&error) => {
                            return self.finish_resumable_close(host, &session);
                        }
                        Err(error) => return Err(error),
                    }
                }
                AcpPendingAbortReason::InteractionTimeout => {
                    return self.finish_resumable_close(host, &session);
                }
                AcpPendingAbortReason::DriverFailed => {
                    self.pending_closes.remove(&request.process_id);
                    if let Some(session) = self.session_mut(owner_id, &pending.session_id) {
                        session.closed = true;
                    }
                    let response = crate::error_response(&AcpCoreError::Execution(format!(
                        "ACP pending driver failed while closing {}",
                        pending.session_id
                    )));
                    self.pending_cleanups
                        .insert(cleanup_key.clone(), Some(response));
                    self.cleanup_session_removals.insert(cleanup_key.clone());
                    return self.drive_pending_cleanup(host, &cleanup_key);
                }
                AcpPendingAbortReason::CallerCancelled => {
                    self.pending_closes.remove(&request.process_id);
                    if let Some(session) = self.session_mut(owner_id, &pending.session_id) {
                        session.closed = true;
                    }
                    let response = AcpResponse::AcpErrorResponse(AcpErrorResponse {
                        code: String::from("agent_interaction_cancelled"),
                        message: format!(
                            "agent interaction was cancelled while closing {}",
                            pending.session_id
                        ),
                    });
                    self.pending_cleanups
                        .insert(cleanup_key.clone(), Some(response));
                    self.cleanup_session_removals.insert(cleanup_key.clone());
                    return self.drive_pending_cleanup(host, &cleanup_key);
                }
            }
        }
        if let Some(pending) = self
            .pending_restarts
            .get(&request.process_id)
            .filter(|pending| pending.cleanup_only)
            .cloned()
        {
            if pending.owner_connection_id != owner_id {
                return Err(AcpCoreError::InvalidState(format!(
                    "no pending ACP interaction for {}",
                    request.process_id
                )));
            }
            let cleanup_key = self
                .promote_cleanup_only_restart(&request.process_id)
                .expect("cleanup-only replacement was checked above");
            return self.drive_pending_cleanup(host, &cleanup_key);
        }
        if let Some((pending_owner, session_id, pending_preamble, exit_code)) = self
            .pending_prompts
            .get(&request.process_id)
            .and_then(|pending| {
                pending.cleanup_restart_exit_code.map(|exit_code| {
                    (
                        pending.owner_connection_id.clone(),
                        pending.session_id.clone(),
                        pending.pending_preamble.clone(),
                        exit_code,
                    )
                })
            })
        {
            if pending_owner != owner_id {
                return Err(AcpCoreError::InvalidState(format!(
                    "no pending ACP interaction for {}",
                    request.process_id
                )));
            }
            if let Err(error) = host.abort_agent(&request.process_id) {
                return Err(AcpCoreError::Cleanup {
                    context: "failed to abort exited ACP adapter before restart",
                    errors: vec![error],
                });
            }
            self.pending_prompts.remove(&request.process_id);
            if let Some(session) = self.session_mut(owner_id, &session_id) {
                session.closed = true;
                if session.pending_preamble.is_none() {
                    session.pending_preamble = pending_preamble;
                }
            }
            return self.begin_resumable_adapter_restart(
                host,
                owner_id,
                &session_id,
                &request.process_id,
                exit_code,
            );
        }
        if request.reason == AcpPendingAbortReason::AgentExited {
            if let Some(pending) = self.pending_restarts.get(&request.process_id) {
                if pending.owner_connection_id != owner_id {
                    return Err(AcpCoreError::InvalidState(format!(
                        "no pending ACP interaction for {}",
                        request.process_id
                    )));
                }
                self.pending_restarts
                    .get_mut(&request.process_id)
                    .expect("pending restart was checked above")
                    .cleanup_only = true;
                if let Err(error) = host.abort_agent(&request.process_id) {
                    return Err(AcpCoreError::Cleanup {
                        context: "failed to abort exited replacement ACP adapter",
                        errors: vec![error],
                    });
                }
                let pending = self
                    .pending_restarts
                    .remove(&request.process_id)
                    .expect("cleanup-only restart was checked above");
                self.remove_session(&pending.owner_connection_id, &pending.session_id);
                let detail = format!(
                    "replacement ACP adapter {} exited before restart completed",
                    request.process_id
                );
                return self.finish_pending_restart_failure(&pending, &detail);
            }
            if let Some(pending) = self.pending_prompts.get(&request.process_id) {
                if pending.owner_connection_id != owner_id {
                    return Err(AcpCoreError::InvalidState(format!(
                        "no pending ACP interaction for {}",
                        request.process_id
                    )));
                }
                let (session_id, pending_preamble, exit_code) = {
                    let pending = self
                        .pending_prompts
                        .get_mut(&request.process_id)
                        .expect("pending prompt was checked above");
                    if pending.cleanup_restart_exit_code.is_none() {
                        pending.cleanup_restart_exit_code = Some(request.exit_code);
                    }
                    (
                        pending.session_id.clone(),
                        pending.pending_preamble.clone(),
                        pending.cleanup_restart_exit_code.flatten(),
                    )
                };
                if let Some(session) = self.session_mut(owner_id, &session_id) {
                    session.closed = true;
                    if session.pending_preamble.is_none() {
                        session.pending_preamble = pending_preamble;
                    }
                }
                if let Err(error) = host.abort_agent(&request.process_id) {
                    return Err(AcpCoreError::Cleanup {
                        context: "failed to abort exited ACP adapter before restart",
                        errors: vec![error],
                    });
                }
                self.pending_prompts.remove(&request.process_id);
                return self.begin_resumable_adapter_restart(
                    host,
                    owner_id,
                    &session_id,
                    &request.process_id,
                    exit_code,
                );
            }
        }
        let (code, message) = match request.reason {
            AcpPendingAbortReason::AgentExited => (
                "agent_exited",
                format!(
                    "agent exited before completing the ACP interaction ({})",
                    request.process_id
                ),
            ),
            AcpPendingAbortReason::InteractionTimeout => (
                "agent_interaction_timeout",
                format!("agent interaction timed out ({})", request.process_id),
            ),
            AcpPendingAbortReason::DriverFailed => (
                "agent_driver_failed",
                format!("ACP agent driver failed ({})", request.process_id),
            ),
            AcpPendingAbortReason::CallerCancelled => (
                "agent_interaction_cancelled",
                format!("agent interaction was cancelled ({})", request.process_id),
            ),
        };
        let response = AcpResponse::AcpErrorResponse(AcpErrorResponse {
            code: code.to_string(),
            message,
        });
        self.remove_pending_state(owner_id, &request.process_id)?;
        self.pending_cleanups
            .insert(cleanup_key.clone(), Some(response));
        self.drive_pending_cleanup(host, &cleanup_key)
    }

    fn drive_pending_cleanup<H: AcpHost>(
        &mut self,
        host: &mut H,
        cleanup_key: &(String, String),
    ) -> Result<AcpResponse, AcpCoreError> {
        let Some(completion) = self.pending_cleanups.get(cleanup_key).cloned() else {
            return Err(AcpCoreError::InvalidState(format!(
                "no pending ACP interaction for {}",
                cleanup_key.1
            )));
        };
        let host_cleanup_complete = self
            .pending_restart_cleanups
            .get(cleanup_key)
            .is_some_and(|completion| completion.host_cleanup_complete);
        if !host_cleanup_complete {
            if let Err(error) = host.abort_agent(&cleanup_key.1) {
                return Err(AcpCoreError::Cleanup {
                    context: "failed to abort ACP adapter during pending cleanup",
                    errors: vec![error],
                });
            }
            if let Some(completion) = self.pending_restart_cleanups.get_mut(cleanup_key) {
                completion.host_cleanup_complete = true;
            }
        }
        if let Some(completion) = self.pending_restart_cleanups.get(cleanup_key).cloned() {
            self.remove_session(
                &completion.pending.owner_connection_id,
                &completion.pending.session_id,
            );
            let exit_error = AcpCoreError::InvalidState(format!(
                "agent exited before completing the ACP interaction ({})",
                completion.pending.dead_process_id
            ));
            let terminal = self.finish_adapter_exit(
                &completion.pending.owner_connection_id,
                &completion.pending.session_id,
                &completion.pending.agent_type,
                &completion.pending.dead_process_id,
                completion.pending.exit_code,
                &completion.pending.restart,
                completion.outcome,
                Some(&completion.detail),
                completion.include_exit_error.then_some(&exit_error),
            )?;
            self.pending_cleanups.remove(cleanup_key);
            self.pending_restart_cleanups.remove(cleanup_key);
            self.process_route_limit_warned.remove(&cleanup_key.0);
            return Ok(crate::error_response(&terminal));
        }
        self.pending_cleanups.remove(cleanup_key);
        if self.cleanup_session_removals.remove(cleanup_key) {
            let closed_session_id = self
                .sessions
                .values()
                .find(|session| {
                    session.owner_connection_id == cleanup_key.0
                        && session.process_id == cleanup_key.1
                        && session.closed
                })
                .map(|session| session.session_id.clone());
            if let Some(session_id) = closed_session_id {
                self.remove_session(&cleanup_key.0, &session_id);
            }
        }
        self.process_route_limit_warned.remove(&cleanup_key.0);
        completion.ok_or_else(|| {
            AcpCoreError::InvalidState(format!(
                "ACP owner cleanup for {} has no client response",
                cleanup_key.1
            ))
        })
    }

    fn with_abort_cleanup<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        process_id: &str,
        context: &'static str,
        primary: AcpCoreError,
    ) -> AcpCoreError {
        let cleanup_key = (owner_id.to_string(), process_id.to_string());
        if self.pending_cleanups.contains_key(&cleanup_key) {
            return primary;
        }
        match host.abort_agent(process_id) {
            Ok(()) => {
                self.process_route_limit_warned.remove(owner_id);
                primary
            }
            Err(cleanup) => {
                self.pending_cleanups
                    .insert(cleanup_key, Some(crate::error_response(&primary)));
                AcpCoreError::Cleanup {
                    context,
                    errors: vec![primary, cleanup],
                }
            }
        }
    }

    fn retain_restart_cleanup_completion(
        &mut self,
        process_id: &str,
        pending: PendingRestart,
        outcome: &'static str,
        detail: String,
        include_exit_error: bool,
    ) {
        let cleanup_key = (pending.owner_connection_id.clone(), process_id.to_string());
        if self.pending_cleanups.contains_key(&cleanup_key) {
            self.pending_restart_cleanups.insert(
                cleanup_key,
                PendingRestartCleanup {
                    pending,
                    outcome,
                    detail,
                    include_exit_error,
                    host_cleanup_complete: false,
                },
            );
        }
    }

    fn promote_cleanup_only_restart(&mut self, process_id: &str) -> Option<(String, String)> {
        let pending = self
            .pending_restarts
            .remove(process_id)
            .filter(|pending| pending.cleanup_only)?;
        let cleanup_key = (pending.owner_connection_id.clone(), process_id.to_string());
        let detail =
            format!("replacement ACP adapter {process_id} exited before restart completed");
        let primary = AcpCoreError::InvalidState(detail.clone());
        self.pending_cleanups
            .insert(cleanup_key.clone(), Some(crate::error_response(&primary)));
        self.pending_restart_cleanups.insert(
            cleanup_key.clone(),
            PendingRestartCleanup {
                pending,
                outcome: "failed",
                detail,
                include_exit_error: true,
                host_cleanup_complete: false,
            },
        );
        Some(cleanup_key)
    }

    fn begin_resumable_adapter_restart<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session_id: &str,
        dead_process_id: &str,
        exit_code: Option<i32>,
    ) -> Result<AcpResponse, AcpCoreError> {
        self.ensure_event_capacity(owner_id, 1, 0)?;
        self.ensure_process_route_capacity(owner_id, 0)?;
        let session = self.session(owner_id, session_id).cloned().ok_or_else(|| {
            AcpCoreError::InvalidState(format!("unknown ACP session {session_id}"))
        })?;
        let agent_type = session.agent_type;
        let exit_error = AcpCoreError::InvalidState(format!(
            "agent exited before completing the ACP interaction ({dead_process_id})"
        ));
        let Some(mut restart) = session.restart else {
            self.remove_session(owner_id, session_id);
            let error = self.finish_adapter_exit(
                owner_id,
                session_id,
                &agent_type,
                dead_process_id,
                exit_code,
                &AcpAdapterRestartState {
                    runtime: AcpRuntimeKind::JavaScript,
                    entrypoint: String::new(),
                    args: Vec::new(),
                    env: BTreeMap::new(),
                    cwd: String::new(),
                    protocol_version: DEFAULT_ACP_PROTOCOL_VERSION,
                    client_capabilities: DEFAULT_ACP_CLIENT_CAPABILITIES.to_string(),
                    count: 0,
                },
                "unsupported",
                Some("launch state unavailable"),
                Some(&exit_error),
            )?;
            return Ok(crate::error_response(&error));
        };
        if restart.count >= MAX_ADAPTER_RESTARTS {
            self.remove_session(owner_id, session_id);
            let error = self.finish_adapter_exit(
                owner_id,
                session_id,
                &agent_type,
                dead_process_id,
                exit_code,
                &restart,
                "exhausted",
                None,
                Some(&exit_error),
            )?;
            return Ok(crate::error_response(&error));
        }
        restart.count += 1;

        if let Err(error) = resolve_agent(host, &agent_type) {
            self.remove_session(owner_id, session_id);
            let terminal = self.finish_adapter_exit(
                owner_id,
                session_id,
                &agent_type,
                dead_process_id,
                exit_code,
                &restart,
                "failed",
                Some(&error.to_string()),
                Some(&exit_error),
            )?;
            return Ok(crate::error_response(&terminal));
        }
        let process_id = self.allocate_process_id("acp-agent");
        let spawned = match host.spawn_agent(SpawnAgentRequest {
            process_id: process_id.clone(),
            runtime: restart.runtime.clone(),
            entrypoint: Some(restart.entrypoint.clone()),
            command: None,
            args: restart.args.clone(),
            env: restart.env.clone(),
            cwd: Some(restart.cwd.clone()),
        }) {
            Ok(spawned) => spawned,
            Err(error) => {
                self.remove_session(owner_id, session_id);
                let terminal = self.finish_adapter_exit(
                    owner_id,
                    session_id,
                    &agent_type,
                    dead_process_id,
                    exit_code,
                    &restart,
                    "failed",
                    Some(&error.to_string()),
                    Some(&exit_error),
                )?;
                return Ok(crate::error_response(&terminal));
            }
        };
        let client_capabilities =
            parse_json_text(&restart.client_capabilities, "clientCapabilities")?;
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": restart.protocol_version,
                "clientCapabilities": client_capabilities,
            },
        });
        if let Err(error) = write_json_line(host, &process_id, &initialize) {
            let detail = error.to_string();
            let error = self.with_abort_cleanup(
                host,
                owner_id,
                &process_id,
                "resumable adapter restart initial write",
                error,
            );
            self.remove_session(owner_id, session_id);
            if matches!(error, AcpCoreError::Cleanup { .. }) {
                self.retain_restart_cleanup_completion(
                    &process_id,
                    PendingRestart {
                        owner_connection_id: owner_id.to_string(),
                        session_id: session_id.to_string(),
                        agent_type,
                        dead_process_id: dead_process_id.to_string(),
                        exit_code,
                        pid: spawned.pid,
                        step: PendingRestartStep::AwaitingInitialize,
                        stdout_buffer: String::new(),
                        init_result: None,
                        agent_capabilities: None,
                        restart,
                        cleanup_only: true,
                    },
                    "failed",
                    detail,
                    true,
                );
                return Err(error);
            }
            let terminal = self.finish_adapter_exit(
                owner_id,
                session_id,
                &agent_type,
                dead_process_id,
                exit_code,
                &restart,
                "failed",
                Some(&error.to_string()),
                Some(&exit_error),
            )?;
            return Ok(crate::error_response(&terminal));
        }
        self.pending_restarts.insert(
            process_id.clone(),
            PendingRestart {
                owner_connection_id: owner_id.to_string(),
                session_id: session_id.to_string(),
                agent_type,
                dead_process_id: dead_process_id.to_string(),
                exit_code,
                pid: spawned.pid,
                step: PendingRestartStep::AwaitingInitialize,
                stdout_buffer: String::new(),
                init_result: None,
                agent_capabilities: None,
                restart,
                cleanup_only: false,
            },
        );
        self.pending_response(process_id)
    }

    /// Dispose every pending and live ACP process owned by one exact browser
    /// connection/wire-session/VM identity. State is removed before any fallible
    /// host cleanup so failed worker termination cannot strand routable sessions.
    pub fn dispose_owner<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
    ) -> Result<(), AcpCoreError> {
        let cleanup_actions = self.take_owner_state(owner_id);

        let mut errors = Vec::new();
        for action in cleanup_actions {
            match action {
                OwnerCleanupAction::Abort { process_id } => {
                    if let Err(error) = host.abort_agent(&process_id) {
                        tracing::error!(
                            owner_id,
                            process_id,
                            error_code = error.code(),
                            error = %error,
                            "failed to abort ACP process while disposing owner"
                        );
                        self.pending_cleanups
                            .insert((owner_id.to_string(), process_id), None);
                        errors.push(error);
                    }
                }
                OwnerCleanupAction::Finalize {
                    session_id,
                    process_id,
                } => {
                    if let Err(error) = host.finalize_session_cleanup(&session_id, &process_id) {
                        tracing::error!(
                            owner_id,
                            session_id,
                            process_id,
                            error_code = error.code(),
                            error = %error,
                            "failed to finalize ACP session while disposing owner"
                        );
                        self.pending_finalizations
                            .insert((owner_id.to_string(), process_id), session_id);
                        errors.push(error);
                    }
                }
            }
        }
        if errors.is_empty() {
            self.process_route_limits.remove(owner_id);
            self.process_route_limit_warned.remove(owner_id);
            Ok(())
        } else {
            Err(AcpCoreError::Cleanup {
                context: "failed to release disposed ACP owner executions",
                errors,
            })
        }
    }

    /// Remove all pending and live state for an owner without invoking host
    /// cleanup. This is for host teardown callbacks that run only after the
    /// owner's VM/process resources have already been destroyed.
    pub fn drop_owner_state(&mut self, owner_id: &str) {
        self.take_owner_state(owner_id);
        self.process_route_limits.remove(owner_id);
        self.process_route_limit_warned.remove(owner_id);
    }

    fn take_owner_state(&mut self, owner_id: &str) -> Vec<OwnerCleanupAction> {
        let mut process_ids = BTreeSet::new();
        let finalizations = self
            .pending_finalizations
            .iter()
            .filter(|((owner, _), _)| owner == owner_id)
            .map(|((_, process_id), session_id)| (process_id.clone(), session_id.clone()))
            .collect::<BTreeMap<_, _>>();
        process_ids.extend(
            self.pending_creates
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_resumes
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_prompts
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_restarts
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_closes
                .iter()
                .filter(|(_, pending)| pending.owner_connection_id == owner_id)
                .map(|(process_id, _)| process_id.clone()),
        );
        process_ids.extend(
            self.pending_cleanups
                .keys()
                .filter(|(owner, _)| owner == owner_id)
                .map(|(_, process_id)| process_id.clone()),
        );
        process_ids.extend(
            self.sessions
                .values()
                .filter(|session| session.owner_connection_id == owner_id && !session.closed)
                .map(|session| session.process_id.clone()),
        );
        for process_id in finalizations.keys() {
            process_ids.remove(process_id);
        }

        self.pending_creates
            .retain(|_, pending| pending.owner_connection_id != owner_id);
        self.pending_resumes
            .retain(|_, pending| pending.owner_connection_id != owner_id);
        self.pending_prompts
            .retain(|_, pending| pending.owner_connection_id != owner_id);
        self.pending_restarts
            .retain(|_, pending| pending.owner_connection_id != owner_id);
        self.pending_closes
            .retain(|_, pending| pending.owner_connection_id != owner_id);
        self.pending_cleanups
            .retain(|(owner, _), _| owner != owner_id);
        self.pending_restart_cleanups
            .retain(|(owner, _), _| owner != owner_id);
        self.cleanup_session_removals
            .retain(|(owner, _)| owner != owner_id);
        self.pending_finalizations
            .retain(|(owner, _), _| owner != owner_id);
        self.sessions
            .retain(|_, session| session.owner_connection_id != owner_id);
        self.pending_events
            .retain(|pending| pending.owner_connection_id != owner_id);
        self.pending_event_limits.remove(owner_id);
        self.pending_event_limit_warned.remove(owner_id);
        let mut actions = process_ids
            .into_iter()
            .map(|process_id| (process_id.clone(), OwnerCleanupAction::Abort { process_id }))
            .collect::<BTreeMap<_, _>>();
        for (process_id, session_id) in finalizations {
            actions.insert(
                process_id.clone(),
                OwnerCleanupAction::Finalize {
                    session_id,
                    process_id,
                },
            );
        }
        actions.into_values().collect()
    }

    fn remove_pending_state(
        &mut self,
        owner_id: &str,
        process_id: &str,
    ) -> Result<(), AcpCoreError> {
        let owner = self
            .pending_creates
            .get(process_id)
            .map(|pending| pending.owner_connection_id.as_str())
            .or_else(|| {
                self.pending_resumes
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_prompts
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_restarts
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            })
            .or_else(|| {
                self.pending_closes
                    .get(process_id)
                    .map(|pending| pending.owner_connection_id.as_str())
            });
        if owner != Some(owner_id) {
            return Err(AcpCoreError::InvalidState(format!(
                "no pending ACP interaction for {process_id}"
            )));
        }
        if self.pending_creates.remove(process_id).is_some()
            || self.pending_resumes.remove(process_id).is_some()
        {
            return Ok(());
        }
        if let Some(pending) = self.pending_restarts.remove(process_id) {
            self.remove_session(&pending.owner_connection_id, &pending.session_id);
            return Ok(());
        }
        if let Some(pending) = self.pending_closes.remove(process_id) {
            self.remove_session(&pending.owner_connection_id, &pending.session_id);
            return Ok(());
        }
        let pending = self
            .pending_prompts
            .remove(process_id)
            .expect("pending owner lookup found prompt state");
        if let Some(session) = self.session_mut(&pending.owner_connection_id, &pending.session_id) {
            session.closed = true;
            if session.pending_preamble.is_none() {
                session.pending_preamble = pending.pending_preamble;
            }
        }
        Ok(())
    }

    fn bootstrap_session<H: AcpHost>(
        &self,
        host: &mut H,
        owner_connection_id: &str,
        request: &ResolvedAcpCreateSessionRequest,
        process_id: &str,
    ) -> Result<SessionBootstrap, AcpCoreError> {
        let mut stdout = String::new();
        let client_capabilities =
            parse_json_text(&request.client_capabilities, "clientCapabilities")?;
        let mcp_servers = parse_json_text(&request.mcp_servers, "mcpServers")?;

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": request.protocol_version,
                "clientCapabilities": client_capabilities,
            },
        });
        let init_exchange = send_json_rpc_exchange(
            host,
            process_id,
            &initialize,
            1,
            INITIALIZE_TIMEOUT_MS,
            &mut stdout,
            self.available_event_capacity(owner_connection_id),
            self.available_event_bytes_capacity(owner_connection_id),
        )?;
        let init_result = response_result(init_exchange.response, "ACP initialize")?;
        validate_initialize_result(&init_result, request.protocol_version)?;

        let session_new = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": { "cwd": request.cwd, "mcpServers": mcp_servers },
        });
        let session_exchange = send_json_rpc_exchange(
            host,
            process_id,
            &session_new,
            2,
            SESSION_NEW_TIMEOUT_MS,
            &mut stdout,
            self.available_event_capacity(owner_connection_id)
                .saturating_sub(init_exchange.notifications.len()),
            self.available_event_bytes_capacity(owner_connection_id)
                .saturating_sub(init_exchange.notification_bytes),
        )?;
        let session_result = response_result(session_exchange.response, "ACP session/new")?;
        let session_id = session_id_from_session_result(&session_result, process_id);

        let fields = derive_bootstrap_fields(
            &request.agent_type,
            &init_result,
            &session_result,
            init_result.get("agentCapabilities"),
        )?;
        Ok(SessionBootstrap {
            session_id,
            modes: fields.modes,
            config_options: fields.config_options,
            agent_capabilities: fields.agent_capabilities,
            agent_info: fields.agent_info,
            stdout_buffer: stdout,
            notifications: init_exchange
                .notifications
                .into_iter()
                .chain(session_exchange.notifications)
                .collect(),
        })
    }

    /// In-session JSON-RPC (`session/prompt`, `session/set_mode`, `session/cancel`,
    /// etc.): owner-only, forwards the method+params to the live adapter over the
    /// seam and returns the agent's JSON-RPC response. Mirrors the native
    /// `session_request`.
    ///
    /// FOLLOW-UP (documented, parity-tracked): the native handler also (a) forwards
    /// adapter notifications emitted during the exchange as `AcpSessionEvent`s and
    /// synthesizes mode/plan notifications via `apply_request_success`, and (b)
    /// converts a `session/cancel` "method not found" into a `session/cancel`
    /// notification fallback. Those layer on the same loop once the host seam
    /// surfaces notifications; the core request/response path is faithful now.
    pub fn session_request<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpSessionRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let mut outbound_params = match request.params.as_deref() {
            Some(params) => to_record(parse_json_text(params, "session request params")?),
            None => Map::new(),
        };
        outbound_params.insert(
            String::from("sessionId"),
            Value::String(request.session_id.clone()),
        );

        // Enforce per-connection ownership and allocate the rpc id BEFORE any
        // outbound write. A non-owner (or missing session) fails closed with the
        // SAME error as `get_session_state` so the victim session is not revealed
        // and no state is mutated on a rejected attempt.
        let unknown =
            || AcpCoreError::InvalidState(format!("unknown ACP session {}", request.session_id));
        let session = self
            .session_mut(caller_connection_id, &request.session_id)
            .ok_or_else(unknown)?;
        if session.closed {
            return Err(unknown());
        }
        let rpc_id = session.allocate_request_id();
        // The transcript-continuation preamble is consumed once, on the first
        // `session/prompt` after a fallback resume; other methods leave it armed.
        let pending_preamble = if request.method == "session/prompt" {
            session.pending_preamble.take()
        } else {
            None
        };
        let process_id = session.process_id.clone();
        let mut stdout_buffer = std::mem::take(&mut session.stdout_buffer);

        if let Some(preamble) = pending_preamble.as_deref() {
            prepend_prompt_preamble(&mut outbound_params, preamble);
        }

        let outbound = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": request.method,
            "params": Value::Object(outbound_params.clone()),
        });
        let timeout = request_timeout_ms(&request.method);
        let exchange = match send_json_rpc_exchange(
            host,
            &process_id,
            &outbound,
            rpc_id,
            timeout,
            &mut stdout_buffer,
            self.available_event_capacity(caller_connection_id)
                .saturating_sub(1),
            self.available_event_bytes_capacity(caller_connection_id),
        ) {
            Ok(exchange) => exchange,
            Err(error) => {
                // Persist any drained stdout and re-arm the consumed preamble so a
                // transient failure does not silently drop transcript context.
                if let Some(session) = self.session_mut(caller_connection_id, &request.session_id) {
                    session.stdout_buffer = stdout_buffer;
                    if pending_preamble.is_some() && session.pending_preamble.is_none() {
                        session.pending_preamble = pending_preamble;
                    }
                }
                if let Some(exit_code) = adapter_gone_exit_code(&error) {
                    return Err(self.handle_adapter_exit(
                        host,
                        caller_connection_id,
                        &request.session_id,
                        exit_code,
                        error,
                    ));
                }
                return Err(error);
            }
        };

        if let Some(session) = self.session_mut(caller_connection_id, &request.session_id) {
            session.stdout_buffer = stdout_buffer;
        }

        let mut response = exchange.response;
        if cancel_fallback_decision(&request.method, &response)
            == AcpCancelFallbackDecision::SendNotification
        {
            write_json_line(host, &process_id, &cancel_notification(&request.session_id))?;
            let id = response
                .get("id")
                .cloned()
                .unwrap_or_else(|| Value::Number(rpc_id.into()));
            response = cancel_notification_fallback_response(id);
        }

        let text = if request.method == "session/prompt" {
            let mut capture = AcpPromptTextAccumulator::default();
            for notification in &exchange.notifications {
                if capture
                    .push_notification(notification)
                    .map_err(AcpCoreError::InvalidState)?
                {
                    tracing::warn!(
                        session_id = request.session_id,
                        "ACP prompt text capture is near its configured limit"
                    );
                }
            }
            Some(capture.into_text())
        } else {
            None
        };

        let notifications = exchange
            .notifications
            .iter()
            .cloned()
            .map(|notification| AcpSessionNotification {
                session_id: request.session_id.clone(),
                notification,
            })
            .collect::<Vec<_>>();
        let (mut updated_session, synthetic_notification) = if response.get("error").is_none() {
            let mut session = self
                .session(caller_connection_id, &request.session_id)
                .cloned()
                .ok_or_else(|| {
                    AcpCoreError::InvalidState(format!(
                        "unknown ACP session {}",
                        request.session_id
                    ))
                })?;
            let synthetic = apply_successful_session_request(
                &mut session,
                &request.method,
                &outbound_params,
                &notifications,
            )?;
            (Some(session), synthetic)
        } else {
            (None, None)
        };
        let response_text = serde_json::to_string(&response).map_err(|error| {
            AcpCoreError::InvalidState(format!("failed to serialize ACP session response: {error}"))
        })?;
        let mut notifications = notifications;
        if let Some(synthetic) = synthetic_notification {
            notifications.push(AcpSessionNotification {
                session_id: request.session_id.clone(),
                notification: synthetic.notification(),
            });
        }
        self.push_session_notification_batch(caller_connection_id, &notifications)?;
        if let Some(session) = updated_session.take() {
            self.insert_session(session);
        }
        Ok(AcpResponse::AcpSessionRpcResponse(AcpSessionRpcResponse {
            session_id: request.session_id.clone(),
            response: response_text,
            text,
        }))
    }

    fn handle_adapter_exit<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session_id: &str,
        exit_code: Option<i32>,
        error: AcpCoreError,
    ) -> AcpCoreError {
        if let Err(limit) = self.ensure_event_capacity(owner_id, 1, 0) {
            self.remove_session(owner_id, session_id);
            return limit;
        }
        let Some(session) = self.session_mut(owner_id, session_id) else {
            return error;
        };
        session.closed = true;
        session.exit_code = exit_code;
        let agent_type = session.agent_type.clone();
        let dead_process_id = session.process_id.clone();
        let Some(mut restart) = session.restart.clone() else {
            self.remove_session(owner_id, session_id);
            return AcpCoreError::InvalidState(format!(
                "{error}; ACP adapter auto-restart unsupported (launch state unavailable) — session evicted"
            ));
        };

        let (outcome, detail) = if restart.count >= MAX_ADAPTER_RESTARTS {
            self.remove_session(owner_id, session_id);
            (String::from("exhausted"), None)
        } else {
            restart.count += 1;
            match self.restart_adapter(host, owner_id, session_id, &agent_type, &restart) {
                Ok(()) => (String::from("restarted"), None),
                Err(AdapterRestartError::Unsupported) => {
                    self.remove_session(owner_id, session_id);
                    (
                        String::from("unsupported"),
                        Some(String::from(
                            "adapter does not advertise loadSession/resume",
                        )),
                    )
                }
                Err(AdapterRestartError::Failed(restart_error)) => {
                    self.remove_session(owner_id, session_id);
                    (String::from("failed"), Some(restart_error.to_string()))
                }
            }
        };

        self.finish_adapter_exit(
            owner_id,
            session_id,
            &agent_type,
            &dead_process_id,
            exit_code,
            &restart,
            &outcome,
            detail.as_deref(),
            Some(&error),
        )
        .unwrap_or_else(|limit| limit)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_adapter_exit(
        &mut self,
        owner_connection_id: &str,
        session_id: &str,
        agent_type: &str,
        dead_process_id: &str,
        exit_code: Option<i32>,
        restart: &AcpAdapterRestartState,
        outcome: &str,
        detail: Option<&str>,
        original_error: Option<&AcpCoreError>,
    ) -> Result<AcpCoreError, AcpCoreError> {
        let event = AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent {
            session_id: session_id.to_string(),
            agent_type: agent_type.to_string(),
            process_id: dead_process_id.to_string(),
            exit_code,
            restart: outcome.to_string(),
            restart_count: restart.count,
            max_restarts: MAX_ADAPTER_RESTARTS,
        });
        let encoded_bytes = Self::encoded_event_len(&event)?;
        self.ensure_event_capacity(owner_connection_id, 1, encoded_bytes)?;
        self.pending_events.push(PendingAcpEvent {
            owner_connection_id: owner_connection_id.to_string(),
            event,
            encoded_bytes,
        });

        let exit_diagnostic = match exit_code {
            Some(code) => format!("ACP adapter process {dead_process_id} exited with code {code}"),
            None => original_error
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("ACP adapter process {dead_process_id} exited")),
        };
        Ok(AcpCoreError::InvalidState(match outcome {
            "restarted" => format!(
                "{exit_diagnostic}; ACP adapter was auto-restarted (attempt {}/{MAX_ADAPTER_RESTARTS}) and the session is live again — retry the request",
                restart.count,
            ),
            "exhausted" => format!(
                "{exit_diagnostic}; ACP adapter restart budget exhausted ({MAX_ADAPTER_RESTARTS} restarts) — session evicted"
            ),
            _ => format!(
                "{exit_diagnostic}; ACP adapter auto-restart {outcome} ({}) — session evicted",
                detail.unwrap_or("no detail"),
            ),
        }))
    }

    fn restart_adapter<H: AcpHost>(
        &mut self,
        host: &mut H,
        owner_id: &str,
        session_id: &str,
        agent_type: &str,
        restart: &AcpAdapterRestartState,
    ) -> Result<(), AdapterRestartError> {
        // Re-resolve to verify the projected package is still present and to let
        // hosts associate the subsequent spawn with its agent package.
        resolve_agent(host, agent_type).map_err(AdapterRestartError::Failed)?;
        self.ensure_process_route_capacity(owner_id, 0)
            .map_err(AdapterRestartError::Failed)?;
        let process_id = self.allocate_process_id("acp-agent");
        let spawned = host
            .spawn_agent(SpawnAgentRequest {
                process_id: process_id.clone(),
                runtime: restart.runtime.clone(),
                entrypoint: Some(restart.entrypoint.clone()),
                command: None,
                args: restart.args.clone(),
                env: restart.env.clone(),
                cwd: Some(restart.cwd.clone()),
            })
            .map_err(AdapterRestartError::Failed)?;

        let result = (|| {
            let client_capabilities =
                parse_json_text(&restart.client_capabilities, "clientCapabilities")?;
            let mut stdout = String::new();
            let notification_limit = self.available_event_capacity(owner_id).saturating_sub(1);
            let notification_bytes_limit = self.available_event_bytes_capacity(owner_id);
            let initialize = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": restart.protocol_version,
                    "clientCapabilities": client_capabilities,
                },
            });
            let init_exchange = send_json_rpc_exchange(
                host,
                &process_id,
                &initialize,
                1,
                INITIALIZE_TIMEOUT_MS,
                &mut stdout,
                notification_limit,
                notification_bytes_limit,
            )?;
            let init_notifications = init_exchange.notifications.len();
            let init_notification_bytes = init_exchange.notification_bytes;
            let init_result = response_result(init_exchange.response, "ACP initialize")?;
            validate_initialize_result(&init_result, restart.protocol_version)?;
            let agent_capabilities = init_result.get("agentCapabilities").cloned();
            let Some(method) = native_resume_method(agent_capabilities.as_ref()) else {
                return Err(AdapterRestartError::Unsupported);
            };
            let load = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": method,
                "params": {
                    "sessionId": session_id,
                    "cwd": restart.cwd,
                    "mcpServers": [],
                },
            });
            let load_exchange = send_json_rpc_exchange(
                host,
                &process_id,
                &load,
                2,
                SESSION_NEW_TIMEOUT_MS,
                &mut stdout,
                notification_limit.saturating_sub(init_notifications),
                notification_bytes_limit.saturating_sub(init_notification_bytes),
            )?;
            let load_result = response_result(load_exchange.response, &format!("ACP {method}"))?;
            let fields = derive_bootstrap_fields(
                agent_type,
                &init_result,
                &load_result,
                agent_capabilities.as_ref(),
            )?;
            host.bind_session(session_id, &process_id)?;
            Ok((fields, stdout))
        })();

        let (fields, stdout) = match result {
            Ok(result) => result,
            Err(AdapterRestartError::Unsupported) => {
                let error = self.with_abort_cleanup(
                    host,
                    owner_id,
                    &process_id,
                    "adapter restart unsupported",
                    AcpCoreError::Unsupported(String::from("adapter restart is unsupported")),
                );
                return if matches!(error, AcpCoreError::Cleanup { .. }) {
                    Err(AdapterRestartError::Failed(error))
                } else {
                    Err(AdapterRestartError::Unsupported)
                };
            }
            Err(AdapterRestartError::Failed(error)) => {
                return Err(AdapterRestartError::Failed(self.with_abort_cleanup(
                    host,
                    owner_id,
                    &process_id,
                    "adapter restart failure",
                    error,
                )));
            }
        };
        let Some(session) = self.session_mut(owner_id, session_id) else {
            let error = AcpCoreError::InvalidState(format!(
                "ACP session {session_id} was removed during adapter restart"
            ));
            return Err(AdapterRestartError::Failed(self.with_abort_cleanup(
                host,
                owner_id,
                &process_id,
                "adapter restart session removed",
                error,
            )));
        };
        session.process_id = process_id;
        session.pid = spawned.pid;
        session.modes = fields.modes;
        session.config_options = fields.config_options;
        session.agent_capabilities = fields.agent_capabilities;
        session.agent_info = fields.agent_info;
        session.stdout_buffer = stdout;
        session.next_request_id = 3;
        session.closed = false;
        session.exit_code = None;
        session.restart = Some(restart.clone());
        Ok(())
    }

    fn prepare_session_config(
        &self,
        caller_connection_id: &str,
        request: &AcpSetSessionConfigRequest,
    ) -> Result<PreparedSessionConfig, AcpCoreError> {
        let unknown =
            || AcpCoreError::InvalidState(format!("unknown ACP session {}", request.session_id));
        let session = self
            .session(caller_connection_id, &request.session_id)
            .ok_or_else(unknown)?;
        if session.closed {
            return Err(unknown());
        }
        let selection = select_config_by_category(&session.config_options, &request.category)
            .map_err(AcpCoreError::InvalidState)?;
        if selection.read_only {
            return Ok(PreparedSessionConfig::Immediate(
                AcpResponse::AcpSessionRpcResponse(AcpSessionRpcResponse {
                    session_id: request.session_id.clone(),
                    response: json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": {
                            "code": -32601,
                            "message": read_only_config_message(
                                &session.agent_type,
                                &request.category,
                            ),
                        },
                    })
                    .to_string(),
                    text: None,
                }),
            ));
        }
        Ok(PreparedSessionConfig::Forward(AcpSessionRequest {
            session_id: request.session_id.clone(),
            method: String::from("session/set_config_option"),
            params: Some(
                json!({
                    "configId": selection.config_id,
                    "value": request.value,
                })
                .to_string(),
            ),
        }))
    }

    /// `session/resume`: re-attach a session that exists in durable storage but is
    /// not live in this VM. Launches a fresh adapter, re-probes its capabilities via
    /// `initialize`, then tries the native `session/load`/`session/resume` tier and
    /// falls back to a fresh `session/new` (arming the transcript-continuation
    /// preamble) on the `unknown_session` sentinel. Mirrors the native
    /// `resume_session` state machine (spec §6/§8).
    pub fn resume_session<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpResumeSessionRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        let request = ResolvedAcpResumeSessionRequest::from(request.clone());
        let resolved = resolve_agent(host, &request.agent_type)?;
        self.ensure_process_route_capacity(caller_connection_id, 1)?;

        let process_id = self.allocate_process_id("acp-agent");
        let (args, env) = prepare_agent_launch(
            host,
            &request.agent_type,
            &resolved,
            &[],
            &request.env,
            true,
            None,
        )?;
        let restart = AcpAdapterRestartState {
            runtime: AcpRuntimeKind::JavaScript,
            entrypoint: resolved.adapter_entrypoint.clone(),
            args: args.clone(),
            env: env.clone(),
            cwd: request.cwd.clone(),
            protocol_version: DEFAULT_ACP_PROTOCOL_VERSION,
            client_capabilities: String::from(DEFAULT_ACP_CLIENT_CAPABILITIES),
            count: 0,
        };

        let spawned = host.spawn_agent(SpawnAgentRequest {
            process_id: process_id.clone(),
            runtime: AcpRuntimeKind::JavaScript,
            entrypoint: Some(resolved.adapter_entrypoint.clone()),
            command: None,
            args,
            env,
            cwd: Some(request.cwd.clone()),
        })?;

        let outcome = match self.resume_bootstrap(host, caller_connection_id, &request, &process_id)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(self.with_abort_cleanup(
                    host,
                    caller_connection_id,
                    &process_id,
                    "blocking resume bootstrap failure",
                    error,
                ));
            }
        };
        let notifications = outcome.bootstrap.notifications.clone();

        let session = AcpSessionRecord {
            session_id: outcome.bootstrap.session_id.clone(),
            owner_connection_id: caller_connection_id.to_string(),
            agent_type: request.agent_type.clone(),
            process_id: process_id.clone(),
            pid: spawned.pid,
            modes: outcome.bootstrap.modes,
            config_options: outcome.bootstrap.config_options,
            agent_capabilities: outcome.bootstrap.agent_capabilities,
            agent_info: outcome.bootstrap.agent_info,
            stdout_buffer: outcome.bootstrap.stdout_buffer,
            next_request_id: 3,
            closed: false,
            exit_code: None,
            pending_preamble: outcome.pending_preamble,
            restart: Some(restart),
        };

        if self
            .session(caller_connection_id, &session.session_id)
            .is_some()
        {
            let error =
                AcpCoreError::InvalidState(format!("session id collision: {}", session.session_id));
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking resume session collision",
                error,
            ));
        }

        if let Err(error) = host.bind_session(&session.session_id, &process_id) {
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking resume bind failure",
                error,
            ));
        }
        let response = AcpResponse::AcpSessionResumedResponse(AcpSessionResumedResponse {
            session_id: session.session_id.clone(),
            mode: outcome.mode,
            agent_type: session.agent_type.clone(),
            process_id: session.process_id.clone(),
            pid: session.pid,
        });
        let session_id = session.session_id.clone();
        self.insert_session(session);
        let notifications = notifications
            .into_iter()
            .map(|notification| AcpSessionNotification {
                session_id: session_id.clone(),
                notification,
            })
            .collect::<Vec<_>>();
        if let Err(error) =
            self.push_session_notification_batch(caller_connection_id, &notifications)
        {
            self.remove_session(caller_connection_id, &session_id);
            return Err(self.with_abort_cleanup(
                host,
                caller_connection_id,
                &process_id,
                "blocking resume event commit failure",
                error,
            ));
        }
        Ok(response)
    }

    fn resume_bootstrap<H: AcpHost>(
        &self,
        host: &mut H,
        owner_connection_id: &str,
        request: &ResolvedAcpResumeSessionRequest,
        process_id: &str,
    ) -> Result<ResumeOutcome, AcpCoreError> {
        let mut stdout = String::new();
        let client_capabilities =
            parse_json_text(DEFAULT_ACP_CLIENT_CAPABILITIES, "clientCapabilities")?;

        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": DEFAULT_ACP_PROTOCOL_VERSION,
                "clientCapabilities": client_capabilities,
            },
        });
        let init_exchange = send_json_rpc_exchange(
            host,
            process_id,
            &initialize,
            1,
            INITIALIZE_TIMEOUT_MS,
            &mut stdout,
            self.available_event_capacity(owner_connection_id),
            self.available_event_bytes_capacity(owner_connection_id),
        )?;
        let init_result = response_result(init_exchange.response, "ACP initialize")?;
        validate_initialize_result(&init_result, DEFAULT_ACP_PROTOCOL_VERSION)?;
        let agent_capabilities = init_result.get("agentCapabilities").cloned();
        let mut notification_bytes = init_exchange.notification_bytes;
        let mut notifications = init_exchange.notifications;

        // Tier 1 — native (capability-gated). Re-probed caps decide eligibility.
        if let Some(native_method) = native_resume_method(agent_capabilities.as_ref()) {
            let load = json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": native_method,
                "params": { "sessionId": request.session_id, "cwd": request.cwd, "mcpServers": [] },
            });
            let load_exchange = send_json_rpc_exchange(
                host,
                process_id,
                &load,
                2,
                SESSION_NEW_TIMEOUT_MS,
                &mut stdout,
                self.available_event_capacity(owner_connection_id)
                    .saturating_sub(notifications.len()),
                self.available_event_bytes_capacity(owner_connection_id)
                    .saturating_sub(notification_bytes),
            )?;
            notification_bytes =
                notification_bytes.saturating_add(load_exchange.notification_bytes);
            notifications.extend(load_exchange.notifications);
            let mut load_response = load_exchange.response;
            normalize_unknown_session_error(&mut load_response);
            if load_response.get("error").is_none() {
                let load_result = response_result(load_response, &format!("ACP {native_method}"))?;
                let mut bootstrap = self.build_bootstrap(
                    request.session_id.clone(),
                    &init_result,
                    &load_result,
                    &request.agent_type,
                    agent_capabilities.as_ref(),
                    stdout,
                )?;
                bootstrap.notifications = notifications;
                return Ok(ResumeOutcome {
                    bootstrap,
                    mode: String::from("native"),
                    pending_preamble: None,
                });
            }
            // Only the `unknown_session` sentinel falls through; every other error
            // propagates verbatim (the durable store survived; this is a real error).
            if !is_unknown_session_error(&load_response) {
                return Err(
                    response_result(load_response, &format!("ACP {native_method}"))
                        .expect_err("native resume error object must map to an AcpCoreError"),
                );
            }
            // fall through to Tier 2
        }

        // Tier 2 — universal fallback: a fresh session plus the transcript pointer.
        let session_new = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": { "cwd": request.cwd, "mcpServers": [] },
        });
        let session_exchange = send_json_rpc_exchange(
            host,
            process_id,
            &session_new,
            2,
            SESSION_NEW_TIMEOUT_MS,
            &mut stdout,
            self.available_event_capacity(owner_connection_id)
                .saturating_sub(notifications.len()),
            self.available_event_bytes_capacity(owner_connection_id)
                .saturating_sub(notification_bytes),
        )?;
        notifications.extend(session_exchange.notifications);
        let session_result = response_result(session_exchange.response, "ACP session/new")?;
        let live_session_id = session_id_from_session_result(&session_result, process_id);
        let pending_preamble = request
            .transcript_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .map(|path| CONTINUATION_PREAMBLE.replace("{path}", path));
        let mut bootstrap = self.build_bootstrap(
            live_session_id,
            &init_result,
            &session_result,
            &request.agent_type,
            agent_capabilities.as_ref(),
            stdout,
        )?;
        bootstrap.notifications = notifications;
        Ok(ResumeOutcome {
            bootstrap,
            mode: String::from("fallback"),
            pending_preamble,
        })
    }

    fn build_bootstrap(
        &self,
        session_id: String,
        init_result: &Map<String, Value>,
        session_result: &Map<String, Value>,
        agent_type: &str,
        agent_capabilities: Option<&Value>,
        stdout_buffer: String,
    ) -> Result<SessionBootstrap, AcpCoreError> {
        let fields =
            derive_bootstrap_fields(agent_type, init_result, session_result, agent_capabilities)?;
        Ok(SessionBootstrap {
            session_id,
            modes: fields.modes,
            config_options: fields.config_options,
            agent_capabilities: fields.agent_capabilities,
            agent_info: fields.agent_info,
            stdout_buffer,
            notifications: Vec::new(),
        })
    }

    /// Dispatch a decoded ACP request to the right handler.
    pub fn dispatch<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: AcpRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        match request {
            AcpRequest::AcpCreateSessionRequest(request) => {
                self.create_session(host, caller_connection_id, &request)
            }
            AcpRequest::AcpGetSessionStateRequest(request) => {
                self.get_session_state(caller_connection_id, &request)
            }
            AcpRequest::AcpListSessionsRequest(_) => Ok(self.list_sessions(caller_connection_id)),
            AcpRequest::AcpCloseSessionRequest(request) => {
                self.close_session(host, caller_connection_id, &request)
            }
            AcpRequest::AcpSessionRequest(request) => {
                self.session_request(host, caller_connection_id, &request)
            }
            AcpRequest::AcpSetSessionConfigRequest(request) => {
                match self.prepare_session_config(caller_connection_id, &request)? {
                    PreparedSessionConfig::Immediate(response) => Ok(response),
                    PreparedSessionConfig::Forward(request) => {
                        self.session_request(host, caller_connection_id, &request)
                    }
                }
            }
            AcpRequest::AcpResumeSessionRequest(request) => {
                self.resume_session(host, caller_connection_id, &request)
            }
            AcpRequest::AcpDeliverAgentOutputRequest(request) => {
                self.deliver_agent_output(host, caller_connection_id, &request)
            }
            AcpRequest::AcpDeliverAgentStderrRequest(request) => {
                self.deliver_agent_stderr(caller_connection_id, &request)
            }
            AcpRequest::AcpAbortPendingRequest(request) => {
                self.abort_pending(host, caller_connection_id, &request)
            }
            AcpRequest::AcpListAgentsRequest(_) => {
                let mut agents = host.list_projected_agents()?;
                agents.sort_by(|left, right| left.id.cmp(&right.id));
                Ok(AcpResponse::AcpListAgentsResponse(AcpListAgentsResponse {
                    agents: agents
                        .into_iter()
                        .map(|agent| AcpAgentEntry {
                            id: agent.id,
                            installed: true,
                            adapter_entrypoint: agent.adapter_entrypoint,
                        })
                        .collect(),
                }))
            }
        }
    }

    /// RESUMABLE dispatch (browser path): create, resume, and in-session RPCs start
    /// a non-blocking interaction and return [`AcpPendingResponse`] with the process
    /// handle. `deliver_agent_output` feeds stdout and returns the real result once
    /// complete (else another pending response). Pure state operations are handled
    /// immediately. This keeps the worker from block-waiting inside `pushFrame`
    /// while an agent makes a mid-turn syscall (§3.2.1).
    pub fn dispatch_resumable<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: AcpRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        match request {
            AcpRequest::AcpCreateSessionRequest(request) => {
                let process_id = self.begin_create_session(host, caller_connection_id, &request)?;
                self.pending_response(process_id)
            }
            AcpRequest::AcpSessionRequest(request) => {
                let process_id =
                    self.begin_session_request(host, caller_connection_id, &request)?;
                self.pending_response(process_id)
            }
            AcpRequest::AcpSetSessionConfigRequest(request) => {
                match self.prepare_session_config(caller_connection_id, &request)? {
                    PreparedSessionConfig::Immediate(response) => Ok(response),
                    PreparedSessionConfig::Forward(request) => {
                        let process_id =
                            self.begin_session_request(host, caller_connection_id, &request)?;
                        self.pending_response(process_id)
                    }
                }
            }
            AcpRequest::AcpResumeSessionRequest(request) => {
                let process_id = self.begin_resume_session(host, caller_connection_id, &request)?;
                self.pending_response(process_id)
            }
            AcpRequest::AcpCloseSessionRequest(request) => {
                self.begin_close_session(host, caller_connection_id, &request)
            }
            AcpRequest::AcpDeliverAgentOutputRequest(request) => {
                self.deliver_agent_output(host, caller_connection_id, &request)
            }
            AcpRequest::AcpDeliverAgentStderrRequest(request) => {
                self.deliver_agent_stderr(caller_connection_id, &request)
            }
            AcpRequest::AcpAbortPendingRequest(request) => {
                self.abort_pending(host, caller_connection_id, &request)
            }
            other => self.dispatch(host, caller_connection_id, other),
        }
    }

    fn deliver_agent_output<H: AcpHost>(
        &mut self,
        host: &mut H,
        caller_connection_id: &str,
        request: &AcpDeliverAgentOutputRequest,
    ) -> Result<AcpResponse, AcpCoreError> {
        match self.feed_agent_output(
            host,
            caller_connection_id,
            &request.process_id,
            &request.chunk,
        )? {
            ResumeStep::Pending => self.pending_response(request.process_id.clone()),
            ResumeStep::Done(response) => Ok(response),
        }
    }
}

/// Outcome of the resume handshake: the bootstrap state plus the chosen tier mode
/// (`native`/`fallback`) and any armed transcript-continuation preamble.
struct ResumeOutcome {
    bootstrap: SessionBootstrap,
    mode: String,
    pending_preamble: Option<String>,
}

// ---- host-free helpers ported from agentos-sidecar::acp_extension ----

/// Coerce a parsed JSON value into an object map (a non-object becomes empty), so
/// session-request params are always a JSON object we can inject `sessionId` into.
fn to_record(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Per-method JSON-RPC timeout. Mirrors the native `request_timeout`.
fn request_timeout_ms(method: &str) -> u64 {
    match method {
        "session/prompt" => 600_000,
        "initialize" => INITIALIZE_TIMEOUT_MS,
        "session/new" => SESSION_NEW_TIMEOUT_MS,
        _ => 120_000,
    }
}

fn adapter_gone_exit_code(error: &AcpCoreError) -> Option<Option<i32>> {
    let (AcpCoreError::Execution(message) | AcpCoreError::InvalidState(message)) = error else {
        return None;
    };
    if let Some(raw) = message
        .split("agent process exited (code=Some(")
        .nth(1)
        .and_then(|tail| tail.split(')').next())
    {
        return raw.parse::<i32>().ok().map(Some);
    }
    if message.contains("agent process exited (code=None)")
        || message.contains("has no active process")
    {
        return Some(None);
    }
    None
}

/// Prepend the transcript-continuation preamble as a leading text block on a
/// `session/prompt`'s `prompt` array (initialized if absent). Mirrors the native
/// `prepend_prompt_preamble`.
fn prepend_prompt_preamble(params: &mut Map<String, Value>, preamble: &str) {
    let block = json!({ "type": "text", "text": preamble });
    match params.get_mut("prompt").and_then(Value::as_array_mut) {
        Some(prompt) => prompt.insert(0, block),
        None => {
            params.insert(String::from("prompt"), Value::Array(vec![block]));
        }
    }
}

/// The adapter's native-resume RPC method from re-probed `agentCapabilities`:
/// prefer ACP `session/load`, then the non-standard `session/resume`. Mirrors the
/// native `native_resume_method`.
fn native_resume_method(agent_capabilities: Option<&Value>) -> Option<&'static str> {
    let caps = agent_capabilities.and_then(Value::as_object)?;
    if caps
        .get("loadSession")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some("session/load");
    }
    if caps.get("resume").and_then(Value::as_bool).unwrap_or(false) {
        return Some("session/resume");
    }
    None
}

/// Normalize an adapter "no such session" error (`-32603` + `details ==
/// "NotFoundError"`) into the shared `unknown_session` discriminator. Strict on
/// purpose: malformed `session/load` must still propagate. Mirrors the native
/// `normalize_unknown_session_error`.
fn normalize_unknown_session_error(response: &mut Value) {
    let Some(error) = response.get_mut("error").and_then(Value::as_object_mut) else {
        return;
    };
    let code = error.get("code").and_then(Value::as_i64);
    let Some(data) = error.get_mut("data").and_then(Value::as_object_mut) else {
        return;
    };
    let details = data.get("details").and_then(Value::as_str);
    if code == Some(-32603) && details == Some("NotFoundError") {
        data.insert(
            String::from("kind"),
            Value::String(String::from("unknown_session")),
        );
    }
}

/// Detect the normalized `unknown_session` fallthrough sentinel. Only this triggers
/// the Tier 2 fallback; transport/timeout errors propagate. Mirrors the native
/// `is_unknown_session_error`.
fn is_unknown_session_error(response: &Value) -> bool {
    response
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("data"))
        .and_then(Value::as_object)
        .and_then(|d| d.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|kind| kind == "unknown_session")
}

/// True when a signal/kill request failed because the target process no longer
/// exists — the process-table `ESRCH` / "no such process" / "has no active
/// process" errors surfaced for an already-reaped PID. `close_session` uses
/// this to skip the exit wait when the adapter is already gone. Mirrors the
/// native sidecar's `is_process_already_gone_error`.
fn is_process_already_gone_error(error: &AcpCoreError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("esrch")
        || message.contains("no such process")
        || message.contains("has no active process")
}

/// Write a JSON-RPC message as a single newline-terminated line to the agent's
/// stdin (no waiting). Used by the resumable handshake.
fn write_json_line<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    message: &Value,
) -> Result<(), AcpCoreError> {
    let mut line = serde_json::to_vec(message).map_err(|error| {
        AcpCoreError::InvalidState(format!("failed to serialize ACP request: {error}"))
    })?;
    line.push(b'\n');
    host.write_stdin(process_id, &line)
}

fn answer_inbound_request<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    request: &Value,
) -> Result<(), AcpCoreError> {
    let response = host.handle_inbound_request(process_id, request)?;
    write_json_line(host, process_id, &response)
}

fn write_resume_session_new<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    cwd: &str,
) -> Result<(), AcpCoreError> {
    write_json_line(
        host,
        process_id,
        &json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": { "cwd": cwd, "mcpServers": [] },
        }),
    )
}

fn parse_json_text(text: &str, label: &str) -> Result<Value, AcpCoreError> {
    serde_json::from_str(text)
        .map_err(|error| AcpCoreError::InvalidState(format!("invalid {label} JSON: {error}")))
}

fn response_result(response: Value, label: &str) -> Result<Map<String, Value>, AcpCoreError> {
    if let Some(error) = response.get("error").and_then(Value::as_object) {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown ACP error");
        let data = error
            .get("data")
            .map(|d| format!(" (data: {d})"))
            .unwrap_or_default();
        return Err(AcpCoreError::Execution(format!(
            "{label} failed: {message}{data}"
        )));
    }
    response
        .get("result")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| AcpCoreError::InvalidState(format!("{label} response missing result")))
}

fn validate_initialize_result(
    result: &Map<String, Value>,
    requested_protocol_version: i32,
) -> Result<(), AcpCoreError> {
    let reported = result
        .get("protocolVersion")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            AcpCoreError::InvalidState(String::from(
                "ACP initialize response missing protocolVersion",
            ))
        })?;
    if reported != i64::from(requested_protocol_version) {
        return Err(AcpCoreError::InvalidState(format!(
            "ACP initialize protocolVersion mismatch: requested {requested_protocol_version}, agent reported {reported}"
        )));
    }
    Ok(())
}

fn session_id_from_session_result(session_result: &Map<String, Value>, fallback: &str) -> String {
    session_result
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{AgentOutput, ProjectedAgentLaunch, SpawnAgentRequest, SpawnedAgent};

    fn projected_agent(id: &str) -> ProjectedAgentLaunch {
        ProjectedAgentLaunch {
            id: id.to_string(),
            adapter_entrypoint: String::from("/opt/agentos/bin/echo-agent"),
            env: BTreeMap::new(),
            launch_args: Vec::new(),
        }
    }

    #[derive(Default)]
    struct MockHost {
        killed: Vec<(String, String)>,
        closed_stdin: Vec<String>,
        wait_failures_remaining: usize,
        finalization_failures_remaining: usize,
        finalized: Vec<(String, String)>,
        cleanup_order: Vec<String>,
    }

    impl AcpHost for MockHost {
        fn resolve_projected_agent(
            &mut self,
            id: &str,
        ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(Some(projected_agent(id)))
        }
        fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(vec![projected_agent("echo")])
        }
        fn spawn_agent(&mut self, _: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
            unreachable!("non-process handlers do not spawn")
        }
        fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn write_stdin(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
            unreachable!()
        }
        fn close_stdin(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
            self.closed_stdin.push(process_id.to_string());
            Ok(())
        }
        fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
            Ok(None)
        }
        fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
            self.cleanup_order
                .push(format!("signal:{process_id}:{signal}"));
            self.killed
                .push((process_id.to_string(), signal.to_string()));
            Ok(())
        }
        fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
            if self.wait_failures_remaining > 0 {
                self.wait_failures_remaining -= 1;
                return Err(AcpCoreError::Execution(String::from(
                    "injected wait failure",
                )));
            }
            Ok(Some(0)) // exits promptly after SIGTERM
        }
        fn finalize_session_cleanup(
            &mut self,
            session_id: &str,
            process_id: &str,
        ) -> Result<(), AcpCoreError> {
            self.finalized
                .push((session_id.to_string(), process_id.to_string()));
            self.cleanup_order.push(format!("finalize:{process_id}"));
            if self.finalization_failures_remaining > 0 {
                self.finalization_failures_remaining -= 1;
                return Err(AcpCoreError::Cleanup {
                    context: "injected finalization failure",
                    errors: vec![AcpCoreError::Execution(String::from(
                        "worker termination failed",
                    ))],
                });
            }
            Ok(())
        }
        fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
            Ok(Vec::new())
        }
        fn now_ms(&self) -> u64 {
            0
        }
    }

    fn record(session_id: &str, owner: &str) -> AcpSessionRecord {
        AcpSessionRecord {
            session_id: session_id.into(),
            owner_connection_id: owner.into(),
            agent_type: "echo".into(),
            process_id: format!("proc-{session_id}"),
            pid: Some(42),
            modes: None,
            config_options: Vec::new(),
            agent_capabilities: None,
            agent_info: None,
            stdout_buffer: String::new(),
            next_request_id: 1,
            closed: false,
            exit_code: None,
            pending_preamble: None,
            restart: None,
        }
    }

    fn pending_session_event(owner: &str, notification: &str) -> PendingAcpEvent {
        let event = AcpEvent::AcpSessionEvent(AcpSessionEvent {
            session_id: String::from("s1"),
            notification: notification.to_string(),
        });
        let encoded_bytes = AcpCore::encoded_event_len(&event).expect("size test event");
        PendingAcpEvent {
            owner_connection_id: owner.to_string(),
            event,
            encoded_bytes,
        }
    }

    #[test]
    fn pending_event_limit_rejects_zero_and_queued_event_underflow() {
        let mut core = AcpCore::new();
        assert!(core.set_pending_event_limit("owner-a", 0).is_err());
        core.pending_events
            .push(pending_session_event("owner-a", "{}"));
        core.pending_events
            .push(pending_session_event("owner-a", "{}"));
        let error = core
            .set_pending_event_limit("owner-a", 1)
            .expect_err("cannot lower beneath queued events");
        assert!(error.to_string().contains("current usage (2 events"));
        assert_eq!(
            core.event_limits("owner-a").0,
            DEFAULT_ACP_PENDING_EVENT_LIMIT
        );
    }

    #[test]
    fn pending_event_limit_resets_warning_state_for_the_new_capacity() {
        let mut core = AcpCore::new();
        core.pending_events
            .push(pending_session_event("owner-a", "{}"));
        core.pending_event_limit_warned
            .insert(String::from("owner-a"));
        core.set_pending_event_limit("owner-a", 10)
            .expect("raise limit");
        assert!(!core.pending_event_limit_warned.contains("owner-a"));
        core.set_pending_event_limit("owner-a", 1)
            .expect("equal queue depth");
        assert!(core.pending_event_limit_warned.contains("owner-a"));
    }

    #[test]
    fn event_delivery_snapshot_retains_unacknowledged_suffix_in_order() {
        let mut core = AcpCore::new();
        for notification in ["first", "second", "third"] {
            core.pending_events
                .push(pending_session_event("owner-a", notification));
        }

        assert_eq!(core.events_for_delivery("owner-a").len(), 3);
        core.acknowledge_delivered_events("owner-a", 1)
            .expect("acknowledge delivered prefix");
        let remaining = core.events_for_delivery("owner-a");
        assert_eq!(remaining.len(), 2);
        let AcpEvent::AcpSessionEvent(first_remaining) = &remaining[0] else {
            panic!("expected session event");
        };
        assert_eq!(first_remaining.notification, "second");
        assert!(core.acknowledge_delivered_events("owner-a", 3).is_err());
        assert_eq!(core.pending_event_count(), 2);
    }

    #[test]
    fn pending_events_are_owner_scoped_and_disposal_purges_only_that_owner() {
        let mut core = AcpCore::new();
        core.pending_events
            .push(pending_session_event("owner-a", "a-first"));
        core.pending_events
            .push(pending_session_event("owner-b", "b-only"));
        core.pending_events
            .push(pending_session_event("owner-a", "a-second"));
        core.set_pending_event_limits("owner-a", 2, usize::MAX)
            .expect("owner A may fill its own quota");
        core.set_pending_event_limits("owner-b", 1, usize::MAX)
            .expect("owner A's full quota must not block owner B");

        let owner_b = core.events_for_delivery("owner-b");
        assert_eq!(owner_b.len(), 1);
        let AcpEvent::AcpSessionEvent(owner_b_event) = &owner_b[0] else {
            panic!("expected owner B session event");
        };
        assert_eq!(owner_b_event.notification, "b-only");
        core.acknowledge_delivered_events("owner-b", 1)
            .expect("acknowledge only owner B");
        assert_eq!(core.events_for_delivery("owner-a").len(), 2);
        assert!(core.events_for_delivery("owner-b").is_empty());

        core.drop_owner_state("owner-a");
        assert_eq!(core.pending_event_count(), 0);
    }

    #[test]
    fn agent_stderr_is_owner_scoped_bounded_and_retryable() {
        let mut core = AcpCore::with_pending_event_limits(1, 4_096);
        core.insert_session(record("s1", "owner-a"));
        let request = AcpDeliverAgentStderrRequest {
            process_id: String::from("proc-s1"),
            chunk: b"warning\n".to_vec(),
        };

        assert!(core.deliver_agent_stderr("owner-b", &request).is_err());
        assert!(matches!(
            core.deliver_agent_stderr("owner-a", &request),
            Ok(AcpResponse::AcpAgentStderrDeliveredResponse(_))
        ));
        assert!(core.deliver_agent_stderr("owner-a", &request).is_err());
        let retained = core.events_for_delivery("owner-a");
        assert_eq!(retained.len(), 1);
        let AcpEvent::AcpAgentStderrEvent(stderr) = &retained[0] else {
            panic!("expected stderr event")
        };
        assert_eq!(stderr.session_id, "s1");
        assert_eq!(stderr.agent_type, "echo");
        assert_eq!(stderr.process_id, "proc-s1");
        assert_eq!(stderr.chunk, b"warning\n");
        assert_eq!(core.events_for_delivery("owner-a"), retained);
        core.acknowledge_delivered_events("owner-a", 1)
            .expect("acknowledge stderr only after host delivery");
        assert!(core.events_for_delivery("owner-a").is_empty());
    }

    #[test]
    fn get_session_state_enforces_ownership() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let req = AcpGetSessionStateRequest {
            session_id: "s1".into(),
        };
        // Owner reads it.
        assert!(core.get_session_state("conn-a", &req).is_ok());
        // Non-owner gets the same "unknown" error (no cross-tenant leak).
        let err = core.get_session_state("conn-b", &req).unwrap_err();
        assert_eq!(err.code(), "invalid_state");
        assert!(err.to_string().contains("unknown ACP session"));
    }

    #[test]
    fn list_sessions_is_owner_scoped() {
        let mut core = AcpCore::new();
        core.insert_session(record("a1", "conn-a"));
        core.insert_session(record("b1", "conn-b"));

        let AcpResponse::AcpListSessionsResponse(listed) = core.list_sessions("conn-a") else {
            panic!("unexpected list response");
        };
        assert_eq!(listed.sessions.len(), 1);
        assert_eq!(listed.sessions[0].session_id, "a1");
        assert_eq!(listed.sessions[0].agent_type, "echo");
    }

    #[test]
    fn identical_adapter_session_ids_are_independent_per_owner() {
        let mut core = AcpCore::new();
        let mut owner_a = record("same-id", "owner-a");
        owner_a.process_id = String::from("process-a");
        let mut owner_b = record("same-id", "owner-b");
        owner_b.process_id = String::from("process-b");
        core.insert_session(owner_a);
        core.insert_session(owner_b);

        assert_eq!(core.session_count(), 2);
        assert_eq!(
            core.session("owner-a", "same-id").unwrap().process_id,
            "process-a"
        );
        assert_eq!(
            core.session("owner-b", "same-id").unwrap().process_id,
            "process-b"
        );
        let AcpResponse::AcpListSessionsResponse(owner_a_list) = core.list_sessions("owner-a")
        else {
            panic!("expected owner-scoped session list");
        };
        assert_eq!(owner_a_list.sessions.len(), 1);

        let mut host = MockHost::default();
        core.close_session(
            &mut host,
            "owner-a",
            &AcpCloseSessionRequest {
                session_id: String::from("same-id"),
            },
        )
        .expect("owner A closes only its same-named session");
        assert!(core.session("owner-a", "same-id").is_none());
        assert!(core.session("owner-b", "same-id").is_some());
        assert_eq!(host.closed_stdin, vec![String::from("process-a")]);
    }

    #[test]
    fn close_session_is_idempotent_owner_only_and_kills_process() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = MockHost::default();
        let req = AcpCloseSessionRequest {
            session_id: "s1".into(),
        };
        // Non-owner receives the same idempotent response as an absent id, but
        // cannot close the owner's session.
        assert!(matches!(
            core.close_session(&mut host, "conn-b", &req),
            Ok(AcpResponse::AcpSessionClosedResponse(_))
        ));
        assert_eq!(core.session_count(), 1);
        // Owner closes: process is torn down and the record removed.
        let resp = core
            .close_session(&mut host, "conn-a", &req)
            .expect("close");
        assert!(matches!(resp, AcpResponse::AcpSessionClosedResponse(_)));
        assert_eq!(core.session_count(), 0);
        assert_eq!(host.closed_stdin, vec!["proc-s1".to_string()]);
        assert_eq!(host.killed, vec![("proc-s1".into(), "SIGTERM".into())]);

        // Repeated close requires no client tombstone and performs no teardown.
        assert!(matches!(
            core.close_session(&mut host, "conn-a", &req),
            Ok(AcpResponse::AcpSessionClosedResponse(_))
        ));
        assert_eq!(host.closed_stdin, vec!["proc-s1".to_string()]);
        assert_eq!(host.killed, vec![("proc-s1".into(), "SIGTERM".into())]);
    }

    #[test]
    fn close_session_retains_authoritative_state_until_cleanup_can_be_retried() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = MockHost {
            wait_failures_remaining: 1,
            ..MockHost::default()
        };
        let request = AcpCloseSessionRequest {
            session_id: String::from("s1"),
        };

        let error = core
            .close_session(&mut host, "conn-a", &request)
            .expect_err("first close must surface the cleanup failure");
        assert_eq!(error.code(), "execution");
        assert_eq!(core.session_count(), 1, "failed close stays retryable");

        core.close_session(&mut host, "conn-a", &request)
            .expect("retry completes cleanup");
        assert_eq!(core.session_count(), 0);
    }

    #[test]
    fn close_finalization_retry_is_non_routable_and_does_not_repeat_signals() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = MockHost {
            finalization_failures_remaining: 1,
            ..MockHost::default()
        };
        let request = AcpCloseSessionRequest {
            session_id: String::from("s1"),
        };

        let error = core
            .close_session(&mut host, "conn-a", &request)
            .expect_err("first host finalization fails");
        assert_eq!(error.code(), "cleanup_failed");
        assert!(core.session("conn-a", "s1").unwrap().closed);
        let AcpResponse::AcpListSessionsResponse(list) = core.list_sessions("conn-a") else {
            panic!("expected session list");
        };
        assert!(list.sessions.is_empty(), "cleanup tombstone is not live");
        assert_eq!(host.closed_stdin, vec![String::from("proc-s1")]);
        assert_eq!(
            host.killed,
            vec![(String::from("proc-s1"), String::from("SIGTERM"))]
        );
        let prompt = AcpSessionRequest {
            session_id: String::from("s1"),
            method: String::from("session/prompt"),
            params: None,
        };
        assert_eq!(
            core.begin_session_request(&mut host, "conn-a", &prompt)
                .expect_err("cleanup-only session cannot start a resumable request")
                .code(),
            "invalid_state"
        );
        assert_eq!(
            core.session_request(&mut host, "conn-a", &prompt)
                .expect_err("cleanup-only session cannot run a blocking request")
                .code(),
            "invalid_state"
        );
        let config_error = core
            .prepare_session_config(
                "conn-a",
                &AcpSetSessionConfigRequest {
                    session_id: String::from("s1"),
                    category: String::from("model"),
                    value: String::from("test"),
                },
            )
            .err()
            .expect("cleanup-only session cannot mutate configuration");
        assert_eq!(config_error.code(), "invalid_state");
        assert_eq!(
            host.finalized.len(),
            1,
            "routing checks do not retry cleanup"
        );

        core.close_session(&mut host, "conn-a", &request)
            .expect("retry runs only host finalization");
        assert_eq!(core.session_count(), 0);
        assert_eq!(host.closed_stdin, vec![String::from("proc-s1")]);
        assert_eq!(host.killed.len(), 1, "retry must not repeat signal phase");
        assert_eq!(host.finalized.len(), 2);
    }

    #[test]
    fn owner_disposal_retries_a_retained_close_finalizer_without_aborting_process() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "owner-a"));
        let mut host = MockHost {
            finalization_failures_remaining: 1,
            ..MockHost::default()
        };
        core.close_session(
            &mut host,
            "owner-a",
            &AcpCloseSessionRequest {
                session_id: String::from("s1"),
            },
        )
        .expect_err("close retains the failed host finalizer");
        assert_eq!(core.pending_cleanup_count(), 1);

        host.finalization_failures_remaining = 1;
        let error = core
            .dispose_owner(&mut host, "owner-a")
            .expect_err("owner disposal preserves a still-failing finalizer");
        assert_eq!(error.code(), "cleanup_failed");
        assert_eq!(core.pending_cleanup_count(), 1);
        assert_eq!(host.killed.len(), 1, "disposal must not repeat signals");

        core.dispose_owner(&mut host, "owner-a")
            .expect("a later owner disposal retries the exact finalizer");
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(host.killed.len(), 1);
        assert_eq!(host.finalized.len(), 3);
    }

    #[test]
    fn owner_disposal_orders_abort_and_finalizer_actions_by_process_id() {
        let mut core = AcpCore::new();
        let mut session = record("s1", "owner-a");
        session.process_id = String::from("proc-a");
        core.insert_session(session);
        let mut host = MockHost {
            finalization_failures_remaining: 1,
            ..MockHost::default()
        };
        core.close_session(
            &mut host,
            "owner-a",
            &AcpCloseSessionRequest {
                session_id: String::from("s1"),
            },
        )
        .expect_err("retain finalizer");
        core.pending_cleanups
            .insert((String::from("owner-a"), String::from("proc-z")), None);
        host.cleanup_order.clear();
        host.finalization_failures_remaining = 1;

        core.dispose_owner(&mut host, "owner-a")
            .expect_err("injected finalizer still fails");
        assert_eq!(
            host.cleanup_order,
            vec![
                String::from("finalize:proc-a"),
                String::from("signal:proc-z:SIGKILL"),
            ]
        );
    }

    #[test]
    fn resumable_close_releases_core_between_bounded_signal_phases() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "owner-a"));
        core.insert_session(record("s2", "owner-b"));
        let mut host = MockHost::default();

        let response = core
            .begin_close_session(
                &mut host,
                "owner-a",
                &AcpCloseSessionRequest {
                    session_id: String::from("s1"),
                },
            )
            .expect("begin resumable close");
        let AcpResponse::AcpPendingResponse(first) = response else {
            panic!("live adapter close must be pending")
        };
        assert_eq!(first.timeout_ms, 5_000);
        assert_eq!(first.timeout_phase, "close.sigterm");
        assert_eq!(core.pending_close_count(), 1);
        assert_eq!(
            host.killed,
            vec![(String::from("proc-s1"), String::from("SIGTERM"))]
        );
        assert!(matches!(
            core.list_sessions("owner-b"),
            AcpResponse::AcpListSessionsResponse(ref sessions)
                if sessions.sessions.len() == 1 && sessions.sessions[0].session_id == "s2"
        ));

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: first.process_id.clone(),
                    reason: AcpPendingAbortReason::InteractionTimeout,
                    exit_code: None,
                },
            )
            .expect("SIGTERM timeout advances to SIGKILL phase");
        let AcpResponse::AcpPendingResponse(second) = response else {
            panic!("SIGKILL wait must remain pending")
        };
        assert_eq!(second.timeout_phase, "close.sigkill");
        assert_eq!(
            host.killed[1],
            (String::from("proc-s1"), String::from("SIGKILL"))
        );

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: second.process_id,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(0),
                },
            )
            .expect("exit completes close");
        assert!(matches!(response, AcpResponse::AcpSessionClosedResponse(_)));
        assert_eq!(core.pending_close_count(), 0);
        assert!(core.session("owner-a", "s1").is_none());
        assert!(core.session("owner-b", "s2").is_some());
    }

    /// Host whose adapter process is already gone: `wait_for_exit` would time
    /// out (returns `None`), so any wait the engine performs is dead time. The
    /// wait counter proves the short-circuit: a regression re-enters the
    /// SIGTERM → wait → SIGKILL → wait sequence and records 2 waits.
    #[derive(Default)]
    struct GoneAdapterHost {
        kill_error: Option<String>,
        killed: Vec<(String, String)>,
        waits: usize,
    }

    impl AcpHost for GoneAdapterHost {
        fn resolve_projected_agent(
            &mut self,
            id: &str,
        ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(Some(projected_agent(id)))
        }
        fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(vec![projected_agent("echo")])
        }
        fn spawn_agent(&mut self, _: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
            unreachable!("close_session does not spawn")
        }
        fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn write_stdin(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
            unreachable!()
        }
        fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
            Ok(None)
        }
        fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
            self.killed
                .push((process_id.to_string(), signal.to_string()));
            match &self.kill_error {
                Some(message) => Err(AcpCoreError::InvalidState(message.clone())),
                None => Ok(()),
            }
        }
        fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
            self.waits += 1;
            Ok(None) // the exit event was already drained; a wait can only time out
        }
        fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
            Ok(Vec::new())
        }
        fn now_ms(&self) -> u64 {
            0
        }
    }

    #[test]
    fn close_session_skips_teardown_wait_when_session_already_closed() {
        let mut core = AcpCore::new();
        let mut dead = record("s1", "conn-a");
        dead.closed = true;
        dead.exit_code = Some(137);
        core.insert_session(dead);
        let mut host = GoneAdapterHost::default();
        let resp = core
            .close_session(
                &mut host,
                "conn-a",
                &AcpCloseSessionRequest {
                    session_id: "s1".into(),
                },
            )
            .expect("close");
        assert!(matches!(resp, AcpResponse::AcpSessionClosedResponse(_)));
        assert_eq!(core.session_count(), 0);
        // Already-observed exit: no signals sent, no dead waiting.
        assert!(host.killed.is_empty());
        assert_eq!(host.waits, 0);
    }

    #[test]
    fn close_session_skips_teardown_wait_when_sigterm_reports_process_gone() {
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = GoneAdapterHost {
            kill_error: Some(String::from("process proc-s1: no such process (ESRCH)")),
            ..GoneAdapterHost::default()
        };
        let resp = core
            .close_session(
                &mut host,
                "conn-a",
                &AcpCloseSessionRequest {
                    session_id: "s1".into(),
                },
            )
            .expect("close");
        assert!(matches!(resp, AcpResponse::AcpSessionClosedResponse(_)));
        // SIGTERM was attempted, classified as process-gone, and both waits
        // (and the SIGKILL escalation) were skipped.
        assert_eq!(host.killed, vec![("proc-s1".into(), "SIGTERM".into())]);
        assert_eq!(host.waits, 0);
    }

    #[test]
    fn session_request_enforces_ownership_without_side_effects() {
        // A non-owner prompt fails closed with the same unknown-session error and
        // does NOT consume a request id or touch the victim's state.
        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = MockHost::default();
        let req = AcpSessionRequest {
            session_id: "s1".into(),
            method: "session/prompt".into(),
            params: None,
        };
        let err = core.session_request(&mut host, "conn-b", &req).unwrap_err();
        assert_eq!(err.code(), "invalid_state");
        assert!(err.to_string().contains("unknown ACP session"));
        // next_request_id untouched (no id consumed on the rejected attempt).
        assert_eq!(core.session("conn-a", "s1").unwrap().next_request_id, 1);
    }

    #[test]
    fn config_category_resolution_and_read_only_behavior_are_sidecar_owned() {
        let mut core = AcpCore::new();
        let mut session = record("s1", "conn-a");
        session.agent_type = String::from("opencode");
        session.config_options = vec![String::from(
            r#"{"id":"model-picker","category":"model","readOnly":true}"#,
        )];
        core.insert_session(session);

        let request = AcpSetSessionConfigRequest {
            session_id: String::from("s1"),
            category: String::from("model"),
            value: String::from("new-model"),
        };
        let PreparedSessionConfig::Immediate(AcpResponse::AcpSessionRpcResponse(response)) = core
            .prepare_session_config("conn-a", &request)
            .expect("prepare read-only response")
        else {
            panic!("read-only category must complete inside the sidecar");
        };
        let response: Value = serde_json::from_str(&response.response).expect("response JSON");
        assert_eq!(response["error"]["code"], -32601);
        assert!(response["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("before createSession()")));

        let writable = AcpSetSessionConfigRequest {
            session_id: String::from("s1"),
            category: String::from("thought_level"),
            value: String::from("high"),
        };
        let PreparedSessionConfig::Forward(request) = core
            .prepare_session_config("conn-a", &writable)
            .expect("prepare writable request")
        else {
            panic!("writable category must forward to the adapter");
        };
        assert_eq!(request.method, "session/set_config_option");
        let params: Value =
            serde_json::from_str(request.params.as_deref().expect("params")).expect("params JSON");
        assert_eq!(params["configId"], "thought_level");
        assert_eq!(params["value"], "high");
    }

    #[test]
    fn session_request_round_trips_a_prompt_through_the_agent() {
        use serde_json::Value;
        use std::collections::VecDeque;

        // A host whose agent answers session/prompt with a stopReason, echoing the
        // rpc id the core allocated.
        #[derive(Default)]
        struct PromptHost {
            out: VecDeque<AgentOutput>,
            clock: u64,
            last_request: Option<Value>,
        }
        impl AcpHost for PromptHost {
            fn resolve_projected_agent(
                &mut self,
                id: &str,
            ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(Some(projected_agent(id)))
            }
            fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(vec![projected_agent("echo")])
            }
            fn spawn_agent(&mut self, _: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
                unreachable!()
            }
            fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
                let request: Value =
                    serde_json::from_slice(chunk.strip_suffix(b"\n").unwrap_or(chunk)).unwrap();
                let id = request["id"].as_i64().unwrap();
                self.last_request = Some(request);
                let notification = json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": { "text": "sidecar text" }
                        }
                    }
                });
                let mut notification_bytes = serde_json::to_vec(&notification).unwrap();
                notification_bytes.push(b'\n');
                self.out.push_back(AgentOutput::Stdout(notification_bytes));
                let reply = json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}});
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                self.out.push_back(AgentOutput::Stdout(bytes));
                Ok(())
            }
            fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
                self.clock += 1;
                Ok(self.out.pop_front())
            }
            fn kill_agent(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
                Ok(Some(0))
            }
            fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
                Ok(Vec::new())
            }
            fn now_ms(&self) -> u64 {
                self.clock
            }
        }

        let mut core = AcpCore::new();
        core.insert_session(record("s1", "conn-a"));
        let mut host = PromptHost::default();
        let req = AcpSessionRequest {
            session_id: "s1".into(),
            method: "session/prompt".into(),
            params: Some(r#"{"prompt":[{"type":"text","text":"hi"}]}"#.into()),
        };

        let response = core
            .session_request(&mut host, "conn-a", &req)
            .expect("prompt round-trip");
        match response {
            AcpResponse::AcpSessionRpcResponse(rpc) => {
                assert_eq!(rpc.session_id, "s1");
                let body: Value = serde_json::from_str(&rpc.response).unwrap();
                assert_eq!(body["result"]["stopReason"], json!("end_turn"));
                assert_eq!(rpc.text.as_deref(), Some("sidecar text"));
            }
            other => panic!("expected rpc response, got {other:?}"),
        }
        // The core injected sessionId into the outbound params and consumed an id.
        let sent = host.last_request.unwrap();
        assert_eq!(sent["params"]["sessionId"], json!("s1"));
        assert_eq!(core.session("conn-a", "s1").unwrap().next_request_id, 2);
    }

    #[test]
    fn resume_falls_back_to_session_new_when_no_native_capability() {
        use agentos_protocol::generated::v1::AcpResumeSessionRequest;
        use serde_json::Value;
        use std::collections::{HashMap, VecDeque};

        // An agent advertising NO loadSession/resume cap: resume must take Tier 2
        // (session/new) and arm the transcript preamble.
        #[derive(Default)]
        struct ResumeHost {
            out: VecDeque<AgentOutput>,
            clock: u64,
        }
        impl AcpHost for ResumeHost {
            fn resolve_projected_agent(
                &mut self,
                id: &str,
            ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(Some(projected_agent(id)))
            }
            fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(vec![projected_agent("echo")])
            }
            fn spawn_agent(
                &mut self,
                request: SpawnAgentRequest,
            ) -> Result<SpawnedAgent, AcpCoreError> {
                Ok(SpawnedAgent {
                    process_id: request.process_id,
                    pid: Some(9),
                })
            }
            fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
                let request: Value =
                    serde_json::from_slice(chunk.strip_suffix(b"\n").unwrap_or(chunk)).unwrap();
                let id = request["id"].as_i64().unwrap();
                let reply = match request["method"].as_str().unwrap() {
                    // No agentCapabilities -> native_resume_method returns None.
                    "initialize" => json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1}}),
                    "session/new" => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"live-1"}})
                    }
                    other => panic!("unexpected method {other}"),
                };
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                self.out.push_back(AgentOutput::Stdout(bytes));
                Ok(())
            }
            fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
                self.clock += 1;
                Ok(self.out.pop_front())
            }
            fn kill_agent(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
                Ok(Some(0))
            }
            fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
                Ok(Vec::new())
            }
            fn now_ms(&self) -> u64 {
                self.clock
            }
        }

        let mut core = AcpCore::new();
        let mut host = ResumeHost::default();
        let request = AcpResumeSessionRequest {
            session_id: "old-session".into(),
            agent_type: "echo".into(),
            transcript_path: Some("/transcripts/old.jsonl".into()),
            cwd: Some("/workspace".into()),
            env: Some(HashMap::new()),
        };

        let response = core
            .resume_session(&mut host, "conn-a", &request)
            .expect("resume");
        match response {
            AcpResponse::AcpSessionResumedResponse(resumed) => {
                assert_eq!(resumed.session_id, "live-1");
                assert_eq!(resumed.mode, "fallback");
                assert_eq!(resumed.agent_type, "echo");
                assert_eq!(resumed.process_id, "acp-agent-0");
                assert_eq!(resumed.pid, Some(9));
            }
            other => panic!("expected resumed response, got {other:?}"),
        }
        // The fallback armed the transcript-continuation preamble for the next prompt.
        let preamble = core
            .session("conn-a", "live-1")
            .unwrap()
            .pending_preamble
            .clone()
            .expect("preamble armed");
        assert!(preamble.contains("/transcripts/old.jsonl"));
    }

    #[test]
    fn create_session_runs_the_acp_handshake_round_trip() {
        use agentos_protocol::generated::v1::{AcpCreateSessionRequest, AcpRuntimeKind};
        use serde_json::{json, Value};
        use std::collections::{HashMap, VecDeque};

        // A mock that spawns and answers the ACP handshake (initialize + session/new)
        // like a minimal ACP echo agent.
        #[derive(Default)]
        struct CreateHost {
            out: VecDeque<AgentOutput>,
            clock: u64,
            bound: Vec<(String, String)>,
        }
        impl AcpHost for CreateHost {
            fn resolve_projected_agent(
                &mut self,
                id: &str,
            ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(Some(projected_agent(id)))
            }
            fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
                Ok(vec![projected_agent("echo")])
            }
            fn spawn_agent(
                &mut self,
                request: SpawnAgentRequest,
            ) -> Result<SpawnedAgent, AcpCoreError> {
                Ok(SpawnedAgent {
                    process_id: request.process_id,
                    pid: Some(7),
                })
            }
            fn bind_session(
                &mut self,
                session_id: &str,
                process_id: &str,
            ) -> Result<(), AcpCoreError> {
                self.bound.push((session_id.into(), process_id.into()));
                Ok(())
            }
            fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
                let request: Value =
                    serde_json::from_slice(chunk.strip_suffix(b"\n").unwrap_or(chunk)).unwrap();
                let id = request["id"].as_i64().unwrap();
                let reply = match request["method"].as_str().unwrap() {
                    "initialize" => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1,"agentInfo":{"name":"echo"}}})
                    }
                    "session/new" => {
                        json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"sess-xyz"}})
                    }
                    other => panic!("unexpected method {other}"),
                };
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                self.out.push_back(AgentOutput::Stdout(bytes));
                Ok(())
            }
            fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
                self.clock += 1;
                Ok(self.out.pop_front())
            }
            fn kill_agent(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
                Ok(Some(0))
            }
            fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
                Ok(())
            }
            fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
                Ok(Vec::new())
            }
            fn now_ms(&self) -> u64 {
                self.clock
            }
        }

        let mut core = AcpCore::new();
        let mut host = CreateHost::default();
        let request = AcpCreateSessionRequest {
            agent_type: "echo".into(),
            runtime: Some(AcpRuntimeKind::JavaScript),
            protocol_version: Some(1),
            cwd: Some("/workspace".into()),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            client_capabilities: Some("{}".into()),
            mcp_servers: Some("[]".into()),
            additional_instructions: None,
            skip_os_instructions: Some(false),
        };

        let response = core
            .create_session(&mut host, "conn-a", &request)
            .expect("create session");
        match response {
            AcpResponse::AcpSessionCreatedResponse(created) => {
                assert_eq!(created.session_id, "sess-xyz");
                assert_eq!(created.agent_type, "echo");
                assert_eq!(created.process_id, "acp-agent-0");
                assert_eq!(created.pid, Some(7));
            }
            other => panic!("expected created response, got {other:?}"),
        }
        assert_eq!(core.session_count(), 1);
        assert_eq!(host.bound, vec![("sess-xyz".into(), "acp-agent-0".into())]);

        // The freshly-created session is readable by its owner (full round-trip:
        // create -> state).
        let state = core
            .get_session_state(
                "conn-a",
                &AcpGetSessionStateRequest {
                    session_id: "sess-xyz".into(),
                },
            )
            .expect("state");
        assert!(matches!(state, AcpResponse::AcpSessionStateResponse(_)));

        let collision = core
            .create_session(&mut host, "conn-a", &request)
            .expect_err("duplicate adapter session id must be rejected by the sidecar");
        assert!(collision
            .to_string()
            .contains("session id collision: sess-xyz"));
        assert_eq!(core.session_count(), 1);
        assert_eq!(host.bound, vec![("sess-xyz".into(), "acp-agent-0".into())]);
    }

    // ---- resumable (browser, non-blocking) create_session ----

    use agentos_protocol::generated::v1::{AcpCreateSessionRequest, AcpRuntimeKind};
    use std::collections::HashMap;

    /// A host that records stdin writes but NEVER produces output on its own — the
    /// resumable path is driven entirely by feed_agent_output, so poll_output must
    /// never be called (it would block forever in the blocking path).
    #[derive(Default)]
    struct ResumableMockHost {
        stdin: Vec<String>,
        bound: Vec<(String, String)>,
        killed: Vec<(String, String)>,
        abort_error: Option<String>,
        close_stdin_error: Option<String>,
    }
    impl AcpHost for ResumableMockHost {
        fn resolve_projected_agent(
            &mut self,
            id: &str,
        ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(Some(projected_agent(id)))
        }
        fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(vec![projected_agent("echo")])
        }
        fn spawn_agent(
            &mut self,
            request: SpawnAgentRequest,
        ) -> Result<SpawnedAgent, AcpCoreError> {
            Ok(SpawnedAgent {
                process_id: request.process_id,
                pid: Some(11),
            })
        }
        fn bind_session(&mut self, session_id: &str, process_id: &str) -> Result<(), AcpCoreError> {
            self.bound.push((session_id.into(), process_id.into()));
            Ok(())
        }
        fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
            self.stdin
                .push(String::from_utf8_lossy(chunk).trim().to_string());
            Ok(())
        }
        fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
            match self.close_stdin_error.take() {
                Some(message) => Err(AcpCoreError::InvalidState(message)),
                None => Ok(()),
            }
        }
        fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
            unreachable!("resumable path must not poll_output (it never blocks)")
        }
        fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
            self.killed
                .push((process_id.to_string(), signal.to_string()));
            Ok(())
        }
        fn abort_agent(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
            self.kill_agent(process_id, "SIGKILL")?;
            match self.abort_error.take() {
                Some(message) => Err(AcpCoreError::Execution(message)),
                None => Ok(()),
            }
        }
        fn wait_for_exit(&mut self, _: &str, _: u64) -> Result<Option<i32>, AcpCoreError> {
            Ok(Some(0))
        }
        fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn read_file(&mut self, _: &str) -> Result<Vec<u8>, AcpCoreError> {
            Ok(Vec::new())
        }
        fn now_ms(&self) -> u64 {
            0
        }
    }

    fn echo_create_request() -> AcpCreateSessionRequest {
        AcpCreateSessionRequest {
            agent_type: "echo".into(),
            runtime: Some(AcpRuntimeKind::JavaScript),
            protocol_version: Some(1),
            cwd: Some("/workspace".into()),
            args: Some(Vec::new()),
            env: Some(HashMap::new()),
            client_capabilities: Some("{}".into()),
            mcp_servers: Some("[]".into()),
            additional_instructions: None,
            skip_os_instructions: Some(false),
        }
    }

    fn echo_resume_request(
        session_id: &str,
        transcript_path: Option<&str>,
    ) -> AcpResumeSessionRequest {
        AcpResumeSessionRequest {
            session_id: session_id.into(),
            agent_type: "echo".into(),
            transcript_path: transcript_path.map(str::to_string),
            cwd: Some("/workspace".into()),
            env: Some(HashMap::new()),
        }
    }

    #[test]
    fn resumable_create_session_drives_the_handshake_without_blocking() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();

        // begin: spawns + writes initialize, returns immediately, no session yet.
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin");
        assert_eq!(core.session_count(), 0);
        assert_eq!(core.pending_create_count(), 1);
        assert!(host.stdin[0].contains("\"method\":\"initialize\""));

        // feed the initialize response → still pending, session/new now written.
        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"echo"}}}
"#,
            )
            .expect("feed initialize");
        assert!(matches!(step, ResumeStep::Pending));
        assert!(host.stdin[1].contains("\"method\":\"session/new\""));
        assert_eq!(core.session_count(), 0);

        // feed the session/new response → Created.
        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-xyz"}}
"#,
            )
            .expect("feed session/new");
        match step {
            ResumeStep::Done(AcpResponse::AcpSessionCreatedResponse(created)) => {
                assert_eq!(created.session_id, "sess-xyz");
                assert_eq!(created.agent_type, "echo");
                assert_eq!(created.process_id, "acp-agent-0");
                assert_eq!(created.pid, Some(11));
            }
            other => panic!("expected Created, got {other:?}"),
        }
        assert_eq!(core.session_count(), 1);
        assert_eq!(core.pending_create_count(), 0);
        assert_eq!(host.bound, vec![("sess-xyz".into(), "acp-agent-0".into())]);

        // The created session is queryable by its owner (full resumable round-trip).
        let state = core
            .get_session_state(
                "conn-a",
                &AcpGetSessionStateRequest {
                    session_id: "sess-xyz".into(),
                },
            )
            .expect("state");
        assert!(matches!(state, AcpResponse::AcpSessionStateResponse(_)));
    }

    #[test]
    fn resumable_create_session_buffers_partial_lines() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin");

        // Deliver the initialize response in three chunks split mid-line.
        let init = br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#;
        let (a, rest) = init.split_at(10);
        let (b, c) = rest.split_at(20);
        assert!(matches!(
            core.feed_agent_output(&mut host, "conn-a", &process_id, a)
                .expect("a"),
            ResumeStep::Pending
        ));
        assert_eq!(host.stdin.len(), 1, "no full line yet → no session/new");
        assert!(matches!(
            core.feed_agent_output(&mut host, "conn-a", &process_id, b)
                .expect("b"),
            ResumeStep::Pending
        ));
        assert!(matches!(
            core.feed_agent_output(&mut host, "conn-a", &process_id, c)
                .expect("c"),
            ResumeStep::Pending
        ));
        // Only once the newline arrives is the line parsed and session/new written.
        assert!(host.stdin[1].contains("session/new"));

        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"chunked-1"}}
"#,
            )
            .expect("feed session/new");
        match step {
            ResumeStep::Done(AcpResponse::AcpSessionCreatedResponse(c)) => {
                assert_eq!(c.session_id, "chunked-1")
            }
            other => panic!("expected Created, got {other:?}"),
        }
    }

    #[test]
    fn resumable_inbound_request_stays_pending_and_bypasses_event_capacity() {
        let mut core = AcpCore::with_pending_event_limit(0);
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin");

        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":"host-1","method":"host/read","params":{}}
"#,
            )
            .expect("inbound request is not a notification overflow");
        assert!(matches!(step, ResumeStep::Pending));
        assert_eq!(core.pending_create_count(), 1);
        assert_eq!(core.pending_event_count(), 0);
        assert!(core.take_events("conn-a").is_empty());
        assert_eq!(host.stdin.len(), 2);
        let response: Value = serde_json::from_str(&host.stdin[1]).expect("host response JSON");
        assert_eq!(response["id"], "host-1");
        assert_eq!(response["error"]["code"], -32601);

        core.feed_agent_output(
            &mut host,
            "conn-a",
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        )
        .expect("initialize after inbound request");
        let completed = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"inbound-sess"}}
"#,
            )
            .expect("session/new after inbound request");
        assert!(matches!(
            completed,
            ResumeStep::Done(AcpResponse::AcpSessionCreatedResponse(_))
        ));
        assert_eq!(core.pending_create_count(), 0);
        assert_eq!(core.pending_event_count(), 0);
    }

    #[test]
    fn resumable_create_session_propagates_an_agent_initialize_error() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin");
        let err = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"boom"}}
"#,
            )
            .expect_err("initialize error must surface");
        assert_eq!(err.code(), "execution");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn resumable_native_resume_never_polls_and_forwards_bootstrap_notifications() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let pending = core
            .dispatch_resumable(
                &mut host,
                "conn-a",
                AcpRequest::AcpResumeSessionRequest(echo_resume_request("durable-1", None)),
            )
            .expect("begin resumable resume");
        let AcpResponse::AcpPendingResponse(pending) = pending else {
            panic!("resume must begin pending");
        };
        assert_eq!(core.pending_resume_count(), 1);
        assert!(host.stdin[0].contains("\"method\":\"initialize\""));

        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &pending.process_id,
                br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"available_commands_update"}}}
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}
"#,
            )
            .expect("initialize response");
        assert!(matches!(step, ResumeStep::Pending));
        assert!(host.stdin[1].contains("\"method\":\"session/load\""));
        assert!(host.stdin[1].contains("\"sessionId\":\"durable-1\""));

        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &pending.process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{"modes":{"currentModeId":"plan"}}}
"#,
            )
            .expect("native load response");
        match step {
            ResumeStep::Done(AcpResponse::AcpSessionResumedResponse(resumed)) => {
                assert_eq!(resumed.session_id, "durable-1");
                assert_eq!(resumed.mode, "native");
            }
            other => panic!("expected native resumed response, got {other:?}"),
        }
        assert_eq!(core.pending_resume_count(), 0);
        assert_eq!(core.session_count(), 1);
        assert_eq!(core.take_events("conn-a").len(), 1);
        assert!(host.killed.is_empty());
    }

    #[test]
    fn resumable_unknown_native_session_falls_back_without_polling() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_resume_session(
                &mut host,
                "conn-a",
                &echo_resume_request("durable-missing", Some("/history/session.jsonl")),
            )
            .expect("begin resumable resume");
        core.feed_agent_output(
            &mut host,
            "conn-a",
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}
"#,
        )
        .expect("initialize response");
        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"missing","data":{"details":"NotFoundError"}}}
"#,
            )
            .expect("unknown session fallback");
        assert!(matches!(step, ResumeStep::Pending));
        assert!(host.stdin[2].contains("\"method\":\"session/new\""));

        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fresh-1"}}
"#,
            )
            .expect("fallback session/new");
        assert!(matches!(
            step,
            ResumeStep::Done(AcpResponse::AcpSessionResumedResponse(
                AcpSessionResumedResponse { ref session_id, ref mode, .. }
            )) if session_id == "fresh-1" && mode == "fallback"
        ));
        assert_eq!(core.pending_resume_count(), 0);

        host.stdin.clear();
        core.begin_session_request(
            &mut host,
            "conn-a",
            &AcpSessionRequest {
                session_id: "fresh-1".into(),
                method: "session/prompt".into(),
                params: Some(r#"{"prompt":[{"type":"text","text":"continue"}]}"#.into()),
            },
        )
        .expect("first prompt");
        assert!(host.stdin[0].contains("/history/session.jsonl"));
    }

    #[test]
    fn resumable_resume_terminal_parse_error_clears_state_and_kills_agent() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_resume_session(&mut host, "conn-a", &echo_resume_request("durable-1", None))
            .expect("begin resumable resume");
        let error = core
            .feed_agent_output(&mut host, "conn-a", &process_id, b"not-json\n")
            .expect_err("malformed adapter output must fail closed");
        assert!(error.to_string().contains("invalid JSON-RPC"));
        assert_eq!(core.pending_resume_count(), 0);
        assert_eq!(core.session_count(), 0);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_session_prompt_drives_a_prompt_without_blocking() {
        use agentos_protocol::generated::v1::AcpSessionRequest;
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();

        // Bring a session live via the resumable create path.
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin create");
        core.feed_agent_output(
            &mut host,
            "conn-a",
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        )
        .expect("init");
        core.feed_agent_output(
            &mut host,
            "conn-a",
            &process_id,
            br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-p"}}
"#,
        )
        .expect("session/new");
        assert_eq!(core.session_count(), 1);
        host.stdin.clear();

        // begin a resumable prompt: writes the prompt, returns immediately.
        let prompt = AcpSessionRequest {
            session_id: "sess-p".into(),
            method: "session/prompt".into(),
            params: Some(r#"{"prompt":[{"type":"text","text":"hi"}]}"#.into()),
        };
        let prompt_process = core
            .begin_session_request(&mut host, "conn-a", &prompt)
            .expect("begin prompt");
        assert_eq!(prompt_process, process_id);
        assert_eq!(core.pending_prompt_count(), 1);
        assert!(matches!(
            core.pending_response(process_id.clone())
                .expect("pending prompt response"),
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                timeout_ms: 600_000,
                ..
            })
        ));
        // sessionId injected; rpc id is 3 (next after the create handshake's 1,2).
        assert!(host.stdin[0].contains("\"method\":\"session/prompt\""));
        assert!(host.stdin[0].contains("\"sessionId\":\"sess-p\""));
        assert!(host.stdin[0].contains("\"id\":3"));

        assert!(matches!(
            core.feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"browser text"}}}}
"#,
            )
            .expect("feed prompt notification"),
            ResumeStep::Pending
        ));

        // feed the prompt response → Done with the agent's reply and text.
        let step = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}
"#,
            )
            .expect("feed prompt response");
        match step {
            ResumeStep::Done(AcpResponse::AcpSessionRpcResponse(rpc)) => {
                assert_eq!(rpc.session_id, "sess-p");
                let body: Value = serde_json::from_str(&rpc.response).unwrap();
                assert_eq!(body["result"]["stopReason"], json!("end_turn"));
                assert_eq!(rpc.text.as_deref(), Some("browser text"));
            }
            other => panic!("expected rpc Done, got {other:?}"),
        }
        assert_eq!(core.pending_prompt_count(), 0);
    }

    fn create_restartable_resumable_session(
        core: &mut AcpCore,
        host: &mut ResumableMockHost,
        session_id: &str,
    ) -> String {
        let process_id = core
            .begin_create_session(host, "owner-a", &echo_create_request())
            .expect("begin restartable session");
        core.feed_agent_output(
            host,
            "owner-a",
            &process_id,
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#,
        )
        .expect("initialize restartable session");
        core.feed_agent_output(
            host,
            "owner-a",
            &process_id,
            format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"sessionId\":\"{session_id}\"}}}}\n"
            )
            .as_bytes(),
        )
        .expect("create restartable session");
        process_id
    }

    fn begin_crashing_prompt(
        core: &mut AcpCore,
        host: &mut ResumableMockHost,
        session_id: &str,
    ) -> String {
        core.begin_session_request(
            host,
            "owner-a",
            &AcpSessionRequest {
                session_id: session_id.to_string(),
                method: String::from("session/prompt"),
                params: Some(String::from(r#"{"prompt":[]}"#)),
            },
        )
        .expect("begin crashing prompt")
    }

    #[test]
    fn resumable_agent_exit_restarts_rebinds_and_allows_retry() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "restartable");
        assert!(core
            .session("owner-a", "restartable")
            .unwrap()
            .restart
            .is_some());
        begin_crashing_prompt(&mut core, &mut host, "restartable");

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process.clone(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(137),
                },
            )
            .expect("sidecar starts the replacement adapter");
        let AcpResponse::AcpPendingResponse(restart_pending) = response else {
            panic!("agent exit must return a restart continuation");
        };
        assert_ne!(restart_pending.process_id, dead_process);
        assert_eq!(restart_pending.timeout_phase, "restart.initialize");
        assert_eq!(core.pending_restart_count(), 1);

        assert!(matches!(
            core.feed_agent_output(
                &mut host,
                "owner-a",
                &restart_pending.process_id,
                br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}
"#,
            )
            .expect("replacement initialize"),
            ResumeStep::Pending
        ));
        let completed = core
            .feed_agent_output(
                &mut host,
                "owner-a",
                &restart_pending.process_id,
                br#"{"jsonrpc":"2.0","id":2,"result":{}}
"#,
            )
            .expect("replacement session/load");
        assert!(matches!(
            completed,
            ResumeStep::Done(AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message }))
                if code == "invalid_state" && message.contains("auto-restarted") && message.contains("retry")
        ));
        assert_eq!(core.pending_restart_count(), 0);
        assert_eq!(
            core.session("owner-a", "restartable").unwrap().process_id,
            restart_pending.process_id
        );
        assert!(!core.session("owner-a", "restartable").unwrap().closed);
        assert_eq!(
            core.session("owner-a", "restartable")
                .unwrap()
                .restart
                .as_ref()
                .unwrap()
                .count,
            1
        );
        assert_eq!(
            host.bound.last(),
            Some(&(
                String::from("restartable"),
                restart_pending.process_id.clone()
            ))
        );
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent {
                restart,
                restart_count: 1,
                exit_code: Some(137),
                ..
            })] if restart == "restarted"
        ));

        let retried_process = begin_crashing_prompt(&mut core, &mut host, "restartable");
        assert_eq!(retried_process, restart_pending.process_id);
    }

    #[test]
    fn exited_prompt_cleanup_is_non_routable_and_retried_before_restart() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "cleanup-restart");
        begin_crashing_prompt(&mut core, &mut host, "cleanup-restart");
        host.abort_error = Some(String::from("injected abort failure"));
        let abort = AcpAbortPendingRequest {
            process_id: dead_process.clone(),
            reason: AcpPendingAbortReason::AgentExited,
            exit_code: Some(137),
        };

        let error = core
            .abort_pending(&mut host, "owner-a", &abort)
            .expect_err("restart waits for cleanup");
        assert_eq!(error.code(), "cleanup_failed");
        assert_eq!(core.pending_prompt_count(), 1);
        let route_error = core
            .feed_agent_output(&mut host, "owner-a", &dead_process, b"{}\n")
            .expect_err("cleanup-only prompt cannot receive adapter output");
        assert_eq!(route_error.code(), "invalid_state");
        assert_eq!(core.pending_prompt_count(), 1);
        assert_eq!(
            core.interrupt_pending_prompt("owner-a", &dead_process)
                .expect_err("cleanup-only prompt cannot be interrupted")
                .code(),
            "invalid_state"
        );
        assert_eq!(
            core.abandon_pending_prompt("owner-a", &dead_process)
                .expect_err("cleanup-only prompt cannot be abandoned")
                .code(),
            "invalid_state"
        );
        assert_eq!(core.pending_prompt_count(), 1);

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    reason: AcpPendingAbortReason::CallerCancelled,
                    ..abort
                },
            )
            .expect("exact retry cleans up then begins restart");
        assert!(matches!(response, AcpResponse::AcpPendingResponse(_)));
        assert_eq!(core.pending_prompt_count(), 0);
        assert_eq!(core.pending_restart_count(), 1);
        assert_eq!(host.killed.len(), 2);
    }

    #[test]
    fn resumable_restart_unsupported_evicts_with_shared_outcome() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "unsupported");
        begin_crashing_prompt(&mut core, &mut host, "unsupported");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: None,
                },
            )
            .expect("start restart")
        else {
            panic!("expected restart continuation");
        };
        host.abort_error = Some(String::from("injected unsupported cleanup failure"));
        let cleanup_error = core
            .feed_agent_output(
                &mut host,
                "owner-a",
                &restart_pending.process_id,
                br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentCapabilities":{}}}
"#,
            )
            .expect_err("cleanup failure delays unsupported outcome");
        assert_eq!(cleanup_error.code(), "cleanup_failed");
        assert!(core.take_events("owner-a").is_empty());
        let completed = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: restart_pending.process_id,
                    reason: AcpPendingAbortReason::DriverFailed,
                    exit_code: None,
                },
            )
            .expect("cleanup retry commits unsupported outcome");
        assert!(matches!(
            completed,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref message, .. })
                if message.contains("auto-restart unsupported") && message.contains("session evicted")
        ));
        assert_eq!(core.session_count(), 0);
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent { restart, .. })]
                if restart == "unsupported"
        ));
    }

    #[test]
    fn resumable_restart_budget_exhaustion_evicts_without_spawning() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process = create_restartable_resumable_session(&mut core, &mut host, "exhausted");
        core.session_mut("owner-a", "exhausted")
            .unwrap()
            .restart
            .as_mut()
            .expect("restart state")
            .count = MAX_ADAPTER_RESTARTS;
        begin_crashing_prompt(&mut core, &mut host, "exhausted");
        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: None,
                },
            )
            .expect("exhaustion is a typed ACP response");
        assert!(matches!(
            response,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref message, .. })
                if message.contains("restart budget exhausted") && message.contains("session evicted")
        ));
        assert_eq!(core.session_count(), 0);
        assert_eq!(core.pending_restart_count(), 0);
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent { restart, restart_count: MAX_ADAPTER_RESTARTS, .. })]
                if restart == "exhausted"
        ));
    }

    #[test]
    fn resumable_restart_malformed_output_emits_failed_and_clears_state() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "malformed-restart");
        begin_crashing_prompt(&mut core, &mut host, "malformed-restart");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process.clone(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(9),
                },
            )
            .expect("begin replacement")
        else {
            panic!("replacement must begin pending");
        };

        host.abort_error = Some(String::from("injected malformed restart cleanup failure"));
        let cleanup_error = core
            .feed_agent_output(
                &mut host,
                "owner-a",
                &restart_pending.process_id,
                b"not-json\n",
            )
            .expect_err("cleanup failure delays the terminal restart outcome");
        assert_eq!(cleanup_error.code(), "cleanup_failed");
        assert_eq!(core.pending_cleanup_count(), 1);
        assert!(core.take_events("owner-a").is_empty());

        let completed = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: restart_pending.process_id.clone(),
                    reason: AcpPendingAbortReason::CallerCancelled,
                    exit_code: None,
                },
            )
            .expect("cleanup retry commits the canonical restart outcome");
        assert!(matches!(
            completed,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message })
                if code == "invalid_state" && message.contains("auto-restart failed") && message.contains("invalid JSON-RPC")
        ));
        assert_eq!(core.pending_restart_count(), 0);
        assert_eq!(core.session_count(), 0);
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent {
                process_id,
                exit_code: Some(9),
                restart,
                ..
            })] if process_id == &dead_process && restart == "failed"
        ));
        assert_eq!(
            host.killed,
            vec![
                (dead_process, String::from("SIGKILL")),
                (restart_pending.process_id.clone(), String::from("SIGKILL")),
                (restart_pending.process_id, String::from("SIGKILL")),
            ]
        );
    }

    #[test]
    fn close_finds_replacement_cleanup_by_session_after_session_record_is_removed() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "close-replacement");
        begin_crashing_prompt(&mut core, &mut host, "close-replacement");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(9),
                },
            )
            .expect("begin replacement")
        else {
            panic!("replacement must begin pending");
        };
        host.abort_error = Some(String::from("injected replacement cleanup failure"));
        core.feed_agent_output(
            &mut host,
            "owner-a",
            &restart_pending.process_id,
            b"not-json\n",
        )
        .expect_err("replacement cleanup is retained");
        assert_eq!(
            core.session_count(),
            0,
            "restart failure removed the live session"
        );
        assert_eq!(core.pending_cleanup_count(), 1);

        let response = core
            .close_session(
                &mut host,
                "owner-a",
                &AcpCloseSessionRequest {
                    session_id: String::from("close-replacement"),
                },
            )
            .expect("close drives the replacement cleanup tombstone");
        assert!(matches!(response, AcpResponse::AcpSessionClosedResponse(_)));
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(
            host.killed.last(),
            Some(&(restart_pending.process_id, String::from("SIGKILL")))
        );
        assert!(host.killed.iter().all(|(_, signal)| signal == "SIGKILL"));
    }

    #[test]
    fn restart_completion_retries_event_commit_without_repeating_host_abort() {
        let mut core = AcpCore::with_pending_event_limit(1);
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "event-backpressure");
        begin_crashing_prompt(&mut core, &mut host, "event-backpressure");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process.clone(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(9),
                },
            )
            .expect("begin replacement")
        else {
            panic!("replacement must begin pending");
        };
        host.abort_error = Some(String::from("injected replacement cleanup failure"));
        core.feed_agent_output(
            &mut host,
            "owner-a",
            &restart_pending.process_id,
            b"not-json\n",
        )
        .expect_err("replacement cleanup is retained");
        core.pending_events
            .push(pending_session_event("owner-a", "sibling"));

        let retry = AcpAbortPendingRequest {
            process_id: restart_pending.process_id.clone(),
            reason: AcpPendingAbortReason::CallerCancelled,
            exit_code: None,
        };
        let error = core
            .abort_pending(&mut host, "owner-a", &retry)
            .expect_err("event capacity delays canonical restart completion");
        assert_eq!(error.code(), "limit_exceeded");
        assert_eq!(core.pending_cleanup_count(), 1);
        let abort_count = host
            .killed
            .iter()
            .filter(|(process_id, _)| process_id == &restart_pending.process_id)
            .count();
        assert_eq!(abort_count, 2, "failed abort plus one successful retry");

        assert_eq!(core.take_events("owner-a").len(), 1);
        let completed = core
            .abort_pending(&mut host, "owner-a", &retry)
            .expect("draining capacity commits the retained completion");
        assert!(matches!(
            completed,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, .. }) if code == "invalid_state"
        ));
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(
            host.killed
                .iter()
                .filter(|(process_id, _)| process_id == &restart_pending.process_id)
                .count(),
            abort_count,
            "event retry must not repeat the committed host abort"
        );
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent { process_id, .. })]
                if process_id == &dead_process
        ));
    }

    #[test]
    fn resumable_replacement_exit_emits_failed_and_clears_state() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "replacement-exit");
        begin_crashing_prompt(&mut core, &mut host, "replacement-exit");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process.clone(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(9),
                },
            )
            .expect("begin replacement")
        else {
            panic!("replacement must begin pending");
        };

        host.abort_error = Some(String::from("injected replacement cleanup failure"));
        let replacement_abort = AcpAbortPendingRequest {
            process_id: restart_pending.process_id.clone(),
            reason: AcpPendingAbortReason::AgentExited,
            exit_code: Some(42),
        };
        let cleanup_error = core
            .abort_pending(&mut host, "owner-a", &replacement_abort)
            .expect_err("replacement exit cleanup failure remains retryable");
        assert_eq!(cleanup_error.code(), "cleanup_failed");
        assert_eq!(core.pending_restart_count(), 1);
        assert_eq!(
            core.feed_agent_output(&mut host, "owner-a", &restart_pending.process_id, b"{}\n",)
                .expect_err("cleanup-only replacement route rejects output")
                .code(),
            "invalid_state"
        );

        let completed = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    reason: AcpPendingAbortReason::DriverFailed,
                    ..replacement_abort
                },
            )
            .expect("replacement exit is a deterministic terminal response");
        assert!(matches!(
            completed,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message })
                if code == "invalid_state" && message.contains("auto-restart failed") && message.contains("replacement ACP adapter")
        ));
        assert_eq!(core.pending_restart_count(), 0);
        assert_eq!(core.session_count(), 0);
        assert!(matches!(
            core.take_events("owner-a").as_slice(),
            [AcpEvent::AcpAgentExitedEvent(AcpAgentExitedEvent {
                process_id,
                exit_code: Some(9),
                restart,
                ..
            })] if process_id == &dead_process && restart == "failed"
        ));
        assert_eq!(
            host.killed,
            vec![
                (dead_process, String::from("SIGKILL")),
                (restart_pending.process_id.clone(), String::from("SIGKILL")),
                (restart_pending.process_id, String::from("SIGKILL")),
            ]
        );
    }

    #[test]
    fn close_drives_cleanup_only_inflight_replacement_by_session_id() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let dead_process =
            create_restartable_resumable_session(&mut core, &mut host, "close-live-replacement");
        begin_crashing_prompt(&mut core, &mut host, "close-live-replacement");
        let AcpResponse::AcpPendingResponse(restart_pending) = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: dead_process,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: Some(9),
                },
            )
            .expect("begin replacement")
        else {
            panic!("replacement must begin pending");
        };
        host.abort_error = Some(String::from("injected replacement exit abort failure"));
        core.abort_pending(
            &mut host,
            "owner-a",
            &AcpAbortPendingRequest {
                process_id: restart_pending.process_id.clone(),
                reason: AcpPendingAbortReason::AgentExited,
                exit_code: Some(42),
            },
        )
        .expect_err("replacement route becomes cleanup-only");
        assert_eq!(core.pending_cleanup_count(), 1);

        let response = core
            .close_session(
                &mut host,
                "owner-a",
                &AcpCloseSessionRequest {
                    session_id: String::from("close-live-replacement"),
                },
            )
            .expect("close promotes and retries the replacement cleanup");
        assert!(matches!(response, AcpResponse::AcpSessionClosedResponse(_)));
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(core.pending_restart_count(), 0);
        assert_eq!(
            host.killed
                .iter()
                .filter(|(process_id, _)| process_id == &restart_pending.process_id)
                .count(),
            2,
            "close retries the failed replacement abort exactly once"
        );
        assert!(host.killed.iter().all(|(_, signal)| signal == "SIGKILL"));
    }

    #[test]
    fn dispatch_resumable_drives_create_session_over_the_wire_types() {
        use agentos_protocol::generated::v1::AcpDeliverAgentOutputRequest;
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();

        // create_session via the resumable dispatch → AcpPendingResponse with the handle.
        let pending = core
            .dispatch_resumable(
                &mut host,
                "conn-a",
                AcpRequest::AcpCreateSessionRequest(echo_create_request()),
            )
            .expect("begin");
        let process_id = match pending {
            AcpResponse::AcpPendingResponse(p) => {
                assert_eq!(p.timeout_ms, 10_000, "initialize timeout is sidecar-owned");
                p.process_id
            }
            other => panic!("expected pending, got {other:?}"),
        };

        // deliver the initialize response → still pending.
        let step = core
            .dispatch_resumable(
                &mut host,
                "conn-a",
                AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
                    process_id: process_id.clone(),
                    chunk: br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#
                    .to_vec(),
                }),
            )
            .expect("deliver init");
        assert!(matches!(
            step,
            AcpResponse::AcpPendingResponse(AcpPendingResponse {
                timeout_ms: 30_000,
                ..
            })
        ));

        // deliver the session/new response → the real created result.
        let step = core
            .dispatch_resumable(
                &mut host,
                "conn-a",
                AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
                    process_id: process_id.clone(),
                    chunk: br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"wire-sess"}}
"#
                    .to_vec(),
                }),
            )
            .expect("deliver session/new");
        match step {
            AcpResponse::AcpSessionCreatedResponse(created) => {
                assert_eq!(created.session_id, "wire-sess")
            }
            other => panic!("expected created, got {other:?}"),
        }
        assert_eq!(core.session_count(), 1);
    }

    #[test]
    fn resumable_session_prompt_enforces_ownership() {
        use agentos_protocol::generated::v1::AcpSessionRequest;
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "conn-a"));
        let req = AcpSessionRequest {
            session_id: "s1".into(),
            method: "session/prompt".into(),
            params: None,
        };
        let err = core
            .begin_session_request(&mut host, "conn-b", &req)
            .expect_err("non-owner must be rejected");
        assert_eq!(err.code(), "invalid_state");
        assert_eq!(core.pending_prompt_count(), 0);
    }

    #[test]
    fn resumable_output_delivery_rejects_cross_owner_injection_before_mutation() {
        use agentos_protocol::generated::v1::AcpDeliverAgentOutputRequest;
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let pending = core
            .dispatch_resumable(
                &mut host,
                "conn-a",
                AcpRequest::AcpCreateSessionRequest(echo_create_request()),
            )
            .expect("begin create");
        let AcpResponse::AcpPendingResponse(pending) = pending else {
            panic!("create must be pending");
        };
        let request = AcpRequest::AcpDeliverAgentOutputRequest(AcpDeliverAgentOutputRequest {
            process_id: pending.process_id.clone(),
            chunk: br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}
"#
            .to_vec(),
        });

        let error = core
            .dispatch_resumable(&mut host, "conn-b", request.clone())
            .expect_err("another connection cannot inject adapter output");
        assert_eq!(error.code(), "invalid_state");
        assert_eq!(core.pending_create_count(), 1);
        assert_eq!(host.stdin.len(), 1, "rejected output writes nothing");

        core.dispatch_resumable(&mut host, "conn-a", request)
            .expect("owner advances its pending interaction");
        assert_eq!(host.stdin.len(), 2);
    }

    #[test]
    fn resumable_abort_is_owner_scoped_typed_and_restores_prompt_preamble() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        core.session_mut("owner-a", "s1")
            .expect("session")
            .pending_preamble = Some(String::from("resume from /history/session.jsonl"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: Some(String::from(r#"{"prompt":[]}"#)),
                },
            )
            .expect("begin prompt");
        assert_eq!(
            core.session("owner-a", "s1").unwrap().pending_preamble,
            None
        );

        let abort = AcpAbortPendingRequest {
            process_id: process_id.clone(),
            reason: AcpPendingAbortReason::InteractionTimeout,
            exit_code: None,
        };
        let error = core
            .abort_pending(&mut host, "owner-b", &abort)
            .expect_err("another owner cannot abort the prompt");
        assert_eq!(error.code(), "invalid_state");
        assert_eq!(core.pending_prompt_count(), 1);
        assert!(host.killed.is_empty());

        let response = core
            .abort_pending(&mut host, "owner-a", &abort)
            .expect("owner aborts prompt");
        assert!(matches!(
            response,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message })
                if code == "agent_interaction_timeout" && message.contains(&process_id)
        ));
        assert_eq!(core.pending_prompt_count(), 0);
        assert!(core.session("owner-a", "s1").unwrap().closed);
        assert_eq!(
            core.session("owner-a", "s1")
                .unwrap()
                .pending_preamble
                .as_deref(),
            Some("resume from /history/session.jsonl")
        );
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_driver_failure_abort_is_sidecar_typed_and_atomic() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        core.session_mut("owner-a", "s1")
            .expect("session")
            .pending_preamble = Some(String::from("durable continuation"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: process_id.clone(),
                    reason: AcpPendingAbortReason::DriverFailed,
                    exit_code: None,
                },
            )
            .expect("driver failure aborts through the sidecar");

        assert!(matches!(
            response,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message })
                if code == "agent_driver_failed" && message.contains(&process_id)
        ));
        assert_eq!(core.pending_prompt_count(), 0);
        assert!(core.session("owner-a", "s1").unwrap().closed);
        assert_eq!(
            core.session("owner-a", "s1")
                .unwrap()
                .pending_preamble
                .as_deref(),
            Some("durable continuation")
        );
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_caller_cancellation_is_sidecar_typed_and_atomic() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");

        let response = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: process_id.clone(),
                    reason: AcpPendingAbortReason::CallerCancelled,
                    exit_code: None,
                },
            )
            .expect("caller cancellation aborts through the sidecar");

        assert!(matches!(
            response,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, ref message })
                if code == "agent_interaction_cancelled" && message.contains(&process_id)
        ));
        assert_eq!(core.pending_prompt_count(), 0);
        assert!(core.session("owner-a", "s1").unwrap().closed);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_abort_retains_non_routable_cleanup_for_exact_owner_retry() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("begin create");
        host.abort_error = Some(String::from("injected execution release failure"));

        let error = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: process_id.clone(),
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: None,
                },
            )
            .expect_err("cleanup failure must propagate");
        assert_eq!(error.code(), "cleanup_failed");
        assert!(error
            .to_string()
            .contains("injected execution release failure"));
        assert_eq!(core.pending_create_count(), 0);
        assert_eq!(core.pending_cleanup_count(), 1);
        assert_eq!(
            host.killed,
            vec![(process_id.clone(), String::from("SIGKILL"))]
        );

        let wrong_owner = core
            .abort_pending(
                &mut host,
                "owner-b",
                &AcpAbortPendingRequest {
                    process_id: process_id.clone(),
                    reason: AcpPendingAbortReason::CallerCancelled,
                    exit_code: None,
                },
            )
            .expect_err("cleanup tombstone must remain owner-scoped");
        assert_eq!(wrong_owner.code(), "invalid_state");
        assert_eq!(host.killed.len(), 1, "wrong owner cannot drive cleanup");

        let retry = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id: process_id.clone(),
                    reason: AcpPendingAbortReason::CallerCancelled,
                    exit_code: None,
                },
            )
            .expect("exact owner retry completes cleanup");
        assert!(matches!(
            retry,
            AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, .. })
                if code == "agent_exited"
        ));
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(host.killed.len(), 2);

        let finished = core
            .abort_pending(
                &mut host,
                "owner-a",
                &AcpAbortPendingRequest {
                    process_id,
                    reason: AcpPendingAbortReason::AgentExited,
                    exit_code: None,
                },
            )
            .expect_err("completed cleanup is no longer pending");
        assert_eq!(finished.code(), "invalid_state");
    }

    #[test]
    fn retained_cleanup_consumes_process_route_capacity_until_retry_succeeds() {
        let mut core = AcpCore::with_process_route_limit(1).expect("valid route limit");
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("first route fits");
        host.abort_error = Some(String::from("injected release failure"));
        core.abort_pending(
            &mut host,
            "owner-a",
            &AcpAbortPendingRequest {
                process_id: process_id.clone(),
                reason: AcpPendingAbortReason::DriverFailed,
                exit_code: None,
            },
        )
        .expect_err("failed cleanup retains the charged route");

        let error = core
            .begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect_err("retained cleanup blocks admission at the configured bound");
        assert_eq!(error.code(), "limit_exceeded");
        assert!(error.to_string().contains("with_process_route_limit"));
        assert_eq!(host.stdin.len(), 1, "rejected admission spawns no adapter");

        core.abort_pending(
            &mut host,
            "owner-a",
            &AcpAbortPendingRequest {
                process_id,
                reason: AcpPendingAbortReason::DriverFailed,
                exit_code: None,
            },
        )
        .expect("cleanup retry releases route capacity");
        core.begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("admission succeeds after cleanup");
        assert_eq!(host.stdin.len(), 2);
    }

    #[test]
    fn close_retries_retained_abort_without_switching_to_orderly_finalization() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");
        host.abort_error = Some(String::from("injected abort failure"));
        core.abort_pending(
            &mut host,
            "owner-a",
            &AcpAbortPendingRequest {
                process_id: process_id.clone(),
                reason: AcpPendingAbortReason::InteractionTimeout,
                exit_code: None,
            },
        )
        .expect_err("abort cleanup is retained");

        let response = core
            .close_session(
                &mut host,
                "owner-a",
                &AcpCloseSessionRequest {
                    session_id: String::from("s1"),
                },
            )
            .expect("close finishes the retained abort");
        assert!(matches!(response, AcpResponse::AcpSessionClosedResponse(_)));
        assert!(core.session("owner-a", "s1").is_none());
        assert_eq!(core.pending_cleanup_count(), 0);
        assert_eq!(
            host.killed,
            vec![
                (process_id.clone(), String::from("SIGKILL")),
                (process_id, String::from("SIGKILL")),
            ],
            "close retries abort and never switches to SIGTERM"
        );
    }

    #[test]
    fn failed_owner_disposal_preserves_its_process_route_limit() {
        let mut core = AcpCore::new();
        core.set_process_route_limit("owner-a", 1)
            .expect("configure owner limit");
        let mut host = ResumableMockHost::default();
        core.begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("first route fits");
        host.abort_error = Some(String::from("injected owner cleanup failure"));
        core.dispose_owner(&mut host, "owner-a")
            .expect_err("failed disposal retains cleanup ownership");

        assert_eq!(
            core.begin_create_session(&mut host, "owner-a", &echo_create_request())
                .expect_err("retained owner cleanup remains charged to its configured limit")
                .code(),
            "limit_exceeded"
        );
        core.dispose_owner(&mut host, "owner-a")
            .expect("owner cleanup retry succeeds");
        core.begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("admission resumes after cleanup releases the owner");
    }

    #[test]
    fn malformed_prompt_output_restores_consumed_preamble_and_aborts_agent() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        core.session_mut("owner-a", "s1")
            .expect("session")
            .pending_preamble = Some(String::from("durable continuation"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");

        let error = core
            .feed_agent_output(&mut host, "owner-a", &process_id, b"not-json\n")
            .expect_err("malformed adapter output fails closed");
        assert!(error.to_string().contains("invalid JSON-RPC"));
        assert_eq!(core.pending_prompt_count(), 0);
        assert!(core.session("owner-a", "s1").unwrap().closed);
        assert_eq!(
            core.session("owner-a", "s1")
                .unwrap()
                .pending_preamble
                .as_deref(),
            Some("durable continuation")
        );
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn disposing_owner_after_prompt_abort_does_not_abort_the_closed_agent_twice() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");
        core.feed_agent_output(&mut host, "owner-a", &process_id, b"not-json\n")
            .expect_err("malformed output aborts the prompt agent");
        host.abort_error = Some(String::from("unknown agent process"));

        core.dispose_owner(&mut host, "owner-a")
            .expect("closed agent needs no second host cleanup");

        assert_eq!(core.session_count(), 0);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
        assert_eq!(host.abort_error.as_deref(), Some("unknown agent process"));
    }

    #[test]
    fn closing_session_after_prompt_abort_skips_removed_process_route() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "owner-a"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "owner-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");
        core.feed_agent_output(&mut host, "owner-a", &process_id, b"not-json\n")
            .expect_err("malformed output aborts the prompt agent");
        host.close_stdin_error = Some(String::from("unknown agent process"));

        core.close_session(
            &mut host,
            "owner-a",
            &AcpCloseSessionRequest {
                session_id: String::from("s1"),
            },
        )
        .expect("closed session needs no second process cleanup");

        assert_eq!(core.session_count(), 0);
        assert_eq!(
            host.close_stdin_error.as_deref(),
            Some("unknown agent process")
        );
    }

    #[test]
    fn dispose_owner_removes_only_that_owners_pending_and_live_state() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        let pending_a = core
            .begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("begin owner-a create");
        let pending_b = core
            .begin_create_session(&mut host, "owner-b", &echo_create_request())
            .expect("begin owner-b create");
        core.insert_session(record("live-a", "owner-a"));
        core.insert_session(record("live-b", "owner-b"));

        core.dispose_owner(&mut host, "owner-a")
            .expect("dispose owner-a");

        assert_eq!(core.pending_create_count(), 1);
        assert_eq!(core.session_count(), 1);
        assert!(core.session("owner-b", "live-b").is_some());
        assert_eq!(
            host.killed,
            vec![
                (pending_a, String::from("SIGKILL")),
                (String::from("proc-live-a"), String::from("SIGKILL")),
            ]
        );
        assert!(!host.killed.iter().any(|(process, _)| process == &pending_b));
    }

    #[test]
    fn drop_owner_state_removes_state_without_repeating_host_cleanup() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.begin_create_session(&mut host, "owner-a", &echo_create_request())
            .expect("begin owner-a create");
        core.begin_create_session(&mut host, "owner-b", &echo_create_request())
            .expect("begin owner-b create");
        core.insert_session(record("live-a", "owner-a"));
        core.insert_session(record("live-b", "owner-b"));

        core.drop_owner_state("owner-a");

        assert_eq!(core.pending_create_count(), 1);
        assert_eq!(core.session_count(), 1);
        assert!(core.session("owner-b", "live-b").is_some());
        assert!(host.killed.is_empty(), "host teardown already ran");
    }

    #[test]
    fn resumable_session_request_rejects_a_second_in_flight_request_before_writing() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "conn-a"));
        let request = AcpSessionRequest {
            session_id: String::from("s1"),
            method: String::from("session/prompt"),
            params: None,
        };

        core.begin_session_request(&mut host, "conn-a", &request)
            .expect("first request starts");
        let error = core
            .begin_session_request(&mut host, "conn-a", &request)
            .expect_err("second request must be rejected as busy");
        assert_eq!(error.code(), "conflict");
        assert_eq!(host.stdin.len(), 1);
        assert_eq!(core.session("conn-a", "s1").unwrap().next_request_id, 2);
        assert_eq!(core.pending_prompt_count(), 1);
    }

    #[test]
    fn interrupted_resumable_prompt_drains_old_boundary_before_accepting_next_prompt() {
        let mut core = AcpCore::new();
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "conn-a"));
        core.session_mut("conn-a", "s1")
            .expect("session")
            .pending_preamble = Some(String::from("durable continuation"));
        let request = AcpSessionRequest {
            session_id: String::from("s1"),
            method: String::from("session/prompt"),
            params: Some(String::from(r#"{"prompt":[]}"#)),
        };

        let process_id = core
            .begin_session_request(&mut host, "conn-a", &request)
            .expect("first prompt starts");
        assert_eq!(core.pending_prompt_count(), 1);
        assert_eq!(core.session("conn-a", "s1").unwrap().pending_preamble, None);
        let wrong_owner = core
            .interrupt_pending_prompt("conn-b", &process_id)
            .expect_err("another owner cannot interrupt the prompt");
        assert_eq!(wrong_owner.code(), "invalid_state");
        assert_eq!(core.pending_prompt_count(), 1);
        assert_eq!(
            core.interrupt_pending_prompt("conn-a", &process_id)
                .expect("transport interrupt marks exact prompt for draining"),
            "s1"
        );
        assert_eq!(core.pending_prompt_count(), 1);
        assert!(!core.session("conn-a", "s1").unwrap().closed);
        assert_eq!(
            core.session("conn-a", "s1")
                .unwrap()
                .pending_preamble
                .as_deref(),
            Some("durable continuation")
        );

        let busy = core
            .begin_session_request(&mut host, "conn-a", &request)
            .expect_err("next prompt waits for the cancelled response boundary");
        assert_eq!(busy.code(), "conflict");
        assert_eq!(host.stdin.len(), 1);

        assert!(matches!(
            core.feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"stale text"}}}}
{"jsonrpc":"2.0","id":999,"result":{"stopReason":"wrong request"}}
"#,
            )
            .expect("stale output before the boundary is discarded"),
            ResumeStep::Pending
        ));
        assert_eq!(core.pending_event_count(), 0);

        let drained = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"more stale text"}}}}
{"jsonrpc":"2.0","id":1,"result":{"stopReason":"cancelled"}}
"#,
            )
            .expect("matching cancelled response drains the old boundary");
        assert!(matches!(
            drained,
            ResumeStep::Done(AcpResponse::AcpErrorResponse(AcpErrorResponse { ref code, .. }))
                if code == "agent_interaction_cancelled"
        ));
        assert_eq!(core.pending_prompt_count(), 0);
        assert_eq!(core.pending_event_count(), 0);

        core.begin_session_request(&mut host, "conn-a", &request)
            .expect("session accepts a later prompt after the old boundary drains");
        assert_eq!(core.pending_prompt_count(), 1);
        assert_eq!(host.stdin.len(), 2);
        let first: Value = serde_json::from_str(&host.stdin[0]).expect("first prompt JSON");
        let second: Value = serde_json::from_str(&host.stdin[1]).expect("second prompt JSON");
        assert_eq!(first["id"], json!(1));
        assert_eq!(second["id"], json!(2));
        assert!(
            host.stdin[1].contains("durable continuation"),
            "the cancelled turn must restore its one-shot preamble for the next prompt"
        );
    }

    #[test]
    fn resumable_create_event_overflow_is_typed_atomic_and_cleans_up() {
        let mut core = AcpCore::with_pending_event_limit(1);
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_create_session(&mut host, "conn-a", &echo_create_request())
            .expect("begin create");
        let error = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"one"}
{"jsonrpc":"2.0","method":"two"}
"#,
            )
            .expect_err("second notification exceeds the bound");
        assert_eq!(error.code(), "limit_exceeded");
        assert_eq!(core.pending_create_count(), 0);
        assert_eq!(core.session_count(), 0);
        assert_eq!(core.pending_event_count(), 0);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_resume_event_overflow_is_typed_atomic_and_cleans_up() {
        let mut core = AcpCore::with_pending_event_limit(1);
        let mut host = ResumableMockHost::default();
        let process_id = core
            .begin_resume_session(&mut host, "conn-a", &echo_resume_request("s1", None))
            .expect("begin resume");
        let error = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"one"}
{"jsonrpc":"2.0","method":"two"}
"#,
            )
            .expect_err("second notification exceeds the bound");
        assert_eq!(error.code(), "limit_exceeded");
        assert_eq!(core.pending_resume_count(), 0);
        assert_eq!(core.session_count(), 0);
        assert_eq!(core.pending_event_count(), 0);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_prompt_event_overflow_is_typed_atomic_and_closes_the_session() {
        let mut core = AcpCore::with_pending_event_limit(1);
        let mut host = ResumableMockHost::default();
        core.insert_session(record("s1", "conn-a"));
        let process_id = core
            .begin_session_request(
                &mut host,
                "conn-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");
        let error = core
            .feed_agent_output(
                &mut host,
                "conn-a",
                &process_id,
                br#"{"jsonrpc":"2.0","method":"one"}
{"jsonrpc":"2.0","method":"two"}
"#,
            )
            .expect_err("second notification exceeds the bound");
        assert_eq!(error.code(), "limit_exceeded");
        assert_eq!(core.pending_prompt_count(), 0);
        assert!(core.session("conn-a", "s1").unwrap().closed);
        assert_eq!(core.pending_event_count(), 0);
        assert_eq!(host.killed, vec![(process_id, String::from("SIGKILL"))]);
    }

    #[test]
    fn resumable_event_byte_limit_cleans_up_create_resume_and_prompt_atomically() {
        let oversized = format!(
            "{}\n",
            serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": { "payload": "x".repeat(256) },
            }))
            .expect("encode oversized notification")
        );

        let mut create_core = AcpCore::with_pending_event_limits(16, 128);
        let mut create_host = ResumableMockHost::default();
        let create_process = create_core
            .begin_create_session(&mut create_host, "conn-a", &echo_create_request())
            .expect("begin create");
        let create_error = create_core
            .feed_agent_output(
                &mut create_host,
                "conn-a",
                &create_process,
                oversized.as_bytes(),
            )
            .expect_err("oversized create notification");
        assert_eq!(create_error.code(), "limit_exceeded");
        assert_eq!(create_core.pending_interaction_count(), 0);
        assert_eq!(create_core.pending_event_count(), 0);
        assert_eq!(
            create_host.killed,
            vec![(create_process, String::from("SIGKILL"))]
        );

        let mut resume_core = AcpCore::with_pending_event_limits(16, 128);
        let mut resume_host = ResumableMockHost::default();
        let resume_process = resume_core
            .begin_resume_session(&mut resume_host, "conn-a", &echo_resume_request("s1", None))
            .expect("begin resume");
        let resume_error = resume_core
            .feed_agent_output(
                &mut resume_host,
                "conn-a",
                &resume_process,
                oversized.as_bytes(),
            )
            .expect_err("oversized resume notification");
        assert_eq!(resume_error.code(), "limit_exceeded");
        assert_eq!(resume_core.pending_interaction_count(), 0);
        assert_eq!(resume_core.pending_event_count(), 0);
        assert_eq!(
            resume_host.killed,
            vec![(resume_process, String::from("SIGKILL"))]
        );

        let mut prompt_core = AcpCore::with_pending_event_limits(16, 128);
        let mut prompt_host = ResumableMockHost::default();
        prompt_core.insert_session(record("s1", "conn-a"));
        let prompt_process = prompt_core
            .begin_session_request(
                &mut prompt_host,
                "conn-a",
                &AcpSessionRequest {
                    session_id: String::from("s1"),
                    method: String::from("session/prompt"),
                    params: None,
                },
            )
            .expect("begin prompt");
        let prompt_error = prompt_core
            .feed_agent_output(
                &mut prompt_host,
                "conn-a",
                &prompt_process,
                oversized.as_bytes(),
            )
            .expect_err("oversized prompt notification");
        assert_eq!(prompt_error.code(), "limit_exceeded");
        assert_eq!(prompt_core.pending_interaction_count(), 0);
        assert!(prompt_core.session("conn-a", "s1").unwrap().closed);
        assert_eq!(prompt_core.pending_event_count(), 0);
        assert_eq!(
            prompt_host.killed,
            vec![(prompt_process, String::from("SIGKILL"))]
        );
    }
}
