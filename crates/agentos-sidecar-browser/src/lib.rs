#![forbid(unsafe_code)]

//! Agent OS browser sidecar wrapper.

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::AgentOsBrowserSidecarWasm;

mod acp_host;
#[cfg(any(target_arch = "wasm32", test))]
mod pending_frames;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use agentos_native_sidecar_browser::{
    BrowserExtension, BrowserExtensionContext, BrowserSidecar, BrowserSidecarBridge,
    BrowserSidecarConfig, BrowserSidecarError,
};
use agentos_sidecar_core::{codec, error_response, AcpCore, AcpCoreError};

use crate::acp_host::{BrowserAcpExecution, BrowserAcpHost, BrowserAcpOwner};

/// The browser ACP extension: decodes ACP wire requests, dispatches them through
/// the host-free `agentos-sidecar-core` engine, and drives the agent process via a
/// `BrowserAcpHost` over the converged executor. This crate stays host-free (no
/// tokio / native agentos-native-sidecar) so it compiles to wasm32; the kernel remains
/// the sole enforcement point and all guest syscalls route through the converged
/// sync bridge. `Mutex` (not `RefCell`) satisfies the `Send + Sync` trait bound; the
/// browser runs single-threaded so there is no real contention.
pub struct BrowserAcpExtension {
    core: Arc<Mutex<AcpCore>>,
    /// process_id -> execution_id, persisted across requests for a session.
    executions: Arc<Mutex<BTreeMap<String, BrowserAcpExecution>>>,
}

#[derive(Clone)]
pub struct BrowserAcpDiagnostics {
    core: Arc<Mutex<AcpCore>>,
    executions: Arc<Mutex<BTreeMap<String, BrowserAcpExecution>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowserAcpResourceCounts {
    pub sessions: usize,
    pub pending_interactions: usize,
    pub process_routes: usize,
}

impl BrowserAcpDiagnostics {
    pub fn resource_counts(&self) -> Result<BrowserAcpResourceCounts, BrowserSidecarError> {
        let core = self.core.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP core lock poisoned"))
        })?;
        let executions = self.executions.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP executions lock poisoned"))
        })?;
        Ok(BrowserAcpResourceCounts {
            sessions: core.session_count(),
            pending_interactions: core.pending_interaction_count(),
            process_routes: executions.len(),
        })
    }
}

impl BrowserAcpExtension {
    pub fn new() -> Self {
        Self {
            core: Arc::new(Mutex::new(AcpCore::new())),
            executions: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn diagnostics(&self) -> BrowserAcpDiagnostics {
        BrowserAcpDiagnostics {
            core: Arc::clone(&self.core),
            executions: Arc::clone(&self.executions),
        }
    }

    fn dispose_owners(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        owners: std::collections::BTreeSet<BrowserAcpOwner>,
    ) -> Result<(), BrowserSidecarError> {
        let mut executions = self.executions.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP executions lock poisoned"))
        })?;
        let mut core = self.core.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP core lock poisoned"))
        })?;
        let mut errors = Vec::new();
        for owner in owners {
            let core_owner_id = owner.core_owner_id();
            let mut host = BrowserAcpHost::new(context, owner, &mut executions);
            if let Err(error) = core.dispose_owner(&mut host, &core_owner_id) {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(BrowserSidecarError::InvalidState(format!(
                "failed to dispose browser ACP owner state: {}",
                errors.join("; ")
            )))
        }
    }
}

impl Default for BrowserAcpExtension {
    fn default() -> Self {
        Self::new()
    }
}

fn to_browser_error(error: AcpCoreError) -> BrowserSidecarError {
    BrowserSidecarError::InvalidState(error.to_string())
}

impl BrowserExtension for BrowserAcpExtension {
    fn namespace(&self) -> &str {
        agentos_protocol::ACP_EXTENSION_NAMESPACE
    }

