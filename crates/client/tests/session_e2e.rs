//! Agent session (ACP) e2e against a real `agentos-sidecar`.
//!
//! `create_session` requires an agent package projected into `/opt/agentos`. This suite builds a
//! tiny mock ACP package on the fly so it exercises the real sidecar/session path without depending
//! on a locally built Pi adapter.
//!
//! When a session CAN be created the suite asserts the real TS contract: the session appears in
//! `list_sessions`, `prompt` returns a `PromptResult` (response + accumulated agent text),
//! `on_session_event` streams live `session/update` notifications, and `close_session` removes the
//! session (later prompts report SessionNotFound).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use agentos_client::config::{AgentOsConfig, PackageRef};
use agentos_client::{AgentOs, ClientError, CreateSessionOptions};
use futures::StreamExt;

const MOCK_AGENT_TYPE: &str = "mock-agent";
const MOCK_SESSION_ID: &str = "mock-session-1";
const MOCK_PROMPT_TEXT: &str = "mock-session-pong";
const MOCK_ACP_ADAPTER: &str = r#"
let buffer = "";
function writeMessage(message) { process.stdout.write(JSON.stringify(message) + "\n"); }
function writeResponse(id, result) { writeMessage({ jsonrpc: "2.0", id, result }); }
process.stdin.resume();
process.stdin.on("data", (chunk) => {
  buffer += chunk instanceof Uint8Array ? new TextDecoder().decode(chunk) : String(chunk);
  while (true) {
    const idx = buffer.indexOf("\n");
    if (idx === -1) break;
    const line = buffer.slice(0, idx);
    buffer = buffer.slice(idx + 1);
    if (!line.trim()) continue;
    const msg = JSON.parse(line);
    if (msg.id === undefined) continue;
    switch (msg.method) {
      case "initialize":
        writeResponse(msg.id, {
          protocolVersion: 1,
          agentInfo: { name: "mock-agent", version: "1.0.0" },
          agentCapabilities: { plan_mode: false, tool_calls: false, promptCapabilities: {} },
          modes: { currentModeId: "default", availableModes: [{ id: "default", label: "Default" }] },
          configOptions: [],
        });
        break;
      case "session/new":
        writeResponse(msg.id, {
          sessionId: "__MOCK_SESSION_ID__",
          modes: { currentModeId: "default", availableModes: [{ id: "default", label: "Default" }] },
          configOptions: [],
        });
        break;
      case "session/prompt":
        writeMessage({ jsonrpc: "2.0", method: "session/update", params: {
          sessionId: "__MOCK_SESSION_ID__",
          update: { sessionUpdate: "agent_message_chunk", content: { text: "__MOCK_PROMPT_TEXT__" } } } });
        writeMessage({ jsonrpc: "2.0", method: "session/update", params: {
          sessionId: "__MOCK_SESSION_ID__",
          update: { sessionUpdate: "completed", stopReason: "end_turn" } } });
        writeResponse(msg.id, { stopReason: "end_turn" });
        break;
      case "session/cancel":
        writeResponse(msg.id, {});
        break;
      default:
        writeMessage({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "Method not found" } });
        break;
    }
  }
});
"#;

fn unique_dir(tag: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("agentos-session-e2e-{tag}-{nonce}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_mock_agent_package(root: &Path) -> PathBuf {
    let package = root.join("mock-agent-package");
    std::fs::create_dir_all(package.join("bin")).expect("create package bin");
    std::fs::write(
        package.join("agentos-package.json"),
        r#"{"name":"mock-agent","version":"1.0.0","agent":{"acpEntrypoint":"mock-agent-acp"}}"#,
    )
    .expect("write agentos-package.json");
    let adapter = MOCK_ACP_ADAPTER
        .replace("__MOCK_SESSION_ID__", MOCK_SESSION_ID)
        .replace("__MOCK_PROMPT_TEXT__", MOCK_PROMPT_TEXT);
    let bin = package.join("bin/mock-agent-acp");
    std::fs::write(&bin, format!("#!/usr/bin/env node\n{adapter}\n")).expect("write adapter");
    let mut perms = std::fs::metadata(&bin).expect("stat adapter").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).expect("chmod adapter");
    package
}

async fn try_create_session_with_options(
    os: &AgentOs,
    options: CreateSessionOptions,
) -> Option<String> {
    match os.create_session(MOCK_AGENT_TYPE, options).await {
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
    let package_root = unique_dir("pkg");
    let package_dir = write_mock_agent_package(&package_root);
    common::ensure_sidecar_env();
    let os = agentos_client::AgentOs::create(AgentOsConfig {
        packages: vec![PackageRef {
            dir: Some(package_dir.to_string_lossy().into_owned()),
            tar: None,
        }],
        ..Default::default()
    })
    .await
    .expect("create VM with mock agent package");

    // --- Runtime-independent session surface (no agents/V8 needed) --------------------------------
    // Real assertions against the real sidecar: the registry starts empty, agents are resolved
    // dynamically from the configured `/opt/agentos` package manifests (there is NO hardcoded
    // agent registry), and every session operation on an unknown id reports SessionNotFound.
    assert!(os.list_sessions().is_empty(), "a fresh VM has no sessions");
    let agents = os.list_agents().await.expect("list_agents");
    assert!(
        agents.iter().any(|agent| agent.id == MOCK_AGENT_TYPE),
        "the projected mock package must appear in list_agents: {agents:?}"
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

    let workspace_dir = "/home/agentos/workspace";
    os.mkdir(workspace_dir, Default::default())
        .await
        .expect("create workspace");

    let session_id = match try_create_session_with_options(
        &os,
        CreateSessionOptions {
            cwd: Some(workspace_dir.to_string()),
            ..Default::default()
        },
    )
    .await
    {
        Some(id) => id,
        None => {
            os.shutdown().await.expect("shutdown");
            std::fs::remove_dir_all(&package_root).ok();
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
        result.text.contains(MOCK_PROMPT_TEXT),
        "prompt should accumulate agent_message_chunk text from live session events"
    );
    assert!(
        live_chunk_text
            .as_deref()
            .is_some_and(|text| text.contains(MOCK_PROMPT_TEXT)),
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
    std::fs::remove_dir_all(&package_root).ok();
}
