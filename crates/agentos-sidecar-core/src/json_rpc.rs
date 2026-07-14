//! Synchronous JSON-RPC-over-stdio primitive for the ACP core.
//!
//! Ported from the native `send_json_rpc_request` (async) to a synchronous loop
//! over the [`AcpHost`] seam: write a newline-delimited JSON request to the agent's
//! stdin, then poll its stdout until the matching response `id` arrives or the
//! timeout elapses. Inbound agent-to-host requests are answered through the
//! [`AcpHost`] seam and are never exposed as notifications.

use serde_json::Value;

use crate::behavior::{
    classify_json_rpc_message, AcpJsonLineAccumulator, AcpJsonRpcMessageKind,
    DEFAULT_ACP_MAX_READ_LINE_BYTES,
};
use crate::host::{AcpHost, AgentOutput};
use crate::AcpCoreError;

pub struct JsonRpcExchange {
    pub response: Value,
    pub notifications: Vec<Value>,
    pub notification_bytes: usize,
}

/// Send a JSON-RPC `request` (with `id == response_id`) to the agent process and
/// block for the matching response. `stdout` accumulates partial output across
/// calls. Returns the parsed response message.
pub fn send_json_rpc<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    request: &Value,
    response_id: i64,
    timeout_ms: u64,
    stdout: &mut String,
    notification_limit: usize,
    notification_bytes_limit: usize,
) -> Result<Value, AcpCoreError> {
    Ok(send_json_rpc_exchange(
        host,
        process_id,
        request,
        response_id,
        timeout_ms,
        stdout,
        notification_limit,
        notification_bytes_limit,
    )?
    .response)
}

pub fn send_json_rpc_exchange<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    request: &Value,
    response_id: i64,
    timeout_ms: u64,
    stdout: &mut String,
    notification_limit: usize,
    notification_bytes_limit: usize,
) -> Result<JsonRpcExchange, AcpCoreError> {
    let mut line = serde_json::to_vec(request).map_err(|error| {
        AcpCoreError::InvalidState(format!("failed to serialize ACP request: {error}"))
    })?;
    line.push(b'\n');
    host.write_stdin(process_id, &line)?;

    let deadline = host.now_ms().saturating_add(timeout_ms);
    let mut notifications = Vec::new();
    let mut notification_bytes = 0usize;
    let mut lines = AcpJsonLineAccumulator::with_buffer(std::mem::take(stdout));
    loop {
        if host.now_ms() >= deadline {
            *stdout = lines.into_retained();
            return Err(AcpCoreError::Execution(format!(
                "timed out waiting for ACP response id={response_id}"
            )));
        }
        let output = match host.poll_output(process_id) {
            Ok(output) => output,
            Err(error) => {
                *stdout = lines.into_retained();
                return Err(error);
            }
        };
        match output {
            Some(AgentOutput::Stdout(chunk)) => {
                let messages = match lines.push_json(&chunk, DEFAULT_ACP_MAX_READ_LINE_BYTES) {
                    Ok(messages) => messages,
                    Err(error) => {
                        *stdout = lines.into_retained();
                        return Err(error);
                    }
                };
                for message in messages {
                    match classify_json_rpc_message(&message)? {
                        AcpJsonRpcMessageKind::InboundRequest => {
                            let response = host.handle_inbound_request(process_id, &message)?;
                            write_json_line(host, process_id, &response)?;
                        }
                        AcpJsonRpcMessageKind::Response
                            if message.get("id").and_then(Value::as_i64) == Some(response_id) =>
                        {
                            *stdout = lines.into_retained();
                            return Ok(JsonRpcExchange {
                                response: message,
                                notifications,
                                notification_bytes,
                            });
                        }
                        AcpJsonRpcMessageKind::Notification => {
                            let message_bytes = serde_json::to_vec(&message)
                                .map_err(|error| {
                                    AcpCoreError::InvalidState(format!(
                                        "failed to size ACP notification: {error}"
                                    ))
                                })?
                                .len();
                            if notifications.len() >= notification_limit
                                || notification_bytes.saturating_add(message_bytes)
                                    > notification_bytes_limit
                            {
                                *stdout = lines.into_retained();
                                return Err(AcpCoreError::LimitExceeded(format!(
                                "ACP exchange notification limit exceeded: at most {notification_limit} notifications / {notification_bytes_limit} bytes may await delivery; drain events after every dispatch or raise the adapter event limits"
                            )));
                            }
                            notification_bytes = notification_bytes.saturating_add(message_bytes);
                            notifications.push(message);
                        }
                        AcpJsonRpcMessageKind::Response => {}
                    }
                }
            }
            Some(AgentOutput::Stderr(_)) => {}
            Some(AgentOutput::Exited(code)) => {
                *stdout = lines.into_retained();
                return Err(AcpCoreError::Execution(format!(
                    "agent process exited (code={code:?}) before responding to id={response_id}"
                )));
            }
            None => {}
        }
    }
}

