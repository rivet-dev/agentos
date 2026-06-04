//! The `AgentOs` struct (all fields from ADR-001 §3), the `create` builder, and the `shutdown`
//! (dispose) teardown.
//!
//! `AgentOs` is `Arc`-cloneable; all interior state lives behind concurrent maps / atomics /
//! channels so `&self` methods never need an outer lock. Module files add only `impl AgentOs` blocks
//! and never introduce new struct fields.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize};
use std::sync::Arc;

use scc::{HashMap as SccHashMap, HashSet as SccHashSet};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

use crate::config::AgentOsConfig;
use crate::cron::CronManager;
use crate::error::ClientError;
use crate::fs::VirtualFileSystem;
use crate::json_rpc::SequencedEvent;
use crate::session::{
    AgentCapabilities, AgentInfo, PermissionReply, PermissionRequest, SessionConfigOption,
    SessionModeState,
};
use crate::sidecar::{AgentOsSidecar, AgentOsSidecarVmLease};
use crate::transport::SidecarTransport;

// ---------------------------------------------------------------------------
// Registry entries
// ---------------------------------------------------------------------------

/// An SDK-spawned process (TS `_processes` value). Keyed by user-facing pid.
pub(crate) struct ProcessEntry {
    pub command: String,
    pub args: Vec<String>,
    pub stdout_tx: broadcast::Sender<Vec<u8>>,
    pub stderr_tx: broadcast::Sender<Vec<u8>>,
    /// Seeded `None`; the already-exited branch fires immediately once it holds `Some(code)`.
    pub exit_tx: watch::Sender<Option<i32>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
}

/// A PTY-backed shell (TS `_shells` value). Keyed by synthetic `shell-N` id.
pub(crate) struct ShellEntry {
    pub pid: u32,
    pub data_tx: broadcast::Sender<Vec<u8>>,
    /// The sidecar-side process id used on the wire.
    pub process_id: String,
}

/// An ACP session (TS `_sessions` value). Keyed by ACP session id.
pub(crate) struct SessionEntry {
    pub agent_type: String,
    pub modes: parking_lot::Mutex<Option<SessionModeState>>,
    pub config_options: parking_lot::Mutex<Vec<SessionConfigOption>>,
    pub capabilities: parking_lot::Mutex<Option<AgentCapabilities>>,
    pub agent_info: parking_lot::Mutex<Option<AgentInfo>>,
    pub config_overrides: parking_lot::Mutex<std::collections::BTreeMap<String, String>>,
    /// Bounded event ring (cap [`crate::ACP_SESSION_EVENT_RETENTION_LIMIT`]).
    pub event_ring: parking_lot::Mutex<VecDeque<SequencedEvent>>,
    /// Highest seen sequence number (ack-based; separate from the truncated ring; negative for
    /// synthetic events).
    pub highest_sequence_number: AtomicI64,
    pub event_tx: broadcast::Sender<SequencedEvent>,
    pub permission_tx: broadcast::Sender<PermissionRequest>,
    pub pending_permission_replies: SccHashMap<String, oneshot::Sender<PermissionReply>>,
    /// Pending prompt resolvers, for cancel prompt-fallback + abort-on-close.
    pub pending_prompt_resolvers: SccHashMap<i64, oneshot::Sender<()>>,
}

// ---------------------------------------------------------------------------
// AgentOs
// ---------------------------------------------------------------------------

/// The high-level client. Cheaply cloneable via `Arc`.
#[derive(Clone)]
pub struct AgentOs {
    inner: Arc<AgentOsInner>,
}

pub(crate) struct AgentOsInner {
    // Transport / connection / VM handle.
    pub(crate) transport: Arc<SidecarTransport>,
    pub(crate) connection_id: String,
    pub(crate) session_id: String,
    pub(crate) vm_id: String,
    pub(crate) request_counter: AtomicI64,
    pub(crate) sidecar_request_counter: AtomicI64,
    pub(crate) max_frame_bytes: AtomicUsize,

    // Process registries.
    pub(crate) processes: SccHashMap<u32, ProcessEntry>,
    pub(crate) process_counter: AtomicU64,

    // Shell registries.
    pub(crate) shells: SccHashMap<String, ShellEntry>,
    pub(crate) shell_counter: AtomicU64,
    pub(crate) pending_shell_exits: SccHashMap<u64, JoinHandle<()>>,
    pub(crate) acp_terminal_pids: SccHashSet<u32>,

    // Session registries.
    pub(crate) sessions: SccHashMap<String, SessionEntry>,
    /// Bounded ordered set (cap [`crate::CLOSED_SESSION_ID_RETENTION_LIMIT`]) for close idempotence.
    pub(crate) closed_session_ids: parking_lot::Mutex<VecDeque<String>>,

    // Cron.
    pub(crate) cron: Arc<CronManager>,

    // Config / lifecycle.
    pub(crate) config: Arc<AgentOsConfig>,
    pub(crate) sidecar: Arc<AgentOsSidecar>,
    pub(crate) sidecar_lease: parking_lot::Mutex<Option<AgentOsSidecarVmLease>>,
    pub(crate) in_process_mounts: SccHashMap<String, Arc<dyn VirtualFileSystem>>,
    pub(crate) disposed: AtomicBool,
}

impl AgentOs {
    /// The sole public VM entry point. Processes software, spawns/authenticates the sidecar, creates
    /// the VM, waits for ready (10s), configures it, takes a lease, and constructs the cron manager
    /// (default [`crate::config::TimerScheduleDriver`]).
    pub async fn create(_options: AgentOsConfig) -> Result<AgentOs, ClientError> {
        todo!("parity: AgentOs::create builder + handshake + createVm + configureVm + lease")
    }

    /// Dispose the VM (= TS `dispose`). Teardown order:
    /// 1. cron dispose
    /// 2. close all sessions (swallow errors)
    /// 3. kill all shells + snapshot pending exits
    /// 4. kill all ACP terminals
    /// 5. drain tracked shell-exit tasks (two-phase, bounded by
    ///    [`crate::SHELL_DISPOSE_TIMEOUT_MS`])
    /// 6. unregister the sidecar event listener
    /// 7. release the lease (or tear down the transport)
    ///
    /// Idempotent (guarded by `disposed`).
    pub async fn shutdown(&self) -> Result<(), ClientError> {
        todo!("parity: AgentOs::shutdown teardown order + two-phase shell drain")
    }

    // --- internal accessors used by sibling impl blocks ---

    pub(crate) fn inner(&self) -> &AgentOsInner {
        &self.inner
    }

    pub(crate) fn transport(&self) -> &Arc<SidecarTransport> {
        &self.inner.transport
    }

    pub(crate) fn connection_id(&self) -> &str {
        &self.inner.connection_id
    }

    pub(crate) fn wire_session_id(&self) -> &str {
        &self.inner.session_id
    }

    pub(crate) fn vm_id(&self) -> &str {
        &self.inner.vm_id
    }

    pub(crate) fn config(&self) -> &Arc<AgentOsConfig> {
        &self.inner.config
    }

    pub(crate) fn cron(&self) -> &Arc<CronManager> {
        &self.inner.cron
    }
}