    fn handle_request(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        payload: &[u8],
    ) -> Result<Vec<u8>, BrowserSidecarError> {
        let mut request = codec::decode_request(payload).map_err(to_browser_error)?;
        let connection_id = context
            .connection_id()
            .ok_or_else(|| {
                BrowserSidecarError::InvalidState(String::from(
                    "ACP requests require connection ownership (no connection_id on the request)",
                ))
            })?
            .to_string();
        let vm_id = context
            .vm_id()
            .ok_or_else(|| {
                BrowserSidecarError::InvalidState(String::from(
                    "ACP requests require VM ownership (no vm_id on the request)",
                ))
            })?
            .to_string();
        let wire_session_id = context
            .wire_session_id()
            .ok_or_else(|| {
                BrowserSidecarError::InvalidState(String::from(
                    "ACP requests require wire-session ownership (no session_id on the request)",
                ))
            })?
            .to_string();
        let owner = BrowserAcpOwner {
            connection_id,
            wire_session_id,
            vm_id,
        };
        let core_owner_id = owner.core_owner_id();
        if let agentos_protocol::generated::v1::AcpRequest::AcpCreateSessionRequest(create) =
            &mut request
        {
            let base = context.agent_additional_instructions(&owner.vm_id)?;
            create.additional_instructions = agentos_protocol::combine_additional_instructions(
                base.as_deref(),
                create.additional_instructions.as_deref(),
            );
        }

        let mut executions = self.executions.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP executions lock poisoned"))
        })?;
        let event_capacity = context.event_capacity();
        let mut host = BrowserAcpHost::new(context, owner, &mut executions);
        let mut core = self.core.lock().map_err(|_| {
            BrowserSidecarError::InvalidState(String::from("ACP core lock poisoned"))
        })?;
        // Browser uses the RESUMABLE path (AGENTOS-WEB-ASYNC-AGENTS.md §3.2.1):
        // create_session / session/prompt return AcpPending{processId} and the kernel
        // worker drives them via deliver_agent_output, so the worker never blocks
        // inside pushFrame while the agent makes a mid-turn syscall. A handler error
        // becomes an `AcpErrorResponse` (matching native), not a rejected wire frame.
        let response = match core
            .set_pending_event_limit(&core_owner_id, event_capacity)
            .and_then(|()| core.dispatch_resumable(&mut host, &core_owner_id, request))
        {
            Ok(response) => response,
            Err(error) => error_response(&error),
        };
        for event in core.events_for_delivery(&core_owner_id) {
            let encoded = match codec::encode_event(&event) {
                Ok(encoded) => encoded,
                Err(error) => {
                    eprintln!(
                        "failed to encode a committed browser ACP event; retaining it for retry: {error}"
                    );
                    break;
                }
            };
            if let Err(error) = context.emit_event(encoded) {
                eprintln!(
                    "failed to deliver a committed browser ACP event; retaining it for retry: {error}"
                );
                break;
            }
            if let Err(error) = core.acknowledge_delivered_events(&core_owner_id, 1) {
                eprintln!(
                    "failed to acknowledge a delivered browser ACP event; it may be retried: {error}"
                );
                break;
            }
        }
        codec::encode_response(&response).map_err(to_browser_error)
    }

    fn on_session_disposed(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        connection_id: &str,
        session_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        // BrowserWireDispatcher invokes on_vm_disposed for every VM before this
        // session-level fallback. Remaining routes cover legacy/direct callers;
        // the route map alone is not the authoritative owner registry because an
        // atomic abort deliberately removes its route before fallible cleanup.
        let owners = self
            .executions
            .lock()
            .map_err(|_| {
                BrowserSidecarError::InvalidState(String::from("ACP executions lock poisoned"))
            })?
            .values()
            .filter(|route| {
                route.owner.connection_id == connection_id
                    && route.owner.wire_session_id == session_id
            })
            .map(|route| route.owner.clone())
            .collect::<std::collections::BTreeSet<_>>();
        self.dispose_owners(context, owners)
    }

    fn on_vm_disposed(
        &self,
        context: &mut BrowserExtensionContext<'_>,
        connection_id: &str,
        session_id: &str,
        vm_id: &str,
    ) -> Result<(), BrowserSidecarError> {
        self.dispose_owners(
            context,
            [BrowserAcpOwner {
                connection_id: connection_id.to_string(),
                wire_session_id: session_id.to_string(),
                vm_id: vm_id.to_string(),
            }]
            .into_iter()
            .collect(),
        )
    }
}

