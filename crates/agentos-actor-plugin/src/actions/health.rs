//! Runtime health for the inspector's observe-only actions (`getRuntimeHealth`
//! / `listSessions`): bounded post-mortem buffers of limit warnings and agent
//! exits plus non-waking snapshots of VM liveness.
//!
//! [`HealthBuffers`] is owned by `actor_worker` NEXT TO the `vm` slot — not
//! inside [`Vars`] — because the buffers must survive VM sleep: the point is
//! reading warnings and crash exits post-mortem while `booted == false`. The
//! pump tasks feeding them are per-VM-lifetime and are tracked/aborted through
//! [`Vars::health_tasks`] like the other pumps (the buffers keep their
//! contents; only the feeds stop).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use agentos_client::{AgentExitEvent, AgentOs, LimitWarning, SessionInfo};
use futures::StreamExt;
use serde::Serialize;

use super::Vars;
use crate::host_ctx::HostCtx;

/// Bounded buffer caps (oldest entries dropped at cap), mirroring the
/// host-shim reference implementation in `rivet-opencode-example/server.ts`.
pub const LIMIT_WARNINGS_CAP: usize = 50;
pub const AGENT_EXITS_CAP: usize = 50;

/// One buffered `limit_warning` structured event (`RuntimeHealth.warnings`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLimitWarningDto {
    /// Epoch milliseconds when the warning was observed host-side.
    pub ts: f64,
    pub limit: String,
    pub category: String,
    pub observed: f64,
    pub capacity: f64,
    pub fill_percent: f64,
}

/// One buffered unexpected adapter exit (`RuntimeHealth.agentExits`), keyed by
/// the client-facing external session id.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAgentExitDto {
    /// Epoch milliseconds when the exit was observed host-side.
    pub ts: f64,
    pub session_id: String,
    pub agent_type: String,
    pub exit_code: Option<i32>,
    pub restart: String,
    pub restart_count: u32,
}

/// One buffered adapter stderr line (`RuntimeHealth.stderrTail`). Always empty
/// at runtime today — see [`runtime_health`] — but kept in the contract so the
/// inspector's `RuntimeHealth` shape stays stable when a stderr feed lands.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStderrLineDto {
    pub ts: f64,
    pub line: String,
}

/// `RuntimeHealth.sidecar`: the non-waking client-side sidecar descriptor
/// subset the status strip renders.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSidecarInfoDto {
    pub state: String,
    pub active_vm_count: u32,
}

/// Reply of `getRuntimeHealth` (TS `RuntimeHealth` in the inspector tabs).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealthDto {
    pub booted: bool,
    /// Live session count; `null` while the VM is asleep.
    pub sessions: Option<u64>,
    /// Sidecar descriptor; `null` while the VM is asleep (the client-side
    /// handle is dropped with the VM, so there is no non-waking source).
    pub sidecar: Option<RuntimeSidecarInfoDto>,
    pub warnings: Vec<RuntimeLimitWarningDto>,
    pub agent_exits: Vec<RuntimeAgentExitDto>,
    pub stderr_tail: Vec<RuntimeStderrLineDto>,
}

/// One row of `listSessions`: a live (loaded-in-VM) session under its
/// client-facing external id.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSessionInfoDto {
    pub session_id: String,
    pub agent_type: String,
}

/// Shared, bounded post-mortem health buffers. Cloning shares the underlying
/// storage (`Arc`), so pump tasks write into the same buffers `actor_worker`
/// snapshots from.
#[derive(Clone, Default)]
pub struct HealthBuffers {
    inner: Arc<Mutex<HealthBuffersInner>>,
}

#[derive(Default)]
struct HealthBuffersInner {
    warnings: VecDeque<RuntimeLimitWarningDto>,
    agent_exits: VecDeque<RuntimeAgentExitDto>,
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn push_capped<T>(buf: &mut VecDeque<T>, item: T, cap: usize) {
    buf.push_back(item);
    while buf.len() > cap {
        buf.pop_front();
    }
}

impl HealthBuffers {
    /// Lock the buffers, recovering from poisoning: the guarded sections are
    /// pure pushes/clones, so a poisoned lock means a panic elsewhere at
    /// worst — health reporting must not cascade it into more panics.
    fn lock(&self) -> std::sync::MutexGuard<'_, HealthBuffersInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Record a runtime limit warning (oldest dropped past
    /// [`LIMIT_WARNINGS_CAP`]).
    pub fn push_limit_warning(&self, warning: &LimitWarning) {
        let dto = RuntimeLimitWarningDto {
            ts: now_ms(),
            limit: warning.limit.clone(),
            category: warning.category.clone(),
            observed: warning.observed,
            capacity: warning.capacity,
            fill_percent: warning.fill_percent,
        };
        push_capped(&mut self.lock().warnings, dto, LIMIT_WARNINGS_CAP);
    }

