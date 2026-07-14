//! Thin cron transport adapter.
//!
//! The sidecar owns cron grammar, IDs/defaults, job and run state, overlap
//! policy, missed-fire coalescing, alarm generations, and lifecycle events.
//! This module retains only resources the sidecar cannot access: the host clock
//! used to deliver an absolute wake and in-process callback closures.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agentos_sidecar_client::wire;
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::agent_os::AgentOs;
use crate::error::ClientError;
use crate::session::{CreateSessionOptions, McpServerConfig};
use crate::stream::{RoutedStreamEvent, StreamRouteFailure};

/// Overlap policy for a cron job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CronOverlap {
    Allow,
    Skip,
    Queue,
}

/// Result returned by a host cron callback and forwarded to the sidecar.
pub type CronCallbackResult = Result<(), String>;

/// Host callback retained by the client because closures cannot cross the wire.
pub type CronCallback =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, CronCallbackResult> + Send + Sync>;

/// A cron action. `Callback` holds an in-process closure and cannot cross the wire.
#[derive(Clone)]
pub enum CronAction {
    /// Create a session, prompt it, then close it.
    Session {
        agent_type: String,
        prompt: String,
        options: Option<CreateSessionOptions>,
    },
    /// Run a command with structured argv.
    Exec { command: String, args: Vec<String> },
    /// Invoke a host-side callback.
    Callback { callback: CronCallback },
}

impl std::fmt::Debug for CronAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CronAction::Session {
                agent_type, prompt, ..
            } => f
                .debug_struct("Session")
                .field("agent_type", agent_type)
                .field("prompt", prompt)
                .finish_non_exhaustive(),
            CronAction::Exec { command, args } => f
                .debug_struct("Exec")
                .field("command", command)
                .field("args", args)
                .finish(),
            CronAction::Callback { .. } => f.debug_struct("Callback").finish_non_exhaustive(),
        }
    }
}

/// Options forwarded to the sidecar by [`AgentOs::schedule_cron`].
#[derive(Clone)]
pub struct CronJobOptions {
    /// Optional caller-selected ID. Omission lets the sidecar allocate one.
    pub id: Option<String>,
    /// 5/6/7-field cron expression or an ISO-8601 one-shot timestamp.
    pub schedule: String,
    pub action: CronAction,
    /// Optional caller override. Omission lets the sidecar default to allow.
    pub overlap: Option<CronOverlap>,
}

/// Authoritative cron job state returned by the sidecar.
#[derive(Debug, Clone)]
pub struct CronJobInfo {
    pub id: String,
    pub schedule: String,
    pub action: CronAction,
    pub overlap: CronOverlap,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub running: bool,
}

/// A cron lifecycle event emitted by the sidecar.
#[derive(Debug, Clone)]
pub enum CronEvent {
    Fire {
        job_id: String,
        time: DateTime<Utc>,
    },
    Complete {
        job_id: String,
        time: DateTime<Utc>,
        duration_ms: f64,
    },
    Error {
        job_id: String,
        time: DateTime<Utc>,
        error: String,
    },
}

/// Absolute sidecar alarm forwarded to a host with durable wake facilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronAlarmUpdate {
    pub generation: u64,
    pub next_alarm_ms: Option<u64>,
}

/// Host-only alarm bridge. This is deliberately narrower than a schedule
/// driver: it cannot parse expressions or own job policy; it can only arrange
/// delivery of the sidecar's opaque generation at an absolute timestamp.
pub type CronAlarmHandler = Arc<
    dyn Fn(CronAlarmUpdate) -> futures::future::BoxFuture<'static, Result<(), String>>
        + Send
        + Sync,
>;

/// Handle to a sidecar-owned cron job.
#[derive(Clone)]
pub struct CronJobHandle {
    pub id: String,
    client: AgentOs,
}