fn write_json_line<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    message: &Value,
) -> Result<(), AcpCoreError> {
    let mut line = serde_json::to_vec(message).map_err(|error| {
        AcpCoreError::InvalidState(format!("failed to serialize ACP response: {error}"))
    })?;
    line.push(b'\n');
    host.write_stdin(process_id, &line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{ProjectedAgentLaunch, SpawnAgentRequest, SpawnedAgent};
    use serde_json::json;
    use std::collections::VecDeque;

    /// A mock agent that replies to each written request with a JSON-RPC response
    /// echoing the request id (a minimal ACP "echo agent" at the host level).
    #[derive(Default)]
    struct EchoHost {
        pending: VecDeque<AgentOutput>,
        clock: u64,
        inbound_before_response: Option<Value>,
        writes: Vec<Value>,
    }

    impl AcpHost for EchoHost {
        fn resolve_projected_agent(
            &mut self,
            _: &str,
        ) -> Result<Option<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(None)
        }
        fn list_projected_agents(&mut self) -> Result<Vec<ProjectedAgentLaunch>, AcpCoreError> {
            Ok(Vec::new())
        }
        fn spawn_agent(&mut self, _: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
            unreachable!()
        }
        fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
            let request: Value = serde_json::from_slice(chunk.strip_suffix(b"\n").unwrap_or(chunk))
                .expect("valid json line");
            self.writes.push(request.clone());
            if request.get("method").is_none() {
                return Ok(());
            }
            if let Some(inbound) = self.inbound_before_response.take() {
                let mut bytes = serde_json::to_vec(&inbound).unwrap();
                bytes.push(b'\n');
                self.pending.push_back(AgentOutput::Stdout(bytes));
            }
            let id = request.get("id").and_then(Value::as_i64).unwrap();
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}});
            let mut bytes = serde_json::to_vec(&reply).unwrap();
            bytes.push(b'\n');
            self.pending.push_back(AgentOutput::Stdout(bytes));
            Ok(())
        }
        fn close_stdin(&mut self, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn poll_output(&mut self, _: &str) -> Result<Option<AgentOutput>, AcpCoreError> {
            self.clock += 1;
            Ok(self.pending.pop_front())
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

    #[test]
    fn round_trips_a_json_rpc_request_against_a_mock_agent() {
        let mut host = EchoHost::default();
        let mut stdout = String::new();
        let request = json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}});
        let response = send_json_rpc(
            &mut host,
            "proc-1",
            &request,
            7,
            10_000,
            &mut stdout,
            16,
            usize::MAX,
        )
        .expect("round-trip");
        assert_eq!(response.get("id").and_then(Value::as_i64), Some(7));
        assert_eq!(response["result"]["ok"], json!(true));
    }

    #[test]
    fn times_out_when_no_matching_response() {
        let mut host = EchoHost {
            // Reply with a non-matching id so the loop never matches and the clock
            // (incremented per poll) drives it to the deadline.
            pending: VecDeque::new(),
            clock: 0,
            inbound_before_response: None,
            writes: Vec::new(),
        };
        let mut stdout = String::new();
        // Use a tiny timeout; each poll advances now_ms by 1.
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "noop", "params": {}});
        // Drain the auto-reply first so it cannot match (id 1 vs requested 999).
        let err = send_json_rpc(
            &mut host,
            "proc-1",
            &request,
            999,
            3,
            &mut stdout,
            16,
            usize::MAX,
        )
        .expect_err("should time out");
        assert_eq!(err.code(), "execution");
    }

    #[test]
    fn inbound_request_is_answered_without_consuming_notification_capacity() {
        let mut host = EchoHost {
            inbound_before_response: Some(json!({
                "jsonrpc": "2.0",
                "id": "host-1",
                "method": "host/read",
                "params": {},
            })),
            ..EchoHost::default()
        };
        let mut stdout = String::new();
        let exchange = send_json_rpc_exchange(
            &mut host,
            "proc-1",
            &json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}}),
            7,
            10_000,
            &mut stdout,
            0,
            0,
        )
        .expect("inbound request is not a notification overflow");

        assert!(exchange.notifications.is_empty());
        assert_eq!(host.writes.len(), 2);
        assert_eq!(host.writes[1]["id"], "host-1");
        assert_eq!(host.writes[1]["error"]["code"], -32601);
    }

    #[test]
    fn complete_response_without_id_fails_without_a_wire_reply() {
        let mut host = EchoHost {
            inbound_before_response: Some(json!({
                "jsonrpc": "2.0",
                "result": {"ok": true},
            })),
            ..EchoHost::default()
        };
        let mut stdout = String::new();

        let error = match send_json_rpc_exchange(
            &mut host,
            "proc-1",
            &json!({"jsonrpc": "2.0", "id": 7, "method": "initialize", "params": {}}),
            7,
            10_000,
            &mut stdout,
            16,
            usize::MAX,
        ) {
            Ok(_) => panic!("missing response id must fail immediately"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "invalid_state");
        assert!(error.to_string().contains("response is missing id"));
        assert_eq!(
            host.writes.len(),
            1,
            "sidecar must not answer an invalid response with an uncorrelated -32600 frame"
        );
    }
}