    /// Record an unexpected adapter exit under the client-facing external
    /// session id (oldest dropped past [`AGENT_EXITS_CAP`]).
    pub fn push_agent_exit(&self, external_session_id: &str, event: &AgentExitEvent) {
        let dto = RuntimeAgentExitDto {
            ts: now_ms(),
            session_id: external_session_id.to_owned(),
            agent_type: event.agent_type.clone(),
            exit_code: event.exit_code,
            restart: event.restart.clone(),
            restart_count: event.restart_count,
        };
        push_capped(&mut self.lock().agent_exits, dto, AGENT_EXITS_CAP);
    }

    fn snapshot(&self) -> (Vec<RuntimeLimitWarningDto>, Vec<RuntimeAgentExitDto>) {
        let inner = self.lock();
        (
            inner.warnings.iter().cloned().collect(),
            inner.agent_exits.iter().cloned().collect(),
        )
    }
}

/// Snapshot the current runtime health WITHOUT waking the VM: liveness fields
/// come from the optional live handle (all client-side state — `list_sessions`
/// and `sidecar().describe()` never round-trip to the sidecar), buffers from
/// the post-mortem store.
pub(crate) fn runtime_health(vm: Option<&AgentOs>, buffers: &HealthBuffers) -> RuntimeHealthDto {
    let (sessions, sidecar) = match vm {
        Some(vm) => {
            let description = vm.sidecar().describe();
            (
                Some(vm.list_sessions().len() as u64),
                Some(RuntimeSidecarInfoDto {
                    state: description.state.as_str().to_owned(),
                    active_vm_count: description.active_vm_count,
                }),
            )
        }
        None => (None, None),
    };
    let (warnings, agent_exits) = buffers.snapshot();
    RuntimeHealthDto {
        booted: vm.is_some(),
        sessions,
        sidecar,
        warnings,
        agent_exits,
        // The Rust client has no per-VM adapter stderr stream (adapter stderr
        // is written straight to host stderr in `deliver_acp_ext_event`; the
        // TS `onAgentStderr` equivalent is a create-time config hook), so the
        // tail is always empty until the client grows a subscription.
        stderr_tail: Vec::new(),
    }
}

/// The live (loaded-in-VM) sessions keyed by EXTERNAL session id, so rows match
/// `listPersistedSessions` records. `[]` while the VM is asleep.
pub(crate) fn list_live_sessions(vm: Option<&AgentOs>, vars: &Vars) -> Vec<LiveSessionInfoDto> {
    match vm {
        Some(vm) => external_session_infos(vm.list_sessions(), vars),
        None => Vec::new(),
    }
}

/// Map the client's live-session infos (keyed by LIVE ACP session id) back to
/// client-facing external ids by inverting `Vars::live_sessions`; a live id
/// with no remap IS the external id (native / not-yet-resumed case).
fn external_session_infos(live: Vec<SessionInfo>, vars: &Vars) -> Vec<LiveSessionInfoDto> {
    let external_by_live: std::collections::HashMap<&str, &str> = vars
        .live_sessions
        .iter()
        .map(|(external, live)| (live.as_str(), external.as_str()))
        .collect();
    live.into_iter()
        .map(|info| LiveSessionInfoDto {
            session_id: external_by_live
                .get(info.session_id.as_str())
                .map(|external| (*external).to_owned())
                .unwrap_or(info.session_id),
            agent_type: info.agent_type,
        })
        .collect()
}

/// Start the per-VM-lifetime health pumps after a fresh VM boot. Currently one
/// pump: the `limit_warning` subscription. Agent exits are teed into the
/// buffers by the existing per-session exit-capture pump
/// (`session::spawn_exit_capture`) rather than double-subscribing here, and
/// there is no stderr pump (see [`runtime_health`]). Tracked in
/// [`Vars::health_tasks`] so VM teardown aborts it; the buffers survive.
pub(crate) fn spawn_health_pumps(
    host: &HostCtx,
    vm: &AgentOs,
    vars: &mut Vars,
    buffers: &HealthBuffers,
) {
    let (mut stream, subscription) = vm.on_limit_warning();
    let host = host.clone();
    let buffers = buffers.clone();
    vars.health_tasks.push(tokio::spawn(async move {
        // Keep the RAII guard alive for the pump's lifetime; dropping the
        // stream (on abort / channel close) is the unsubscribe.
        let _subscription = subscription;
        while let Some(warning) = stream.next().await {
            // Host-visible per repo policy: near-capacity warnings must reach
            // the host log, not just the buffered inspector snapshot.
            host.log_warn(&format!(
                "agent-os limit warning: {} {}/{} ({}%)",
                warning.limit, warning.observed, warning.capacity, warning.fill_percent
            ));
            buffers.push_limit_warning(&warning);
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warning(limit: &str) -> LimitWarning {
        LimitWarning {
            limit: limit.to_owned(),
            category: "queue".to_owned(),
            observed: 80.0,
            capacity: 100.0,
            fill_percent: 80.0,
        }
    }

    fn exit_event(session_id: &str) -> AgentExitEvent {
        AgentExitEvent {
            session_id: session_id.to_owned(),
            agent_type: "pi".to_owned(),
            process_id: "proc-1".to_owned(),
            exit_code: Some(1),
            restart: "restarted".to_owned(),
            restart_count: 1,
            max_restarts: 3,
        }
    }

    #[test]
    fn buffers_drop_oldest_at_cap() {
        let buffers = HealthBuffers::default();
        for i in 0..(LIMIT_WARNINGS_CAP + 5) {
            buffers.push_limit_warning(&warning(&format!("limit-{i}")));
        }
        for i in 0..(AGENT_EXITS_CAP + 5) {
            buffers.push_agent_exit(&format!("session-{i}"), &exit_event("live-1"));
        }
        let (warnings, exits) = buffers.snapshot();
        assert_eq!(warnings.len(), LIMIT_WARNINGS_CAP);
        assert_eq!(warnings[0].limit, "limit-5", "oldest warnings dropped");
        assert_eq!(exits.len(), AGENT_EXITS_CAP);
        assert_eq!(exits[0].session_id, "session-5", "oldest exits dropped");
    }

    #[test]
    fn agent_exit_rows_use_the_external_session_id() {
        let buffers = HealthBuffers::default();
        buffers.push_agent_exit("external-1", &exit_event("live-9"));
        let (_, exits) = buffers.snapshot();
        assert_eq!(exits[0].session_id, "external-1");
    }

    #[test]
    fn runtime_health_reports_unbooted_vm_with_surviving_buffers() {
        let buffers = HealthBuffers::default();
        buffers.push_limit_warning(&warning("vm_open_fds"));
        buffers.push_agent_exit("session-1", &exit_event("session-1"));

        let health = runtime_health(None, &buffers);

        assert!(!health.booted);
        assert_eq!(health.sessions, None);
        assert!(health.sidecar.is_none());
        // The post-mortem point: buffered telemetry stays readable unbooted.
        assert_eq!(health.warnings.len(), 1);
        assert_eq!(health.agent_exits.len(), 1);
        assert!(health.stderr_tail.is_empty());
    }

    #[test]
    fn external_session_infos_invert_the_live_remap() {
        let mut vars = Vars::default();
        vars.live_sessions
            .insert("external-1".to_owned(), "live-1".to_owned());
        let live = vec![
            SessionInfo {
                session_id: "live-1".to_owned(),
                agent_type: "pi".to_owned(),
            },
            SessionInfo {
                session_id: "native-2".to_owned(),
                agent_type: "default".to_owned(),
            },
        ];

        let rows = external_session_infos(live, &vars);

        assert_eq!(rows[0].session_id, "external-1", "remapped to external id");
        assert_eq!(rows[1].session_id, "native-2", "native ids pass through");
    }
}