impl CronJobHandle {
    /// Cancel the job (a no-op when the sidecar no longer has the ID).
    pub async fn cancel(&self) -> Result<(), ClientError> {
        self.client.cancel_cron_job(&self.id).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireCreateSessionOptions {
    cwd: Option<String>,
    env: BTreeMap<String, String>,
    mcp_servers: Vec<McpServerConfig>,
    skip_os_instructions: bool,
    additional_instructions: Option<String>,
}

impl From<CreateSessionOptions> for WireCreateSessionOptions {
    fn from(options: CreateSessionOptions) -> Self {
        Self {
            cwd: options.cwd,
            env: options.env,
            mcp_servers: options.mcp_servers,
            skip_os_instructions: options.skip_os_instructions,
            additional_instructions: options.additional_instructions,
        }
    }
}

impl From<WireCreateSessionOptions> for CreateSessionOptions {
    fn from(options: WireCreateSessionOptions) -> Self {
        Self {
            cwd: options.cwd,
            env: options.env,
            mcp_servers: options.mcp_servers,
            skip_os_instructions: options.skip_os_instructions,
            additional_instructions: options.additional_instructions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum WireCronAction {
    Session {
        #[serde(rename = "agentType")]
        agent_type: String,
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<WireCreateSessionOptions>,
    },
    Exec {
        command: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
    Callback {
        #[serde(rename = "callbackId")]
        callback_id: String,
    },
}

struct CallbackRoute {
    callback: CronCallback,
    scheduled: bool,
    active_runs: usize,
}

#[derive(Default)]
struct CallbackRegistry {
    sequence: u64,
    routes: HashMap<String, CallbackRoute>,
    by_job: HashMap<String, String>,
}

#[derive(Default)]
struct AlarmState {
    generation: u64,
    next_alarm_ms: Option<u64>,
    task: Option<JoinHandle<()>>,
}

/// Host-only state for sidecar cron delivery.
pub struct CronManager {
    callbacks: parking_lot::Mutex<CallbackRegistry>,
    alarm: parking_lot::Mutex<AlarmState>,
    alarm_handler: parking_lot::Mutex<Option<CronAlarmHandler>>,
    event_tx: broadcast::Sender<RoutedStreamEvent<CronEvent>>,
    event_route_failure: parking_lot::Mutex<Option<StreamRouteFailure>>,
    disposed: AtomicBool,
}

impl CronManager {
    pub(crate) fn new() -> Self {
        let (event_tx, _receiver) = broadcast::channel(256);
        Self {
            callbacks: parking_lot::Mutex::new(CallbackRegistry::default()),
            alarm: parking_lot::Mutex::new(AlarmState::default()),
            alarm_handler: parking_lot::Mutex::new(None),
            event_tx,
            event_route_failure: parking_lot::Mutex::new(None),
            disposed: AtomicBool::new(false),
        }
    }

    pub(crate) fn dispose(&self) {
        if self.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(task) = self.alarm.lock().task.take() {
            task.abort();
        }
        let mut callbacks = self.callbacks.lock();
        callbacks.routes.clear();
        callbacks.by_job.clear();
    }

    pub(crate) fn fail_event_route(&self, failure: StreamRouteFailure) {
        let mut route_failure = self.event_route_failure.lock();
        if route_failure.is_none() {
            *route_failure = Some(failure);
        }
        drop(route_failure);
        let generation = {
            let mut alarm = self.alarm.lock();
            if let Some(task) = alarm.task.take() {
                task.abort();
            }
            alarm.generation = alarm.generation.saturating_add(1);
            alarm.next_alarm_ms = None;
            alarm.generation
        };
        if let Some(handler) = self.alarm_handler.lock().clone() {
            tokio::spawn(async move {
                if let Err(error) = handler(CronAlarmUpdate {
                    generation,
                    next_alarm_ms: None,
                })
                .await
                {
                    tracing::error!(
                        error,
                        generation,
                        "failed to clear host cron alarm after control-route failure"
                    );
                }
            });
        }
        let _ = self.event_tx.send(failure.event());
    }

    fn ensure_event_route(&self) -> Result<(), ClientError> {
        match *self.event_route_failure.lock() {
            Some(StreamRouteFailure::Lagged { skipped }) => {
                Err(ClientError::EventStreamLagged { skipped })
            }
            Some(StreamRouteFailure::Closed { context }) => {
                Err(ClientError::EventStreamClosed { context })
            }
            None => Ok(()),
        }
    }

    fn set_alarm_handler(&self, handler: CronAlarmHandler) {
        if let Some(task) = self.alarm.lock().task.take() {
            task.abort();
        }
        *self.alarm_handler.lock() = Some(handler);
    }

    fn allocate_callback(&self, callback: CronCallback) -> Result<String, ClientError> {
        let mut registry = self.callbacks.lock();
        registry.sequence = registry.sequence.checked_add(1).ok_or_else(|| {
            ClientError::Sidecar("cron callback id counter exhausted; recreate the VM".to_string())
        })?;
        let callback_id = format!("host-cron-callback-{}", registry.sequence);
        registry.routes.insert(
            callback_id.clone(),
            CallbackRoute {
                callback,
                scheduled: false,
                active_runs: 0,
            },
        );
        Ok(callback_id)
    }

    fn replace_job_callback(&self, job_id: &str, callback_id: Option<&str>) {
        let mut registry = self.callbacks.lock();
        if let Some(previous) = registry.by_job.remove(job_id) {
            if Some(previous.as_str()) != callback_id {
                if let Some(route) = registry.routes.get_mut(&previous) {
                    route.scheduled = false;
                }
                release_callback(&mut registry, &previous);
            }
        }
        if let Some(callback_id) = callback_id {
            if let Some(route) = registry.routes.get_mut(callback_id) {
                route.scheduled = true;
                registry
                    .by_job
                    .insert(job_id.to_string(), callback_id.to_string());
            }
        }
    }

    fn release_unscheduled_callback(&self, callback_id: &str) {
        let mut registry = self.callbacks.lock();
        release_callback(&mut registry, callback_id);
    }

    fn callback_for_run(&self, callback_id: &str) -> Result<CronCallback, String> {
        let mut registry = self.callbacks.lock();
        let route = registry
            .routes
            .get_mut(callback_id)
            .ok_or_else(|| format!("cron callback route not found: {callback_id}"))?;
        route.active_runs += 1;
        Ok(route.callback.clone())
    }

    fn complete_callback_run(&self, callback_id: &str) {
        let mut registry = self.callbacks.lock();
        if let Some(route) = registry.routes.get_mut(callback_id) {
            route.active_runs = route.active_runs.saturating_sub(1);
        }
        release_callback(&mut registry, callback_id);
    }

    fn callback_action(&self, callback_id: &str) -> CronAction {
        let callback = self
            .callbacks
            .lock()
            .routes
            .get(callback_id)
            .map(|route| route.callback.clone())
            .unwrap_or_else(|| {
                Arc::new(|| {
                    Box::pin(async {
                        Err(String::from(
                            "cron callback route is unavailable on this host",
                        ))
                    })
                })
            });
        CronAction::Callback { callback }
    }

    fn apply_alarm<'a>(
        self: &'a Arc<Self>,
        client: &'a AgentOs,
        alarm: wire::CronAlarm,
    ) -> futures::future::BoxFuture<'a, Result<(), ClientError>> {
        Box::pin(async move {
            if self.disposed.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.ensure_event_route()?;
            let handler = self.alarm_handler.lock().clone();
            {
                let mut state = self.alarm.lock();
                self.ensure_event_route()?;
                if alarm.generation < state.generation {
                    return Ok(());
                }
                if alarm.generation == state.generation
                    && alarm.next_alarm_ms == state.next_alarm_ms
                {
                    return Ok(());
                }
                if let Some(task) = state.task.take() {
                    task.abort();
                }
                state.generation = alarm.generation;
                state.next_alarm_ms = alarm.next_alarm_ms;

                if handler.is_none() {
                    let Some(next_alarm_ms) = alarm.next_alarm_ms else {
                        return Ok(());
                    };
                    let manager = Arc::downgrade(self);
                    let client = client.downgrade_inner();
                    let generation = alarm.generation;
                    state.task = Some(tokio::spawn(async move {
                        tokio::time::sleep(duration_until(next_alarm_ms)).await;
                        let (Some(manager), Some(inner)) = (manager.upgrade(), client.upgrade())
                        else {
                            return;
                        };
                        let client = AgentOs::from_inner(inner);
                        if let Err(error) = manager.wake(&client, generation).await {
                            tracing::error!(?error, generation, "cron sidecar wake failed");
                        }
                    }));
                    return Ok(());
                }
            }

            let handler = handler.expect("alarm handler checked above");
            handler(CronAlarmUpdate {
                generation: alarm.generation,
                next_alarm_ms: alarm.next_alarm_ms,
            })
            .await
            .map_err(|error| ClientError::Sidecar(format!("failed to arm cron alarm: {error}")))?;

            // A route failure can race an asynchronous host alarm update. If the stale arm
            // completed after `fail_event_route` sent its clear, clear once more before returning
            // the sticky route error so a durable host cannot retain the stale wake.
            if let Err(error) = self.ensure_event_route() {
                let generation = self.alarm.lock().generation;
                handler(CronAlarmUpdate {
                    generation,
                    next_alarm_ms: None,
                })
                .await
                .map_err(|clear_error| {
                    ClientError::Sidecar(format!(
                        "failed to clear host cron alarm after route failure: {clear_error}"
                    ))
                })?;
                return Err(error);
            }
            Ok(())
        })
    }

    async fn wake(self: &Arc<Self>, client: &AgentOs, generation: u64) -> Result<(), ClientError> {
        if self.disposed.load(Ordering::SeqCst) {
            return Ok(());
        }
        let response = client
            .transport()
            .request_wire(
                cron_ownership(client),
                wire::RequestPayload::WakeCronRequest(wire::WakeCronRequest { generation }),
            )
            .await?;
        match response {
            wire::ResponsePayload::CronWakeResponse(dispatch) => {
                self.consume_dispatch(client, dispatch.alarm, dispatch.runs, dispatch.events)
                    .await
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("wake_cron", other)),
        }
    }

    pub(crate) fn consume_dispatch<'a>(
        self: &'a Arc<Self>,
        client: &'a AgentOs,
        alarm: wire::CronAlarm,
        runs: Vec<wire::CronRun>,
        events: Vec<wire::CronEventRecord>,
    ) -> futures::future::BoxFuture<'a, Result<(), ClientError>> {
        Box::pin(async move {
            self.ensure_event_route()?;
            self.apply_alarm(client, alarm).await?;
            self.ensure_event_route()?;
            for event in events {
                self.emit_event(event)?;
            }
            self.ensure_event_route()?;
            for run in runs {
                let manager = Arc::clone(self);
                let client = client.clone();
                tokio::spawn(async move {
                    if let Err(error) = manager.execute_run(&client, run).await {
                        tracing::error!(?error, "cron run completion failed");
                    }
                });
            }
            Ok(())
        })
    }

    fn emit_event(&self, event: wire::CronEventRecord) -> Result<(), ClientError> {
        let time = datetime_from_ms(event.time_ms, "cron event time")?;
        let event = match event.kind {
            wire::CronEventKind::Fire => CronEvent::Fire {
                job_id: event.job_id,
                time,
            },
            wire::CronEventKind::Complete => CronEvent::Complete {
                job_id: event.job_id,
                time,
                duration_ms: event.duration_ms.ok_or_else(|| {
                    ClientError::Sidecar(
                        "sidecar complete cron event is missing duration_ms".to_string(),
                    )
                })? as f64,
            },
            wire::CronEventKind::Error => CronEvent::Error {
                job_id: event.job_id,
                time,
                error: event.error.ok_or_else(|| {
                    ClientError::Sidecar("sidecar error cron event is missing error".to_string())
                })?,
            },
        };
        if self.event_tx.receiver_count() > 0 {
            if let Err(error) = self.event_tx.send(RoutedStreamEvent::Data(event)) {
                tracing::warn!(?error, "failed to deliver cron lifecycle event");
            }
        }
        Ok(())
    }

    async fn execute_run(
        self: &Arc<Self>,
        client: &AgentOs,
        run: wire::CronRun,
    ) -> Result<(), ClientError> {
        let action = serde_json::from_str::<WireCronAction>(&run.action)
            .map_err(|error| format!("invalid cron action: {error}"));
        let callback_id = match action.as_ref() {
            Ok(WireCronAction::Callback { callback_id }) => Some(callback_id.clone()),
            _ => None,
        };
        let action_result = match action {
            Ok(action) => run_host_action(self, action).await,
            Err(error) => Err(error),
        };
        if let Some(callback_id) = callback_id {
            self.complete_callback_run(&callback_id);
        }

        // VM disposal removes the sidecar scheduler and all active runs. A
        // host action that finishes during teardown has nothing left to
        // acknowledge and must not race the closed transport.
        if self.disposed.load(Ordering::SeqCst) {
            return Ok(());
        }

        let response = client
            .transport()
            .request_wire(
                cron_ownership(client),
                wire::RequestPayload::CompleteCronRunRequest(wire::CompleteCronRunRequest {
                    run_id: run.run_id,
                    error: action_result.err(),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::CronRunCompletedResponse(dispatch) => {
                self.consume_dispatch(client, dispatch.alarm, dispatch.runs, dispatch.events)
                    .await
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("complete_cron_run", other)),
        }
    }
}

fn release_callback(registry: &mut CallbackRegistry, callback_id: &str) {
    if registry
        .routes
        .get(callback_id)
        .is_some_and(|route| !route.scheduled && route.active_runs == 0)
    {
        registry.routes.remove(callback_id);
    }
}

async fn run_host_action(manager: &Arc<CronManager>, action: WireCronAction) -> CronCallbackResult {
    match action {
        WireCronAction::Session { .. } => Err(String::from(
            "sidecar returned non-host cron action to client: session",
        )),
        WireCronAction::Exec { .. } => Err(String::from(
            "sidecar returned non-host cron action to client: exec",
        )),
        WireCronAction::Callback { callback_id } => {
            let callback = manager.callback_for_run(&callback_id)?;
            callback().await
        }
    }
}

impl AgentOs {
    fn ensure_cron_event_route(&self) -> Result<(), ClientError> {
        match *self.inner().control_route_failure.lock() {
            Some(StreamRouteFailure::Lagged { skipped }) => {
                Err(ClientError::EventStreamLagged { skipped })
            }
            Some(StreamRouteFailure::Closed { context }) => {
                Err(ClientError::EventStreamClosed { context })
            }
            None => Ok(()),
        }
    }

    /// Forward a cron registration to the sidecar.
    pub async fn schedule_cron(
        &self,
        options: CronJobOptions,
    ) -> Result<CronJobHandle, ClientError> {
        self.ensure_cron_event_route()?;
        let (action, callback_id) = match options.action {
            CronAction::Session {
                agent_type,
                prompt,
                options,
            } => (
                WireCronAction::Session {
                    agent_type,
                    prompt,
                    options: options.map(Into::into),
                },
                None,
            ),
            CronAction::Exec { command, args } => (WireCronAction::Exec { command, args }, None),
            CronAction::Callback { callback } => {
                let callback_id = self.cron().allocate_callback(callback)?;
                (
                    WireCronAction::Callback {
                        callback_id: callback_id.clone(),
                    },
                    Some(callback_id),
                )
            }
        };
        let action = serde_json::to_string(&action).map_err(|error| {
            ClientError::Sidecar(format!("failed to encode cron action: {error}"))
        })?;
        let request = wire::ScheduleCronRequest {
            id: options.id,
            schedule: options.schedule.clone(),
            action,
            overlap: options.overlap.map(to_wire_overlap),
        };
        let response = self
            .transport()
            .request_wire(
                cron_ownership(self),
                wire::RequestPayload::ScheduleCronRequest(request),
            )
            .await;
        let scheduled = match response {
            Ok(wire::ResponsePayload::CronScheduledResponse(scheduled)) => scheduled,
            Ok(wire::ResponsePayload::RejectedResponse(rejected)) => {
                if let Some(callback_id) = callback_id.as_deref() {
                    self.cron().release_unscheduled_callback(callback_id);
                }
                return Err(cron_rejected(rejected, &options.schedule));
            }
            Ok(other) => {
                if let Some(callback_id) = callback_id.as_deref() {
                    self.cron().release_unscheduled_callback(callback_id);
                }
                return Err(unexpected_response("schedule_cron", other));
            }
            Err(error) => {
                if let Some(callback_id) = callback_id.as_deref() {
                    self.cron().release_unscheduled_callback(callback_id);
                }
                return Err(error.into());
            }
        };
        self.cron()
            .replace_job_callback(&scheduled.id, callback_id.as_deref());
        self.cron().apply_alarm(self, scheduled.alarm).await?;
        Ok(CronJobHandle {
            id: scheduled.id,
            client: self.clone(),
        })
    }

    /// Read the authoritative sidecar cron registry.
    pub async fn list_cron_jobs(&self) -> Result<Vec<CronJobInfo>, ClientError> {
        self.ensure_cron_event_route()?;
        let response = self
            .transport()
            .request_wire(
                cron_ownership(self),
                wire::RequestPayload::ListCronJobsRequest,
            )
            .await?;
        match response {
            wire::ResponsePayload::CronJobsResponse(response) => {
                self.cron().apply_alarm(self, response.alarm).await?;
                response
                    .jobs
                    .into_iter()
                    .map(|job| self.cron_job_info(job))
                    .collect()
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("list_cron_jobs", other)),
        }
    }

    fn cron_job_info(&self, job: wire::CronJobEntry) -> Result<CronJobInfo, ClientError> {
        let action = match serde_json::from_str::<WireCronAction>(&job.action)
            .map_err(|error| ClientError::Sidecar(format!("invalid cron action: {error}")))?
        {
            WireCronAction::Session {
                agent_type,
                prompt,
                options,
            } => CronAction::Session {
                agent_type,
                prompt,
                options: options.map(Into::into),
            },
            WireCronAction::Exec { command, args } => CronAction::Exec { command, args },
            WireCronAction::Callback { callback_id } => self.cron().callback_action(&callback_id),
        };
        Ok(CronJobInfo {
            id: job.id,
            schedule: job.schedule,
            action,
            overlap: from_wire_overlap(job.overlap),
            last_run: job
                .last_run_ms
                .map(|value| datetime_from_ms(value, "cron lastRun"))
                .transpose()?,
            next_run: job
                .next_run_ms
                .map(|value| datetime_from_ms(value, "cron nextRun"))
                .transpose()?,
            run_count: job.run_count,
            running: job.running,
        })
    }

    /// Cancel a cron job by ID.
    pub async fn cancel_cron_job(&self, id: &str) -> Result<(), ClientError> {
        self.ensure_cron_event_route()?;
        let response = self
            .transport()
            .request_wire(
                cron_ownership(self),
                wire::RequestPayload::CancelCronJobRequest(wire::CancelCronJobRequest {
                    id: id.to_string(),
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::CronCancelledResponse(response) => {
                if response.cancelled {
                    self.cron().replace_job_callback(id, None);
                }
                self.cron().apply_alarm(self, response.alarm).await?;
                Ok(())
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("cancel_cron_job", other)),
        }
    }

    /// Subscribe to sidecar cron lifecycle events.
    pub fn cron_events(
        &self,
    ) -> std::pin::Pin<Box<dyn Stream<Item = std::result::Result<CronEvent, ClientError>> + Send>>
    {
        let failure = *self.inner().control_route_failure.lock();
        if let Some(failure) = failure {
            return Box::pin(futures::stream::once(async move {
                Err(match failure {
                    StreamRouteFailure::Lagged { skipped } => {
                        ClientError::EventStreamLagged { skipped }
                    }
                    StreamRouteFailure::Closed { context } => {
                        ClientError::EventStreamClosed { context }
                    }
                })
            }));
        }
        let rx = self.cron().event_tx.subscribe();
        Box::pin(futures::stream::unfold(Some(rx), move |state| async move {
            let mut rx = state?;
            match rx.recv().await {
                Ok(RoutedStreamEvent::Data(event)) => Some((Ok(event), Some(rx))),
                Ok(RoutedStreamEvent::Lagged { skipped })
                | Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    Some((Err(ClientError::EventStreamLagged { skipped }), None))
                }
                Ok(RoutedStreamEvent::Closed { context }) => {
                    Some((Err(ClientError::EventStreamClosed { context }), None))
                }
                Err(broadcast::error::RecvError::Closed) => None,
            }
        }))
    }

    /// Replace the normal process timer with a host-specific absolute-alarm
    /// bridge (for example a durable actor `schedule_at`).
    pub fn set_cron_alarm_handler(&self, handler: CronAlarmHandler) {
        self.cron().set_alarm_handler(handler);
        if let Some(failure) = *self.inner().control_route_failure.lock() {
            self.cron().fail_event_route(failure);
        }
    }

    /// Deliver a generation previously returned to a host alarm bridge.
    pub async fn wake_cron_generation(&self, generation: u64) -> Result<(), ClientError> {
        self.ensure_cron_event_route()?;
        self.cron().wake(self, generation).await
    }

    /// Export an opaque sidecar-owned scheduler snapshot for a durable host.
    ///
    /// Hosts must store this string verbatim and return it only to
    /// [`AgentOs::import_cron_state`]. It is not a public scheduling model.
    #[doc(hidden)]
    pub async fn export_cron_state(&self) -> Result<String, ClientError> {
        self.ensure_cron_event_route()?;
        let response = self
            .transport()
            .request_wire(
                cron_ownership(self),
                wire::RequestPayload::ExportCronStateRequest,
            )
            .await?;
        match response {
            wire::ResponsePayload::CronStateExportedResponse(response) => Ok(response.state),
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("export_cron_state", other)),
        }
    }

    /// Restore an opaque snapshot produced by [`AgentOs::export_cron_state`].
    #[doc(hidden)]
    pub async fn import_cron_state(&self, state: String) -> Result<(), ClientError> {
        self.ensure_cron_event_route()?;
        let response = self
            .transport()
            .request_wire(
                cron_ownership(self),
                wire::RequestPayload::ImportCronStateRequest(wire::ImportCronStateRequest {
                    state,
                }),
            )
            .await?;
        match response {
            wire::ResponsePayload::CronStateImportedResponse(dispatch) => {
                self.cron()
                    .consume_dispatch(self, dispatch.alarm, dispatch.runs, dispatch.events)
                    .await
            }
            wire::ResponsePayload::RejectedResponse(rejected) => Err(cron_rejected(rejected, "")),
            other => Err(unexpected_response("import_cron_state", other)),
        }
    }
}

fn cron_ownership(client: &AgentOs) -> wire::OwnershipScope {
    wire::OwnershipScope::VmOwnership(wire::VmOwnership {
        connection_id: client.connection_id().to_string(),
        session_id: client.wire_session_id().to_string(),
        vm_id: client.vm_id().to_string(),
    })
}

fn to_wire_overlap(overlap: CronOverlap) -> wire::CronOverlap {
    match overlap {
        CronOverlap::Allow => wire::CronOverlap::Allow,
        CronOverlap::Skip => wire::CronOverlap::Skip,
        CronOverlap::Queue => wire::CronOverlap::Queue,
    }
}

fn from_wire_overlap(overlap: wire::CronOverlap) -> CronOverlap {
    match overlap {
        wire::CronOverlap::Allow => CronOverlap::Allow,
        wire::CronOverlap::Skip => CronOverlap::Skip,
        wire::CronOverlap::Queue => CronOverlap::Queue,
    }
}

fn duration_until(timestamp_ms: u64) -> Duration {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let delay_ms = u128::from(timestamp_ms).saturating_sub(now_ms);
    Duration::from_millis(u64::try_from(delay_ms).unwrap_or(u64::MAX))
}

fn datetime_from_ms(value: u64, field: &str) -> Result<DateTime<Utc>, ClientError> {
    let value = i64::try_from(value)
        .map_err(|_| ClientError::Sidecar(format!("{field} exceeds signed timestamp range")))?;
    DateTime::from_timestamp_millis(value)
        .ok_or_else(|| ClientError::Sidecar(format!("{field} is outside the supported range")))
}

fn cron_rejected(rejected: wire::RejectedResponse, schedule: &str) -> ClientError {
    if rejected.code.contains("invalid_schedule") || rejected.message.contains("[invalid_schedule]")
    {
        return ClientError::InvalidSchedule(schedule.to_string());
    }
    if rejected.code.contains("past_schedule") || rejected.message.contains("[past_schedule]") {
        return ClientError::PastSchedule(schedule.to_string());
    }
    ClientError::Kernel {
        code: rejected.code,
        message: rejected.message,
    }
}

fn unexpected_response(operation: &str, response: wire::ResponsePayload) -> ClientError {
    ClientError::Sidecar(format!("unexpected {operation} response: {response:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_until_due_alarm_is_zero() {
        assert_eq!(duration_until(0), Duration::ZERO);
    }

    #[test]
    fn session_action_wire_shape_matches_typescript() {
        let action = WireCronAction::Session {
            agent_type: "pi".to_string(),
            prompt: "hello".to_string(),
            options: None,
        };
        assert_eq!(
            serde_json::to_value(action).expect("serialize action"),
            serde_json::json!({
                "type": "session",
                "agentType": "pi",
                "prompt": "hello"
            })
        );
    }

    #[test]
    fn cron_events_do_not_invent_missing_sidecar_results() {
        let manager = CronManager::new();
        let missing_duration = manager
            .emit_event(wire::CronEventRecord {
                kind: wire::CronEventKind::Complete,
                job_id: "job".to_string(),
                time_ms: 0,
                duration_ms: None,
                error: None,
            })
            .expect_err("complete event must include duration");
        assert!(missing_duration.to_string().contains("duration_ms"));

        let missing_error = manager
            .emit_event(wire::CronEventRecord {
                kind: wire::CronEventKind::Error,
                job_id: "job".to_string(),
                time_ms: 0,
                duration_ms: Some(1),
                error: None,
            })
            .expect_err("error event must include error");
        assert!(missing_error.to_string().contains("missing error"));
    }

    #[tokio::test]
    async fn control_route_failure_clears_host_alarm_and_fails_event_stream() {
        let manager = CronManager::new();
        let (update_tx, update_rx) = tokio::sync::oneshot::channel();
        let update_tx = Arc::new(parking_lot::Mutex::new(Some(update_tx)));
        manager.set_alarm_handler(Arc::new(move |update| {
            let update_tx = Arc::clone(&update_tx);
            Box::pin(async move {
                if let Some(tx) = update_tx.lock().take() {
                    let _ = tx.send(update);
                }
                Ok(())
            })
        }));
        let mut events = manager.event_tx.subscribe();

        manager.fail_event_route(StreamRouteFailure::Lagged { skipped: 4 });

        let update = update_rx.await.expect("host alarm clear update");
        assert_eq!(update.next_alarm_ms, None);
        assert!(matches!(
            events.recv().await,
            Ok(RoutedStreamEvent::Lagged { skipped: 4 })
        ));
        assert!(matches!(
            manager.ensure_event_route(),
            Err(ClientError::EventStreamLagged { skipped: 4 })
        ));

        // A dispatch that was already in flight when the route failed reaches this same guard at
        // the beginning of `consume_dispatch`, before it can apply a newer alarm or start work.
        let alarm = manager.alarm.lock();
        assert_eq!(alarm.next_alarm_ms, None);
    }

    #[tokio::test]
    async fn host_callback_failure_is_forwarded_exactly_and_releases_route() {
        let manager = Arc::new(CronManager::new());
        let callback_id = manager
            .allocate_callback(Arc::new(|| {
                Box::pin(async { Err(String::from("rust callback failed")) })
            }))
            .expect("allocate callback route");

        let result = run_host_action(
            &manager,
            WireCronAction::Callback {
                callback_id: callback_id.clone(),
            },
        )
        .await;
        assert_eq!(result, Err(String::from("rust callback failed")));

        manager.complete_callback_run(&callback_id);
        assert!(!manager.callbacks.lock().routes.contains_key(&callback_id));
    }

    #[tokio::test]
    async fn unavailable_host_callback_reports_failure_instead_of_success() {
        let manager = CronManager::new();
        let CronAction::Callback { callback } = manager.callback_action("missing") else {
            panic!("expected callback action");
        };
        assert_eq!(
            callback().await,
            Err(String::from(
                "cron callback route is unavailable on this host"
            ))
        );
    }
}
