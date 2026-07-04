//! Agent session (ACP) e2e against a real `agentos-sidecar`.
//!
//! `create_session` requires agent adapters + a mock LLM + V8 execution. In this environment the
//! client. This suite fails fast by default when session creation is unavailable; set
//! `AGENT_OS_CLIENT_ALLOW_E2E_SKIPS=1` only for local skip-only runs.
//!
//! When a session CAN be created the suite asserts the real TS contract: the session appears in
//! `list_sessions`, `prompt` returns a `PromptResult` (response + accumulated agent text),
//! `on_session_event` streams live `session/update` notifications, and `close_session` removes the
//! session (later prompts report SessionNotFound).

mod common;

use std::collections::BTreeMap;

use agentos_client::fs::FileContent;
use agentos_client::{AgentOs, ClientError, CreateSessionOptions};
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

struct MockAnthropic {
    url: String,
    port: u16,
    task: JoinHandle<()>,
}

impl MockAnthropic {
    fn stop(self) {
        self.task.abort();
    }
}

async fn start_mock_anthropic() -> MockAnthropic {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock anthropic server");
    let port = listener.local_addr().expect("mock server address").port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = [0_u8; 8192];
                let _ = socket.read(&mut buffer).await;
                let body = r#"{"id":"msg_mock","type":"message","role":"assistant","model":"claude-3-5-sonnet-20241022","content":[{"type":"text","text":"PONG"}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });

    MockAnthropic {
        url: format!("http://127.0.0.1:{port}"),
        port,
        task,
    }
}

async fn try_create_session_with_options(
    os: &AgentOs,
    options: CreateSessionOptions,
) -> Option<String> {
    match os.create_session("pi", options).await {
        Ok(session) => Some(session.session_id),
        Err(error) => {
            if common::allow_local_e2e_skips() {
                eprintln!(
                    "skipping session e2e: create_session unavailable in this environment ({error})"
                );
                None
            } else {
                panic!("create_session unavailable; this e2e cannot pass as a skip: {error}");
            }
        }
    }
}

fn agent_message_chunk_text(notification: &agentos_client::JsonRpcNotification) -> Option<&str> {
    let params = notification.params.as_ref()?;
    let update = params.get("update").unwrap_or(params);
    if update.get("sessionUpdate").and_then(|value| value.as_str()) != Some("agent_message_chunk") {
        return None;
    }
    update
        .get("content")
        .and_then(|content| content.get("text"))
        .and_then(|value| value.as_str())
}

#[tokio::test]
async fn session_surface_create_prompt_events_close() {
    if !common::require_sidecar("session_surface_create_prompt_events_close") {
        return;
    }
    let mock = start_mock_anthropic().await;
    let os = common::new_vm_with_loopback_ports(vec![mock.port]).await;

    // --- Runtime-independent session surface (no agents/V8 needed) --------------------------------
    // Real assertions against the real sidecar: the registry starts empty, agents are resolved
    // dynamically from the configured `/opt/agentos` package manifests (there is NO hardcoded
    // agent registry), and every session operation on an unknown id reports SessionNotFound.
    assert!(os.list_sessions().is_empty(), "a fresh VM has no sessions");
    // This VM configures no `/opt/agentos` agent packages, so no agents are listed. `list_agents`
    // is a sidecar ACP RPC that enumerates the projected `/opt/agentos` packages; with none
    // projected it returns empty.
    let agents = os.list_agents().await.expect("list_agents");
    assert!(
        agents.is_empty(),
        "with no agent packages configured, list_agents must be empty (dynamic resolution)"
    );
    assert!(
        matches!(
            os.close_session("nope"),
            Err(ClientError::SessionNotFound(_))
        ),
        "close_session(unknown) must return SessionNotFound"
    );
    assert!(
        os.prompt("nope", "x")
            .await
            .unwrap_err()
            .downcast_ref::<ClientError>()
            .map(|error| matches!(error, ClientError::SessionNotFound(_)))
            .unwrap_or(false),
        "prompt(unknown) must return SessionNotFound"
    );

    let home_dir = "/home/agentos";
    let workspace_dir = "/home/agentos/workspace";
    os.mkdir("/home/agentos/.pi/agent", Default::default())
        .await
        .expect("create pi config directory");
    os.mkdir(workspace_dir, Default::default())
        .await
        .expect("create workspace");
    os.write_file(
        "/home/agentos/.pi/agent/models.json",
        FileContent::Text(format!(
            r#"{{
  "providers": {{
    "anthropic": {{
      "baseUrl": "{}",
      "apiKey": "mock-key"
    }}
  }}
}}"#,
            mock.url
        )),
    )
    .await
    .expect("write pi model config");

    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home_dir.to_string());
    env.insert("ANTHROPIC_API_KEY".to_string(), "mock-key".to_string());
    env.insert("ANTHROPIC_BASE_URL".to_string(), mock.url.clone());
    env.insert("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string());

    let session_id = match try_create_session_with_options(
        &os,
        CreateSessionOptions {
            cwd: Some(workspace_dir.to_string()),
            env,
            ..Default::default()
        },
    )
    .await
    {
        Some(id) => id,
        None => {
            os.shutdown().await.expect("shutdown");
            mock.stop();
            return;
        }
    };

    // --- list_sessions: the new session is registered --------------------------------------------
    assert!(
        os.list_sessions()
            .iter()
            .any(|s| s.session_id == session_id),
        "created session must appear in list_sessions"
    );

    // --- on_session_event: subscribe before prompting so prompt-time chunks are observed ---------
    let (mut events, _sub) = os
        .on_session_event(&session_id)
        .expect("on_session_event for live session");

    // --- prompt: returns a PromptResult (response + accumulated agent text) -----------------------
    let result = os
        .prompt(&session_id, "Say the word PONG and nothing else.")
        .await
        .expect("prompt");
    // The JSON-RPC response is returned even when it carries an error; here a healthy mock should
    // produce a non-error response. We assert the response shape rather than exact model text.
    assert_eq!(result.response.jsonrpc, "2.0");
    assert!(
        result.response.error.is_none(),
        "mock-backed prompt should not return a JSON-RPC error: {:?}",
        result.response.error
    );

    // The first agent_message_chunk must arrive live because the subscription was created before
    // prompt.
    let live_chunk_text = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(notification) = events.next().await {
            if let Some(text) = agent_message_chunk_text(&notification) {
                return Some(text.to_string());
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    assert!(
        !result.text.is_empty(),
        "prompt should accumulate agent_message_chunk text from live session events"
    );
    assert!(
        live_chunk_text
            .as_deref()
            .is_some_and(|text| !text.is_empty()),
        "on_session_event should stream a live agent_message_chunk during prompt"
    );

    // --- close_session: removes the session; later prompts report SessionNotFound -----------------
    os.close_session(&session_id).expect("close_session");
    // close_session is fire-and-forget; the in-memory registry removal is synchronous in the close
    // path, but the detached internal close runs on a task. Poll briefly for the deregistration.
    let gone = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if matches!(
                os.prompt(&session_id, "ignored").await,
                Err(error) if error.downcast_ref::<ClientError>()
                    .map(|e| matches!(e, ClientError::SessionNotFound(_)))
                    .unwrap_or(false)
            ) {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or(false);
    assert!(
        gone,
        "after close_session, prompting the session must report SessionNotFound"
    );

    os.shutdown().await.expect("shutdown");
    mock.stop();
}
