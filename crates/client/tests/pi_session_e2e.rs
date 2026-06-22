//! Real Pi agent session e2e against a real `agentos-sidecar`.
//!
//! The HONEST regression gate for the agent-session path. When a built Pi adapter is available it
//! ASSERTS that `create_session("pi")` succeeds and that a real prompt round-trips through the Pi
//! SDK (via a host llmock LLM). It never skips on a feature error — a broken Pi path fails the test.
//! It skips only when the prerequisite is genuinely absent (Pi not built).
//!
//! Module-access dir resolution:
//! - `AGENT_OS_PI_MODULE_CWD` env (a workspace with a built/installed `@rivet-dev/agentos-pi`), else
//! - the repo root, but only when the in-repo adapter is built
//!   (`node_modules/@rivet-dev/agentos-pi/dist/adapter.js`). Build it with `pnpm --dir packages/core
//!   build && pnpm --dir registry/agent/pi build` (core first for types).
//!
//! Background: a real agent SDK exercises module-loading patterns (tsc `__exportStar` CJS barrels,
//! deep pnpm symlink graphs, `__dirname` package self-location) that mock ACP adapters never touch.
//! Those were silently broken; this gate keeps them honest.

mod common;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use agentos_client::config::AgentOsConfig;
use agentos_client::fs::MkdirOptions;
use agentos_client::{AgentOs, CreateSessionOptions};

const LLMOCK_SENTINEL: &str = "PONG_FROM_LLMOCK";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

/// The directory whose `node_modules` holds a built Pi adapter, or `None` when the prerequisite is
/// genuinely absent (so the test skips honestly rather than masking a feature error).
fn pi_module_cwd() -> Option<String> {
    if let Ok(env) = std::env::var("AGENT_OS_PI_MODULE_CWD") {
        if !env.is_empty() {
            return Some(env);
        }
    }
    let root = repo_root();
    let in_repo_adapter = root.join("node_modules/@rivet-dev/agentos-pi/dist/adapter.js");
    in_repo_adapter
        .is_file()
        .then(|| root.to_string_lossy().into_owned())
}

/// A host-side llmock LLM server, killed on drop.
struct LlmockServer {
    child: Child,
    url: String,
}

impl LlmockServer {
    // The spawned child is owned by `LlmockServer`, whose `Drop` kills and
    // waits it; the only path that skips construction is an `assert!` that
    // aborts the test process, so the zombie-process lint does not apply.
    #[allow(clippy::zombie_processes)]
    fn start() -> Self {
        let root = repo_root();
        let mut child = Command::new("node")
            .arg(root.join("crates/client/tests/helpers/llmock-server.mjs"))
            .current_dir(&root)
            .env("LLMOCK_SENTINEL", LLMOCK_SENTINEL)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn host llmock server (is node on PATH?)");
        let stdout = child.stdout.take().expect("llmock stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line).expect("read llmock stdout");
            assert_ne!(read, 0, "llmock exited before printing its URL");
            if let Some(url) = line.trim().strip_prefix("LLMOCK_URL=") {
                return Self {
                    child,
                    url: url.to_string(),
                };
            }
        }
    }

    fn port(&self) -> u16 {
        self.url
            .rsplit(':')
            .next()
            .and_then(|tail| tail.trim_end_matches('/').parse().ok())
            .expect("parse llmock port")
    }
}

impl Drop for LlmockServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// One comprehensive Pi session lifecycle: create -> list -> prompt (real SDK -> host llmock) ->
/// close. A single test (one VM) per the one-test-per-file e2e convention, because the shared
/// sidecar pool tears down when an `AgentOs` from a prior test drops.
#[tokio::test]
async fn pi_session_create_prompt_close() {
    if !common::sidecar_available() {
        eprintln!("skipping pi_session_create_prompt_close: sidecar binary not built");
        return;
    }
    let Some(module_cwd) = pi_module_cwd() else {
        eprintln!(
            "skipping pi_session_create_prompt_close: no built Pi adapter \
             (build it, or set AGENT_OS_PI_MODULE_CWD)"
        );
        return;
    };

    let llmock = LlmockServer::start();
    let url = llmock.url.clone();
    let port = llmock.port();

    common::ensure_sidecar_env();
    let os = AgentOs::create(AgentOsConfig {
        module_access_cwd: Some(module_cwd),
        loopback_exempt_ports: vec![port],
        ..Default::default()
    })
    .await
    .expect("create VM for pi prompt");

    // Pi reads its provider endpoint from ~/.pi/agent/models.json (not just env). Point it at llmock.
    os.mkdir("/home/user/.pi/agent", MkdirOptions { recursive: true })
        .await
        .expect("mkdir .pi/agent");
    let models = serde_json::json!({
        "providers": { "anthropic": { "baseUrl": url, "apiKey": "mock-key" } }
    })
    .to_string();
    os.write_file("/home/user/.pi/agent/models.json", models.as_str())
        .await
        .expect("write models.json");
    os.mkdir("/home/user/workspace", MkdirOptions { recursive: true })
        .await
        .expect("mkdir workspace");

    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), "/home/user".to_string());
    env.insert("ANTHROPIC_API_KEY".to_string(), "mock-key".to_string());
    env.insert("ANTHROPIC_BASE_URL".to_string(), url.clone());
    env.insert("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string());
    let session = os
        .create_session(
            "pi",
            CreateSessionOptions {
                cwd: Some("/home/user/workspace".to_string()),
                env,
                skip_os_instructions: true,
                ..Default::default()
            },
        )
        .await
        .expect("create_session(\"pi\") must succeed against a built Pi tree");
    assert!(
        !session.session_id.is_empty(),
        "session id must be non-empty"
    );
    assert!(
        os.list_sessions()
            .iter()
            .any(|s| s.session_id == session.session_id),
        "created session must appear in list_sessions"
    );

    // The real Pi SDK ACP prompt flow must reach llmock and return its scripted reply.
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        os.prompt(&session.session_id, "Reply with the sentinel."),
    )
    .await
    .expect("prompt timed out")
    .expect("prompt must succeed");

    assert!(
        result.text.contains(LLMOCK_SENTINEL),
        "prompt response must contain the llmock sentinel; got: {:?}",
        result.text
    );

    os.close_session(&session.session_id).ok();
    os.shutdown().await.expect("shutdown");
}
