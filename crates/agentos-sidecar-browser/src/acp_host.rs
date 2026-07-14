//! Browser implementation of `agentos_sidecar_core::AcpHost`.
//!
//! Maps the host-free ACP core's synchronous host operations onto the converged
//! browser executor exposed through `BrowserExtensionContext`: agent launch is the
//! two-step `create_javascript_context` + `start_execution`; stdin/kill/output are
//! keyed by `(vm_id, execution_id)`. The browser sidecar centrally demultiplexes
//! its VM-global event stream, so this host only receives stdout/stderr/exit for
//! the requested execution while GuestRequest and SignalState remain owned by the
//! ordinary browser event loop. `now_ms` is a poll counter (each poll is a real
//! kernel poll over the SAB bridge), so timeouts are interpreted as poll budgets.
//!
//! Create, resume, and session requests all use the resumable ACP state machine;
//! this filtered output seam is retained for blocking conformance tests and
//! teardown waits without consuming centrally owned events.

use std::collections::BTreeMap;

use agentos_bridge::{
    CreateJavascriptContextRequest, CreateWasmContextRequest, ExecutionHandleRequest,
    ExecutionSignal, KillExecutionRequest, StartExecutionRequest, WriteExecutionStdinRequest,
};
use agentos_native_sidecar_browser::{
    BrowserExtensionContext, BrowserProjectedAgentLaunch, BrowserSidecarError, ExecutionOutput,
    PollExecutionOutputRequest,
};
use agentos_sidecar_core::host::{
    AcpHost, AgentOutput, ProjectedAgentLaunch, SpawnAgentRequest, SpawnedAgent,
};
use agentos_sidecar_core::AcpCoreError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BrowserAcpOwner {
    pub connection_id: String,
    pub wire_session_id: String,
    pub vm_id: String,
}