pub fn extensions() -> Vec<Box<dyn BrowserExtension>> {
    vec![Box::new(BrowserAcpExtension::new())]
}

pub fn browser_sidecar<B>(
    bridge: B,
    config: BrowserSidecarConfig,
) -> Result<BrowserSidecar<B>, BrowserSidecarError>
where
    B: BrowserSidecarBridge,
    <B as agentos_bridge::BridgeTypes>::Error: std::fmt::Debug,
{
    BrowserSidecar::with_extensions(bridge, config, extensions())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_protocol::ACP_EXTENSION_NAMESPACE;
    use std::collections::VecDeque;

    #[test]
    fn browser_extensions_register_acp_namespace() {
        let extensions = extensions();

        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].namespace(), ACP_EXTENSION_NAMESPACE);
    }

    #[test]
    fn browser_acp_extension_rejects_invalid_payload() {
        // A bogus payload fails ACP wire decoding before any host work — proving the
        // request is now routed through the core codec (not a stub).
        let extension = BrowserAcpExtension::new();
        let mut host = NullBrowserExtensionHost::default();
        let mut context = BrowserExtensionContext::with_ownership(
            &mut host,
            None,
            Some(String::from("conn-a")),
            16,
        );

        let error = extension
            .handle_request(&mut context, b"not-a-valid-acp-frame")
            .expect_err("invalid ACP payload must be rejected");
        assert!(error.to_string().contains("invalid ACP request"));
    }

    #[test]
    fn browser_acp_extension_requires_vm_ownership() {
        // A well-formed request with no vm_id on the context fails closed (the seam
        // that threads vm ownership into the extension is exercised).
        use agentos_protocol::generated::v1::{AcpGetSessionStateRequest, AcpRequest};
        let extension = BrowserAcpExtension::new();
        let mut host = NullBrowserExtensionHost::default();
        let mut context = BrowserExtensionContext::with_ownership(
            &mut host,
            None,
            Some(String::from("conn-a")),
            16,
        );

        let payload = serde_bare::to_vec(&AcpRequest::AcpGetSessionStateRequest(
            AcpGetSessionStateRequest {
                session_id: "s1".into(),
            },
        ))
        .expect("encode");
        let error = extension
            .handle_request(&mut context, &payload)
            .expect_err("missing vm ownership must fail");
        assert!(error.to_string().contains("require VM ownership"));
    }

    #[test]
    fn browser_acp_extension_requires_connection_ownership() {
        use agentos_protocol::generated::v1::{AcpGetSessionStateRequest, AcpRequest};
        let extension = BrowserAcpExtension::new();
        let mut host = NullBrowserExtensionHost::default();
        let mut context = BrowserExtensionContext::with_ownership(
            &mut host,
            Some(String::from("vm-a")),
            None,
            16,
        );
        let payload = serde_bare::to_vec(&AcpRequest::AcpGetSessionStateRequest(
            AcpGetSessionStateRequest {
                session_id: String::from("s1"),
            },
        ))
        .expect("encode");

        let error = extension
            .handle_request(&mut context, &payload)
            .expect_err("missing connection ownership must fail");
        assert!(error.to_string().contains("require connection ownership"));
    }

    #[derive(Default)]
    struct NullBrowserExtensionHost {
        execution_output: VecDeque<agentos_native_sidecar_browser::ExecutionOutput>,
        execution_output_requests: Vec<agentos_native_sidecar_browser::PollExecutionOutputRequest>,
        raw_execution_event_polls: usize,
        projected_agents:
            BTreeMap<String, agentos_native_sidecar_browser::BrowserProjectedAgentLaunch>,
        abort_requests: Vec<(String, String)>,
        stdin_requests: Vec<agentos_bridge::WriteExecutionStdinRequest>,
        close_stdin_requests: Vec<agentos_bridge::ExecutionHandleRequest>,
        kill_requests: Vec<agentos_bridge::KillExecutionRequest>,
        javascript_context_requests: Vec<agentos_bridge::CreateJavascriptContextRequest>,
        wasm_context_requests: Vec<agentos_bridge::CreateWasmContextRequest>,
        start_execution_requests: Vec<agentos_bridge::StartExecutionRequest>,
        released_contexts: Vec<(String, String)>,
        abort_error: Option<String>,
    }

    impl agentos_native_sidecar_browser::BrowserExtensionHost for NullBrowserExtensionHost {
        fn resolve_projected_agent(
            &mut self,
            _vm_id: &str,
            id: &str,
        ) -> Result<
            Option<agentos_native_sidecar_browser::BrowserProjectedAgentLaunch>,
            BrowserSidecarError,
        > {
            Ok(self.projected_agents.get(id).cloned())
        }

        fn list_projected_agents(
            &mut self,
            _vm_id: &str,
        ) -> Result<
            Vec<agentos_native_sidecar_browser::BrowserProjectedAgentLaunch>,
            BrowserSidecarError,
        > {
            Ok(self.projected_agents.values().cloned().collect())
        }

        fn agent_additional_instructions(
            &mut self,
            _vm_id: &str,
        ) -> Result<Option<String>, agentos_native_sidecar_browser::BrowserSidecarError> {
            Ok(None)
        }

        fn registered_host_tool_reference(
            &mut self,
            _vm_id: &str,
        ) -> Result<String, agentos_native_sidecar_browser::BrowserSidecarError> {
            Ok(String::new())
        }

        fn write_file(
            &mut self,
            _vm_id: &str,
            _path: &str,
            _contents: Vec<u8>,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn read_file(&mut self, _vm_id: &str, _path: &str) -> Result<Vec<u8>, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn mkdir(
            &mut self,
            _vm_id: &str,
            _path: &str,
            _recursive: bool,
        ) -> Result<(), BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn read_dir(
            &mut self,
            _vm_id: &str,
            _path: &str,
        ) -> Result<Vec<String>, BrowserSidecarError> {
            unreachable!("test ACP extension does not call browser context")
        }

        fn create_javascript_context(
            &mut self,
            request: agentos_bridge::CreateJavascriptContextRequest,
        ) -> Result<agentos_bridge::GuestContextHandle, BrowserSidecarError> {
            let context_id = format!(
                "javascript-context-{}",
                self.javascript_context_requests.len()
            );
            self.javascript_context_requests.push(request);
            Ok(agentos_bridge::GuestContextHandle {
                context_id,
                runtime: agentos_bridge::GuestRuntime::JavaScript,
            })
        }

        fn create_wasm_context(
            &mut self,
            request: agentos_bridge::CreateWasmContextRequest,
        ) -> Result<agentos_bridge::GuestContextHandle, BrowserSidecarError> {
            let context_id = format!("wasm-context-{}", self.wasm_context_requests.len());
            self.wasm_context_requests.push(request);
            Ok(agentos_bridge::GuestContextHandle {
                context_id,
                runtime: agentos_bridge::GuestRuntime::WebAssembly,
            })
        }

        fn start_execution(
            &mut self,
            request: agentos_bridge::StartExecutionRequest,
        ) -> Result<agentos_bridge::StartedExecution, BrowserSidecarError> {
            let execution_id = format!("execution-{}", self.start_execution_requests.len());
            self.start_execution_requests.push(request);
            Ok(agentos_bridge::StartedExecution { execution_id })
        }

        fn release_context(
            &mut self,
            vm_id: &str,
            context_id: &str,
        ) -> Result<(), BrowserSidecarError> {
            self.released_contexts
                .push((vm_id.to_string(), context_id.to_string()));
            Ok(())
        }

        fn write_stdin(
            &mut self,
            request: agentos_bridge::WriteExecutionStdinRequest,
        ) -> Result<(), BrowserSidecarError> {
            self.stdin_requests.push(request);
            Ok(())
        }

        fn close_stdin(
            &mut self,
            request: agentos_bridge::ExecutionHandleRequest,
        ) -> Result<(), BrowserSidecarError> {
            self.close_stdin_requests.push(request);
            Ok(())
        }

        fn kill_execution(
            &mut self,
            request: agentos_bridge::KillExecutionRequest,
        ) -> Result<(), BrowserSidecarError> {
            self.kill_requests.push(request);
            Ok(())
        }

        fn poll_execution_event(
            &mut self,
            _request: agentos_bridge::PollExecutionEventRequest,
        ) -> Result<Option<agentos_bridge::ExecutionEvent>, BrowserSidecarError> {
            self.raw_execution_event_polls += 1;
            Ok(None)
        }

        fn poll_execution_output(
            &mut self,
            _request: agentos_native_sidecar_browser::PollExecutionOutputRequest,
        ) -> Result<Option<agentos_native_sidecar_browser::ExecutionOutput>, BrowserSidecarError>
        {
            self.execution_output_requests.push(_request);
            Ok(self.execution_output.pop_front())
        }

        fn release_execution(&mut self, _execution_id: &str) -> Result<(), BrowserSidecarError> {
            Ok(())
        }

        fn abort_execution(
            &mut self,
            vm_id: &str,
            execution_id: &str,
        ) -> Result<(), BrowserSidecarError> {
            self.abort_requests
                .push((vm_id.to_string(), execution_id.to_string()));
            match self.abort_error.take() {
                Some(message) => Err(BrowserSidecarError::Bridge(message)),
                None => Ok(()),
            }
        }
    }

    fn browser_owner() -> BrowserAcpOwner {
        BrowserAcpOwner {
            connection_id: String::from("connection-1"),
            wire_session_id: String::from("wire-session-1"),
            vm_id: String::from("vm-1"),
        }
    }

    #[test]
    fn browser_wrapper_retries_retained_events_only_for_the_exact_owner() {
        use agentos_protocol::generated::v1::{
            AcpDeliverAgentOutputRequest, AcpGetSessionStateRequest, AcpRequest, AcpResponse,
            AcpSessionRequest,
        };
        use agentos_sidecar_core::AcpSessionRecord;

        let extension = BrowserAcpExtension::new();
        let owner_a = browser_owner();
        let owner_b = BrowserAcpOwner {
            connection_id: String::from("connection-2"),
            wire_session_id: String::from("wire-session-2"),
            vm_id: String::from("vm-2"),
        };
        {
            let mut core = extension.core.lock().expect("lock core");
            let mut executions = extension.executions.lock().expect("lock executions");
            for (owner, session_id, process_id, execution_id) in [
                (&owner_a, "session-a", "process-a", "execution-a"),
                (&owner_b, "session-b", "process-b", "execution-b"),
            ] {
                core.insert_session(AcpSessionRecord {
                    session_id: session_id.to_string(),
                    owner_connection_id: owner.core_owner_id(),
                    agent_type: String::from("echo"),
                    process_id: process_id.to_string(),
                    pid: None,
                    modes: None,
                    config_options: Vec::new(),
                    agent_capabilities: None,
                    agent_info: None,
                    stdout_buffer: String::new(),
                    next_request_id: 3,
                    closed: false,
                    exit_code: None,
                    pending_preamble: None,
                    restart: None,
                });
                executions.insert(
                    process_id.to_string(),
                    BrowserAcpExecution {
                        execution_id: execution_id.to_string(),
                        context_id: format!("context-{process_id}"),
                        owner: owner.clone(),
                    },
                );
            }
        }

        let mut extension_host = NullBrowserExtensionHost::default();
        let begin_payload = serde_bare::to_vec(&AcpRequest::AcpSessionRequest(AcpSessionRequest {
            session_id: String::from("session-a"),
            method: String::from("session/prompt"),
            params: Some(String::from(r#"{"prompt":[]}"#)),
        }))
        .expect("encode prompt");
        let begin_response = {
            let mut context = BrowserExtensionContext::with_full_ownership(
                &mut extension_host,
                Some(owner_a.vm_id.clone()),
                Some(owner_a.connection_id.clone()),
                Some(owner_a.wire_session_id.clone()),
                1,
            );
            extension
                .handle_request(&mut context, &begin_payload)
                .expect("begin owner A prompt")
        };
        assert!(matches!(
            serde_bare::from_slice::<AcpResponse>(&begin_response).expect("decode pending"),
            AcpResponse::AcpPendingResponse(_)
        ));

        let output = concat!(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"session-a","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}"#,
            "\n"
        )
        .as_bytes()
        .to_vec();
        let complete_payload = serde_bare::to_vec(&AcpRequest::AcpDeliverAgentOutputRequest(
            AcpDeliverAgentOutputRequest {
                process_id: String::from("process-a"),
                chunk: output,
            },
        ))
        .expect("encode output delivery");
        {
            let mut context = BrowserExtensionContext::with_full_ownership(
                &mut extension_host,
                Some(owner_a.vm_id.clone()),
                Some(owner_a.connection_id.clone()),
                Some(owner_a.wire_session_id.clone()),
                1,
            );
            context.emit_event(vec![0]).expect("prefill event capacity");
            extension
                .handle_request(&mut context, &complete_payload)
                .expect("committed response survives event delivery failure");
        }
        assert_eq!(
            extension
                .core
                .lock()
                .expect("inspect retained event")
                .pending_event_count(),
            1
        );

        let owner_b_state = serde_bare::to_vec(&AcpRequest::AcpGetSessionStateRequest(
            AcpGetSessionStateRequest {
                session_id: String::from("session-b"),
            },
        ))
        .expect("encode owner B state");
        {
            let mut context = BrowserExtensionContext::with_full_ownership(
                &mut extension_host,
                Some(owner_b.vm_id.clone()),
                Some(owner_b.connection_id.clone()),
                Some(owner_b.wire_session_id.clone()),
                1,
            );
            extension
                .handle_request(&mut context, &owner_b_state)
                .expect("owner B state");
            context
                .emit_event(vec![1])
                .expect("owner A event must not consume owner B event capacity");
        }
        assert_eq!(
            extension
                .core
                .lock()
                .expect("inspect event after owner B")
                .pending_event_count(),
            1
        );

        let owner_a_state = serde_bare::to_vec(&AcpRequest::AcpGetSessionStateRequest(
            AcpGetSessionStateRequest {
                session_id: String::from("session-a"),
            },
        ))
        .expect("encode owner A state");
        {
            let mut context = BrowserExtensionContext::with_full_ownership(
                &mut extension_host,
                Some(owner_a.vm_id),
                Some(owner_a.connection_id),
                Some(owner_a.wire_session_id),
                1,
            );
            extension
                .handle_request(&mut context, &owner_a_state)
                .expect("owner A retries retained event");
            assert!(
                context.emit_event(vec![2]).is_err(),
                "owner A's retained event must fill its own event capacity"
            );
        }
        assert_eq!(
            extension
                .core
                .lock()
                .expect("inspect acknowledged event")
                .pending_event_count(),
            0
        );
    }

    #[test]
    fn browser_acp_host_polls_only_execution_filtered_output() {
        use agentos_bridge::OutputChunk;
        use agentos_native_sidecar_browser::{ExecutionOutput, PollExecutionOutputRequest};
        use agentos_sidecar_core::host::{AcpHost, AgentOutput};

        let mut extension_host = NullBrowserExtensionHost::default();
        extension_host
            .execution_output
            .push_back(ExecutionOutput::Stdout(OutputChunk {
                vm_id: String::from("vm-1"),
                execution_id: String::from("execution-a"),
                chunk: b"hello".to_vec(),
            }));
        let mut context = BrowserExtensionContext::new(&mut extension_host);
        let owner = browser_owner();
        let mut executions = BTreeMap::from([(
            String::from("process-a"),
            BrowserAcpExecution {
                execution_id: String::from("execution-a"),
                context_id: String::from("context-a"),
                owner: owner.clone(),
            },
        )]);
        let mut acp_host =
            crate::acp_host::BrowserAcpHost::new(&mut context, owner, &mut executions);

        assert_eq!(
            acp_host.poll_output("process-a").expect("poll output"),
            Some(AgentOutput::Stdout(b"hello".to_vec()))
        );
        drop(acp_host);
        drop(context);
        assert_eq!(extension_host.raw_execution_event_polls, 0);
        assert_eq!(
            extension_host.execution_output_requests,
            vec![PollExecutionOutputRequest {
                vm_id: String::from("vm-1"),
                execution_id: String::from("execution-a"),
            }]
        );
    }

    #[test]
    fn browser_acp_host_retains_route_until_abort_cleanup_can_be_retried() {
        use agentos_sidecar_core::host::AcpHost;

        let mut extension_host = NullBrowserExtensionHost {
            abort_error: Some(String::from("injected worker termination failure")),
            ..NullBrowserExtensionHost::default()
        };
        let owner = browser_owner();
        let mut context = BrowserExtensionContext::new(&mut extension_host);
        let mut executions = BTreeMap::from([(
            String::from("process-a"),
            BrowserAcpExecution {
                execution_id: String::from("execution-a"),
                context_id: String::from("context-a"),
                owner: owner.clone(),
            },
        )]);
        let mut host = BrowserAcpHost::new(&mut context, owner, &mut executions);

        let error = host
            .abort_agent("process-a")
            .expect_err("host cleanup error must propagate");
        assert!(error
            .to_string()
            .contains("injected worker termination failure"));
        drop(host);
        assert!(executions.contains_key("process-a"));
        let mut retry_host = BrowserAcpHost::new(&mut context, browser_owner(), &mut executions);
        retry_host
            .abort_agent("process-a")
            .expect("retry cleanup after the one-shot bridge failure");
        drop(retry_host);
        assert!(executions.is_empty());
        drop(context);
        assert_eq!(
            extension_host.abort_requests,
            vec![
                (String::from("vm-1"), String::from("execution-a")),
                (String::from("vm-1"), String::from("execution-a")),
            ]
        );
    }

    #[test]
    fn orderly_close_churn_releases_every_browser_route_without_double_abort() {
        use agentos_bridge::ExecutionExited;
        use agentos_protocol::generated::v1::AcpCloseSessionRequest;
        use agentos_sidecar_core::host::AcpHost;
        use agentos_sidecar_core::AcpSessionRecord;

        const CLOSE_COUNT: usize = 32;
        let owner = browser_owner();
        let core_owner_id = owner.core_owner_id();
        let mut core = AcpCore::new();
        let mut executions = BTreeMap::new();
        let mut extension_host = NullBrowserExtensionHost::default();

        for index in 0..CLOSE_COUNT {
            let session_id = format!("session-{index}");
            let process_id = format!("process-{index}");
            let execution_id = format!("execution-{index}");
            core.insert_session(AcpSessionRecord {
                session_id: session_id.clone(),
                owner_connection_id: core_owner_id.clone(),
                agent_type: String::from("echo"),
                process_id: process_id.clone(),
                pid: None,
                modes: None,
                config_options: Vec::new(),
                agent_capabilities: None,
                agent_info: None,
                stdout_buffer: String::new(),
                next_request_id: 3,
                closed: false,
                exit_code: None,
                pending_preamble: None,
                restart: None,
            });
            executions.insert(
                process_id.clone(),
                BrowserAcpExecution {
                    execution_id: execution_id.clone(),
                    context_id: format!("context-{index}"),
                    owner: owner.clone(),
                },
            );
            extension_host.execution_output.push_back(
                agentos_native_sidecar_browser::ExecutionOutput::Exited(ExecutionExited {
                    vm_id: owner.vm_id.clone(),
                    execution_id,
                    exit_code: 0,
                }),
            );

            let mut context = BrowserExtensionContext::new(&mut extension_host);
            let mut host = BrowserAcpHost::new(&mut context, owner.clone(), &mut executions);
            core.close_session(
                &mut host,
                &core_owner_id,
                &AcpCloseSessionRequest {
                    session_id: session_id.clone(),
                },
            )
            .expect("orderly browser close");
            host.release_agent_route(&process_id)
                .expect("absent browser route release is idempotent");
            drop(host);
            drop(context);

            assert!(
                executions.is_empty(),
                "close {index} leaked a process route"
            );
            assert_eq!(core.session_count(), 0, "close {index} leaked core state");
        }

        assert_eq!(extension_host.close_stdin_requests.len(), CLOSE_COUNT);
        assert_eq!(extension_host.kill_requests.len(), CLOSE_COUNT);
        assert!(
            extension_host.abort_requests.is_empty(),
            "orderly close must not double-abort the execution"
        );
    }

    #[test]
    fn browser_acp_host_uses_the_ordinary_browser_context_for_each_runtime() {
        use agentos_protocol::generated::v1::AcpRuntimeKind;
        use agentos_sidecar_core::host::{AcpHost, SpawnAgentRequest};

        let owner = browser_owner();
        let mut extension_host = NullBrowserExtensionHost::default();
        let mut executions = BTreeMap::new();
        let mut context = BrowserExtensionContext::new(&mut extension_host);
        let mut host = BrowserAcpHost::new(&mut context, owner, &mut executions);

        for (index, runtime) in [
            AcpRuntimeKind::JavaScript,
            AcpRuntimeKind::Python,
            AcpRuntimeKind::WebAssembly,
        ]
        .into_iter()
        .enumerate()
        {
            let process_id = format!("process-{index}");
            host.spawn_agent(SpawnAgentRequest {
                process_id,
                runtime,
                entrypoint: Some(format!("/opt/agentos/bin/adapter-{index}")),
                command: None,
                args: vec![String::from("--fixture")],
                env: BTreeMap::new(),
                cwd: Some(String::from("/workspace")),
            })
            .expect("spawn browser ACP adapter");
        }
        drop(host);
        drop(context);

        assert_eq!(extension_host.javascript_context_requests.len(), 2);
        assert_eq!(extension_host.wasm_context_requests.len(), 1);
        assert_eq!(extension_host.start_execution_requests.len(), 3);
        assert_eq!(
            extension_host.javascript_context_requests[0].bootstrap_module,
            Some(String::from("/opt/agentos/bin/adapter-0"))
        );
        assert_eq!(
            extension_host.javascript_context_requests[1].bootstrap_module,
            Some(String::from("/opt/agentos/bin/adapter-1"))
        );
        assert_eq!(
            extension_host.wasm_context_requests[0].module_path,
            Some(String::from("/opt/agentos/bin/adapter-2"))
        );
        assert_eq!(
            extension_host.start_execution_requests[2].argv,
            vec![
                String::from("/opt/agentos/bin/adapter-2"),
                String::from("--fixture")
            ]
        );
        assert_eq!(executions.len(), 3);
    }

    #[test]
    fn browser_acp_host_delegates_projected_agent_resolution_and_listing() {
        use agentos_native_sidecar_browser::BrowserProjectedAgentLaunch;
        use agentos_sidecar_core::host::AcpHost;

        let projected = BrowserProjectedAgentLaunch {
            id: String::from("pi"),
            adapter_entrypoint: String::from("/opt/agentos/bin/pi-acp"),
            env: [(String::from("PI_DEFAULT"), String::from("yes"))]
                .into_iter()
                .collect(),
            launch_args: vec![String::from("--fixture")],
        };
        let mut extension_host = NullBrowserExtensionHost {
            projected_agents: BTreeMap::from([(projected.id.clone(), projected)]),
            ..NullBrowserExtensionHost::default()
        };
        let mut context = BrowserExtensionContext::new(&mut extension_host);
        let mut executions = BTreeMap::new();
        let mut acp_host =
            crate::acp_host::BrowserAcpHost::new(&mut context, browser_owner(), &mut executions);

        let resolved = acp_host
            .resolve_projected_agent("pi")
            .expect("resolve projected agent")
            .expect("pi projected");
        assert_eq!(resolved.id, "pi");
        assert_eq!(resolved.adapter_entrypoint, "/opt/agentos/bin/pi-acp");
        assert_eq!(
            resolved.env.get("PI_DEFAULT").map(String::as_str),
            Some("yes")
        );
        assert_eq!(resolved.launch_args, vec!["--fixture"]);
        assert_eq!(
            acp_host
                .list_projected_agents()
                .expect("list projected agents"),
            vec![resolved]
        );
    }
}
