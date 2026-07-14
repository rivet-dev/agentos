//! Synchronous host-operation seam for the ACP core.
//!
//! The ACP orchestration (create/resume session, session/prompt, etc.) needs to
//! spawn the agent process, drive its stdin/stdout, and touch the guest fs. Those
//! are the only host-coupled operations; everything else (JSON-RPC framing, session
//! state, the ACP protocol) is pure logic that belongs in this host-free crate.
//!
//! This synchronous trait is the browser driver's host seam and the blocking
//! conformance strategy. Browser production releases the worker between every
//! adapter-output step through the core's resumable API. Native production drives
//! blocking core dispatch through a bounded async-host broker; neither wrapper
//! owns ACP lifecycle transitions.

use std::collections::BTreeMap;

use agentos_protocol::generated::v1::AcpRuntimeKind;
use serde_json::Value;

use crate::behavior::unsupported_inbound_request_response;
use crate::AcpCoreError;

/// Request to launch an agent adapter process inside the VM.
#[derive(Debug, Clone)]
pub struct SpawnAgentRequest {
    pub process_id: String,
    pub runtime: AcpRuntimeKind,
    pub entrypoint: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
}

/// Result of spawning an agent adapter process.
#[derive(Debug, Clone)]
pub struct SpawnedAgent {
    pub process_id: String,
    pub pid: Option<u32>,
}

/// A single piece of output drained from a running agent process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOutput {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exited(Option<i32>),
}

/// One agent adapter projected into a VM by the sidecar package layer.
///
/// The host supplies the full guest entrypoint because package projection owns
/// its location. The ACP core neither reads package files from the guest nor
/// assumes a particular projection root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedAgentLaunch {
    pub id: String,
    pub adapter_entrypoint: String,
    pub env: BTreeMap<String, String>,
    pub launch_args: Vec<String>,
}

/// Synchronous host operations the ACP core drives. Implemented by the native
/// sidecar (adapting its async runtime) and the browser sidecar (direct, single
/// threaded). Every operation routes through the kernel, the sole enforcement
/// point; this seam never bypasses an applied permission/limit.
pub trait AcpHost {
    /// Resolve one agent from sidecar-owned projected-package state.
    fn resolve_projected_agent(
        &mut self,
        id: &str,
    ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError>;

    /// Enumerate agents from sidecar-owned projected-package state.
    fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError>;

    /// Render the currently registered host tools for prompt injection. Hosts
    /// without callback tooling return the empty reference.
    fn registered_host_tool_reference(&mut self) -> Result<String, AcpCoreError> {
        Ok(String::new())
    }
    /// Handle an agent-to-host JSON-RPC request received while another ACP RPC is
    /// pending. Hosts without an inbound callback transport answer explicitly;
    /// requests must never be reclassified as session notifications.
    fn handle_inbound_request(
        &mut self,
        _process_id: &str,
        request: &Value,
    ) -> Result<Value, AcpCoreError> {
        Ok(unsupported_inbound_request_response(request))
    }
    fn spawn_agent(&mut self, request: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError>;
    /// Associate a spawned process with its ACP session id (for routing/teardown).
    fn bind_session(&mut self, session_id: &str, process_id: &str) -> Result<(), AcpCoreError>;
    fn write_stdin(&mut self, process_id: &str, chunk: &[u8]) -> Result<(), AcpCoreError>;
    fn close_stdin(&mut self, process_id: &str) -> Result<(), AcpCoreError>;
    /// Drain the next available output event, or `None` if nothing is pending.
    fn poll_output(&mut self, process_id: &str) -> Result<Option<AgentOutput>, AcpCoreError>;
    fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError>;
    /// Fail-closed cleanup for an interaction that cannot continue. Browser hosts
    /// override this to kill and release the worker mapping atomically; blocking
    /// hosts default to a hard kill.
    fn abort_agent(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
        self.kill_agent(process_id, "SIGKILL")
    }
    /// Drop host-only routing state after an orderly close has reaped the
    /// adapter. Native hosts do not need a second action; browser hosts remove
    /// their core-process -> executor route without terminating twice.
    fn release_agent_route(&mut self, _process_id: &str) -> Result<(), AcpCoreError> {
        Ok(())
    }
    /// Block (up to `timeout_ms`) for the process to exit. Returns `Some(exit_code)`
    /// if it exited within the window, `None` on timeout. Native: a runtime timeout;
    /// browser: a bounded poll over the sync bridge.
    fn wait_for_exit(
        &mut self,
        process_id: &str,
        timeout_ms: u64,
    ) -> Result<Option<i32>, AcpCoreError>;
    fn write_file(&mut self, path: &str, contents: &[u8]) -> Result<(), AcpCoreError>;
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AcpCoreError>;
    /// Monotonic-ish milliseconds, used for JSON-RPC timeouts. Native: Instant;
    /// browser: performance.now() via the host bridge.
    fn now_ms(&self) -> u64;
}