impl BrowserAcpOwner {
    pub fn core_owner_id(&self) -> String {
        format!(
            "browser:{}:{}:{}:{}:{}:{}",
            self.connection_id.len(),
            self.connection_id,
            self.wire_session_id.len(),
            self.wire_session_id,
            self.vm_id.len(),
            self.vm_id,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserAcpExecution {
    pub execution_id: String,
    pub context_id: String,
    pub owner: BrowserAcpOwner,
    pub cleaning: bool,
    pub execution_cleanup_complete: bool,
    pub context_cleanup_complete: bool,
}

/// Per-request adapter: borrows the extension context + the session→execution map.
pub struct BrowserAcpHost<'ctx, 'host> {
    ctx: &'ctx mut BrowserExtensionContext<'host>,
    owner: BrowserAcpOwner,
    /// process_id (core's handle) -> execution_id (browser executor handle).
    executions: &'ctx mut BTreeMap<String, BrowserAcpExecution>,
    poll_clock: u64,
}

impl<'ctx, 'host> BrowserAcpHost<'ctx, 'host> {
    pub fn new(
        ctx: &'ctx mut BrowserExtensionContext<'host>,
        owner: BrowserAcpOwner,
        executions: &'ctx mut BTreeMap<String, BrowserAcpExecution>,
    ) -> Self {
        Self {
            ctx,
            owner,
            executions,
            poll_clock: 0,
        }
    }

    fn execution_id(&self, process_id: &str) -> Result<String, AcpCoreError> {
        self.executions
            .get(process_id)
            .filter(|route| route.owner == self.owner && !route.cleaning)
            .map(|route| route.execution_id.clone())
            .ok_or_else(|| {
                AcpCoreError::InvalidState(format!("unknown agent process {process_id}"))
            })
    }

    fn drive_route_cleanup(&mut self, process_id: &str, abort: bool) -> Result<(), AcpCoreError> {
        let Some(route) = self
            .executions
            .get_mut(process_id)
            .filter(|route| route.owner == self.owner)
        else {
            return Ok(());
        };
        route.cleaning = true;
        let mut route = route.clone();
        let mut errors = Vec::new();
        if !route.execution_cleanup_complete {
            let result = if abort {
                self.ctx
                    .abort_execution(&self.owner.vm_id, &route.execution_id)
            } else {
                self.ctx.release_execution(&route.execution_id)
            };
            match result.map_err(map_err) {
                Ok(()) => route.execution_cleanup_complete = true,
                Err(error) => errors.push(error),
            }
        }
        // BrowserSidecar rejects context release while its execution record is
        // live; that record also owns the handles needed to retry kernel/worker
        // cleanup. Context release is therefore a dependent phase.
        if route.execution_cleanup_complete && !route.context_cleanup_complete {
            match self
                .ctx
                .release_context(&self.owner.vm_id, &route.context_id)
                .map_err(map_err)
            {
                Ok(()) => route.context_cleanup_complete = true,
                Err(error) => errors.push(error),
            }
        }
        if route.execution_cleanup_complete && route.context_cleanup_complete {
            self.executions.remove(process_id);
        } else {
            self.executions.insert(process_id.to_string(), route);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AcpCoreError::Cleanup {
                context: "failed to clean up browser ACP route completely",
                errors,
            })
        }
    }
}

fn map_err(error: BrowserSidecarError) -> AcpCoreError {
    match error {
        BrowserSidecarError::InvalidState(message)
        | BrowserSidecarError::InvalidPackage(message)
        | BrowserSidecarError::PackageStateCorrupt(message) => AcpCoreError::InvalidState(message),
        BrowserSidecarError::PackageConflict(message) => AcpCoreError::Conflict(message),
        BrowserSidecarError::LimitExceeded { .. } => AcpCoreError::LimitExceeded(error.to_string()),
        BrowserSidecarError::PackageMount(message)
        | BrowserSidecarError::Kernel(message)
        | BrowserSidecarError::Bridge(message) => AcpCoreError::Execution(message),
        BrowserSidecarError::Cleanup { context, errors } => AcpCoreError::Cleanup {
            context,
            errors: errors.into_iter().map(map_err).collect(),
        },
        BrowserSidecarError::Context { context, source } => AcpCoreError::Context {
            context,
            source: Box::new(map_err(*source)),
        },
    }
}

fn map_projected_agent(agent: BrowserProjectedAgentLaunch) -> ProjectedAgentLaunch {
    ProjectedAgentLaunch {
        id: agent.id,
        adapter_entrypoint: agent.adapter_entrypoint,
        env: agent.env,
        launch_args: agent.launch_args,
    }
}

impl AcpHost for BrowserAcpHost<'_, '_> {
    fn resolve_projected_agent(
        &mut self,
        id: &str,
    ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
        self.ctx
            .resolve_projected_agent(&self.owner.vm_id, id)
            .map(|agent| agent.map(map_projected_agent))
            .map_err(map_err)
    }

    fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
        self.ctx
            .list_projected_agents(&self.owner.vm_id)
            .map(|agents| agents.into_iter().map(map_projected_agent).collect())
            .map_err(map_err)
    }

    fn registered_host_tool_reference(&mut self) -> Result<String, AcpCoreError> {
        // Browser has no agent-to-client host-callback transport yet. Advertising
        // registered tools would invite requests this host can only reject.
        Ok(String::new())
    }

    fn spawn_agent(&mut self, request: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
        let handle = match request.runtime {
            agentos_protocol::generated::v1::AcpRuntimeKind::JavaScript
            | agentos_protocol::generated::v1::AcpRuntimeKind::Python => self
                .ctx
                .create_javascript_context(CreateJavascriptContextRequest {
                    vm_id: self.owner.vm_id.clone(),
                    bootstrap_module: request.entrypoint.clone(),
                }),
            agentos_protocol::generated::v1::AcpRuntimeKind::WebAssembly => {
                self.ctx.create_wasm_context(CreateWasmContextRequest {
                    vm_id: self.owner.vm_id.clone(),
                    module_path: request.entrypoint.clone(),
                })
            }
        }
        .map_err(map_err)?;
        let mut argv = Vec::new();
        if let Some(entrypoint) = &request.entrypoint {
            argv.push(entrypoint.clone());
        }
        argv.extend(request.args.clone());
        let context_id = handle.context_id;
        let started = match self.ctx.start_execution(StartExecutionRequest {
            vm_id: self.owner.vm_id.clone(),
            context_id: context_id.clone(),
            argv,
            env: request.env.clone(),
            cwd: request.cwd.clone().unwrap_or_default(),
        }) {
            Ok(started) => started,
            Err(error) => {
                let start_error = map_err(error);
                return match self
                    .ctx
                    .release_context(&self.owner.vm_id, &context_id)
                    .map_err(map_err)
                {
                    Ok(()) => Err(start_error),
                    Err(cleanup_error) => Err(AcpCoreError::Execution(format!(
                        "{start_error}; failed to release browser agent context after start failure: {cleanup_error}"
                    ))),
                };
            }
        };
        self.executions.insert(
            request.process_id.clone(),
            BrowserAcpExecution {
                execution_id: started.execution_id,
                context_id,
                owner: self.owner.clone(),
                cleaning: false,
                execution_cleanup_complete: false,
                context_cleanup_complete: false,
            },
        );
        Ok(SpawnedAgent {
            process_id: request.process_id,
            pid: None,
        })
    }

    fn bind_session(&mut self, _session_id: &str, _process_id: &str) -> Result<(), AcpCoreError> {
        // The browser executor tracks executions by id; no separate bind needed.
        Ok(())
    }

    fn write_stdin(&mut self, process_id: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
        let execution_id = self.execution_id(process_id)?;
        self.ctx
            .write_stdin(WriteExecutionStdinRequest {
                vm_id: self.owner.vm_id.clone(),
                execution_id,
                chunk: chunk.to_vec(),
            })
            .map_err(map_err)
    }

    fn close_stdin(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
        let execution_id = self.execution_id(process_id)?;
        self.ctx
            .close_stdin(ExecutionHandleRequest {
                vm_id: self.owner.vm_id.clone(),
                execution_id,
            })
            .map_err(map_err)
    }

    fn poll_output(&mut self, process_id: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
        self.poll_clock += 1;
        let execution_id = self.execution_id(process_id)?;
        let event = self
            .ctx
            .poll_execution_output(PollExecutionOutputRequest {
                vm_id: self.owner.vm_id.clone(),
                execution_id,
            })
            .map_err(map_err)?;
        Ok(match event {
            Some(ExecutionOutput::Stdout(chunk)) => Some(AgentOutput::Stdout(chunk.chunk)),
            Some(ExecutionOutput::Stderr(chunk)) => Some(AgentOutput::Stderr(chunk.chunk)),
            Some(ExecutionOutput::Exited(exited)) => {
                Some(AgentOutput::Exited(Some(exited.exit_code)))
            }
            None => None,
        })
    }

    fn kill_agent(&mut self, process_id: &str, signal: &str) -> Result<(), AcpCoreError> {
        let execution_id = self.execution_id(process_id)?;
        let signal = match signal {
            "SIGKILL" => ExecutionSignal::Kill,
            "SIGINT" => ExecutionSignal::Interrupt,
            _ => ExecutionSignal::Terminate,
        };
        self.ctx
            .kill_execution(KillExecutionRequest {
                vm_id: self.owner.vm_id.clone(),
                execution_id,
                signal,
            })
            .map_err(map_err)
    }

    fn abort_agent(&mut self, process_id: &str) -> Result<(), AcpCoreError> {
        self.drive_route_cleanup(process_id, true)
    }

    fn finalize_session_cleanup(
        &mut self,
        _session_id: &str,
        process_id: &str,
    ) -> Result<(), AcpCoreError> {
        self.drive_route_cleanup(process_id, false)
    }

    fn wait_for_exit(
        &mut self,
        process_id: &str,
        timeout_ms: u64,
    ) -> Result<Option<i32>, AcpCoreError> {
        let deadline = self.poll_clock.saturating_add(timeout_ms);
        while self.poll_clock < deadline {
            if let Some(AgentOutput::Exited(code)) = self.poll_output(process_id)? {
                return Ok(code);
            }
        }
        Ok(None)
    }

    fn write_file(&mut self, path: &str, contents: &[u8]) -> Result<(), AcpCoreError> {
        self.ctx
            .write_file(&self.owner.vm_id, path, contents.to_vec())
            .map_err(map_err)
    }

    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, AcpCoreError> {
        self.ctx.read_file(&self.owner.vm_id, path).map_err(map_err)
    }

    fn now_ms(&self) -> u64 {
        self.poll_clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_sidecar_errors_keep_their_acp_semantic_class() {
        let limit = map_err(BrowserSidecarError::LimitExceeded {
            limit: "max_deferred_execution_events_per_vm",
            capacity: 64,
            how_to_raise: "raise max_deferred_execution_events_per_vm",
        });
        assert_eq!(limit.code(), "limit_exceeded");
        assert!(limit
            .to_string()
            .contains("raise max_deferred_execution_events_per_vm"));

        assert_eq!(
            map_err(BrowserSidecarError::PackageConflict(String::from(
                "duplicate"
            )))
            .code(),
            "conflict"
        );
        assert_eq!(
            map_err(BrowserSidecarError::InvalidState(String::from("missing"))).code(),
            "invalid_state"
        );
        assert_eq!(
            map_err(BrowserSidecarError::Bridge(String::from("worker failed"))).code(),
            "execution"
        );
    }
}
