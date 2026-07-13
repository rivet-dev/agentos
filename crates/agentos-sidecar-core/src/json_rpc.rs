//! Synchronous JSON-RPC-over-stdio primitive for the ACP core.
//!
//! Ported from the native `send_json_rpc_request` (async) to a synchronous loop
//! over the [`AcpHost`] seam: write a newline-delimited JSON request to the agent's
//! stdin, then poll its stdout until the matching response `id` arrives or the
//! timeout elapses. This first cut handles response matching (enough for the
//! initialize/create round-trip); inbound agent->client callbacks and notification
//! forwarding are a follow-up that layers on the same loop.

use serde_json::Value;

use crate::host::{AcpHost, AgentOutput};
use crate::AcpCoreError;

pub struct JsonRpcExchange {
    pub response: Value,
    pub notifications: Vec<Value>,
}

/// Drain any complete (newline-terminated) lines from `buffer`, returning them
/// with the trailing newline removed and leaving any partial trailing line.
fn drain_lines(buffer: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(idx) = buffer.find('\n') {
        let line: String = buffer.drain(..=idx).collect();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    lines
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
) -> Result<Value, AcpCoreError> {
    Ok(
        send_json_rpc_exchange(host, process_id, request, response_id, timeout_ms, stdout)?
            .response,
    )
}

pub fn send_json_rpc_exchange<H: AcpHost>(
    host: &mut H,
    process_id: &str,
    request: &Value,
    response_id: i64,
    timeout_ms: u64,
    stdout: &mut String,
) -> Result<JsonRpcExchange, AcpCoreError> {
    let mut line = serde_json::to_vec(request).map_err(|error| {
        AcpCoreError::InvalidState(format!("failed to serialize ACP request: {error}"))
    })?;
    line.push(b'\n');
    host.write_stdin(process_id, &line)?;

    let deadline = host.now_ms().saturating_add(timeout_ms);
    let mut notifications = Vec::new();
    loop {
        if host.now_ms() >= deadline {
            return Err(AcpCoreError::Execution(format!(
                "timed out waiting for ACP response id={response_id}"
            )));
        }
        match host.poll_output(process_id)? {
            Some(AgentOutput::Stdout(chunk)) => {
                stdout.push_str(&String::from_utf8_lossy(&chunk));
                for line in drain_lines(stdout) {
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if message.get("id").and_then(Value::as_i64) == Some(response_id) {
                        return Ok(JsonRpcExchange {
                            response: message,
                            notifications,
                        });
                    }
                    if message.get("method").and_then(Value::as_str).is_some() {
                        notifications.push(message);
                    }
                }
            }
            Some(AgentOutput::Stderr(_)) => {}
            Some(AgentOutput::Exited(code)) => {
                return Err(AcpCoreError::Execution(format!(
                    "agent process exited (code={code:?}) before responding to id={response_id}"
                )));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{SpawnAgentRequest, SpawnedAgent};
    use serde_json::json;
    use std::collections::VecDeque;

    /// A mock agent that replies to each written request with a JSON-RPC response
    /// echoing the request id (a minimal ACP "echo agent" at the host level).
    #[derive(Default)]
    struct EchoHost {
        pending: VecDeque<AgentOutput>,
        clock: u64,
    }

    impl AcpHost for EchoHost {
        fn spawn_agent(&mut self, _: SpawnAgentRequest) -> Result<SpawnedAgent, AcpCoreError> {
            unreachable!()
        }
        fn bind_session(&mut self, _: &str, _: &str) -> Result<(), AcpCoreError> {
            Ok(())
        }
        fn write_stdin(&mut self, _: &str, chunk: &[u8]) -> Result<(), AcpCoreError> {
            let request: Value = serde_json::from_slice(chunk.strip_suffix(b"\n").unwrap_or(chunk))
                .expect("valid json line");
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
        let response = send_json_rpc(&mut host, "proc-1", &request, 7, 10_000, &mut stdout)
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
        };
        let mut stdout = String::new();
        // Use a tiny timeout; each poll advances now_ms by 1.
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "noop", "params": {}});
        // Drain the auto-reply first so it cannot match (id 1 vs requested 999).
        let err = send_json_rpc(&mut host, "proc-1", &request, 999, 3, &mut stdout)
            .expect_err("should time out");
        assert_eq!(err.code(), "execution");
    }
}
