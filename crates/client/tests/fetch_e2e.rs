//! Port-based virtual `fetch` e2e against a real `agentos-sidecar`.
//!
//! `fetch` dispatches to a guest HTTP server listening on a port INSIDE the kernel (never the host).
//! Standing up that guest listener requires the V8/JS guest runtime, which may be broken in this
//! environment. This suite fails fast by default when prerequisites are missing; set
//! `AGENT_OS_CLIENT_ALLOW_E2E_SKIPS=1` only for local skip-only runs:
//!
//!   1. The sidecar binary must be present.
//!   2. The guest command/runtime toolchain must be present.
//!   3. `AgentOs::fetch` must be implemented and responsive.
//!
//! When the full path IS available the suite asserts the TS contract: a guest GET returns the
//! server's body/status, a guest POST round-trips its request body, and a custom request header
//! reaches the guest server.

mod common;

use agentos_client::config::{
    PatternPermissionRule, PatternPermissions, PermissionMode, Permissions, RulePermissions,
};
use agentos_client::AgentOs;
use bytes::Bytes;
use futures::{FutureExt, StreamExt};

async fn fetch_tolerant(
    os: &AgentOs,
    port: u16,
    request: http::Request<Bytes>,
) -> anyhow::Result<http::Response<Bytes>> {
    let os = os.clone();
    let handle = tokio::spawn(async move { os.fetch(port, request).await });
    match handle.await {
        Ok(result) => result,
        Err(join_error) if join_error.is_panic() => {
            panic!("AgentOs::fetch panicked; fetch e2e cannot be treated as a skip")
        }
        Err(join_error) => panic!("fetch task did not complete: {join_error}"),
    }
}

async fn fetch_tolerant_with_timeout(
    os: &AgentOs,
    port: u16,
    request: http::Request<Bytes>,
    duration: std::time::Duration,
) -> Option<anyhow::Result<http::Response<Bytes>>> {
    let os = os.clone();
    let mut handle = tokio::spawn(async move { os.fetch(port, request).await });
    tokio::select! {
        joined = &mut handle => Some(match joined {
            Ok(result) => result,
            Err(join_error) if join_error.is_panic() => {
                panic!("AgentOs::fetch panicked; fetch e2e cannot be treated as a skip")
            }
            Err(join_error) => panic!("fetch task did not complete: {join_error}"),
        }),
        _ = tokio::time::sleep(duration) => {
            handle.abort();
            None
        }
    }
}

fn append_output(buffer: &mut String, chunk: Vec<u8>) {
    buffer.push_str(&String::from_utf8_lossy(&chunk));
    const MAX_CAPTURED_OUTPUT: usize = 4096;
    if buffer.len() > MAX_CAPTURED_OUTPUT {
        let excess = buffer.len() - MAX_CAPTURED_OUTPUT;
        buffer.drain(..excess);
    }
}

#[tokio::test]
async fn fetch_surface_get_post_and_headers() {
    if !common::require_sidecar("fetch_surface_get_post_and_headers") {
        return;
    }
    let port: u16 = 18080;
    let os = common::new_vm_with_wasm_commands_and_permissions(Permissions {
        network: Some(PatternPermissions::Rules(RulePermissions {
            default: Some(PermissionMode::Deny),
            rules: vec![PatternPermissionRule {
                mode: PermissionMode::Allow,
                operations: Some(vec!["listen".to_string()]),
                patterns: Some(vec![format!("tcp://127.0.0.1:{port}")]),
            }],
        })),
        ..Default::default()
    })
    .await;

    // --- Runtime-independent: fetch reaches the sidecar and handles a no-listener port ------------
    // Nothing is bound on this guest port, so the port-based fetch must surface an error or a
    // non-success response (never a hang or 2xx). This exercises the full client -> VmFetch ->
    // sidecar wire path without needing a guest HTTP server.
    let probe = http::Request::builder()
        .method(http::Method::GET)
        .uri("http://guest.local/none")
        .body(Bytes::new())
        .expect("build probe request");
    match fetch_tolerant_with_timeout(&os, 18079, probe, std::time::Duration::from_secs(8)).await {
        Some(Ok(response)) => assert!(
            !response.status().is_success(),
            "fetch to an unbound port must not return a success status, got {}",
            response.status()
        ),
        Some(Err(_)) => { /* an error is the expected no-listener outcome */ }
        None => panic!("fetch to an unbound port did not resolve within 8s"),
    }

    if !common::require_wasm_commands(&os, "fetch_surface_get_post_and_headers").await {
        os.shutdown().await.expect("shutdown after local skip");
        return;
    }

    let server = os
        .spawn(
            "node",
            vec![
                "-e".to_string(),
                format!(
                    r#"
const http = require("node:http");
const server = http.createServer((req, res) => {{
  const chunks = [];
  req.on("data", (chunk) => chunks.push(chunk));
  req.on("end", () => {{
    res.writeHead(200, {{ "content-type": "text/plain" }});
    res.end([req.method, req.url, req.headers["x-agentos-test"] || "", Buffer.concat(chunks).toString()].join("\n"));
  }});
}});
server.on("error", (error) => {{
  console.error(`LISTEN_ERROR ${{error && error.stack || error}}`);
  process.exit(1);
}});
server.listen({port}, "0.0.0.0", () => console.log("READY"));
"#
                ),
            ],
            Default::default(),
        )
        .expect("spawn guest HTTP server");
    let mut server_stdout = os
        .on_process_stdout(server.pid)
        .expect("subscribe guest HTTP server stdout");
    let mut server_stderr = os
        .on_process_stderr(server.pid)
        .expect("subscribe guest HTTP server stderr");
    let mut captured_stdout = String::new();
    let mut captured_stderr = String::new();
    let mut last_fetch_result = String::from("not attempted");

    // --- GET: the guest server's response body/status reach the caller ---------------------------
    let response = {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            while let Some(Some(chunk)) = server_stdout.next().now_or_never() {
                append_output(&mut captured_stdout, chunk);
            }
            while let Some(Some(chunk)) = server_stderr.next().now_or_never() {
                append_output(&mut captured_stderr, chunk);
            }
            if let Some(exit_result) = os.wait_process(server.pid).now_or_never() {
                let exit_code = exit_result.expect("wait guest HTTP server");
                panic!(
                    "guest HTTP server exited before fetch became ready (exit {exit_code}); stdout: {captured_stdout:?}; stderr: {captured_stderr:?}; last fetch: {last_fetch_result}"
                );
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "guest HTTP server did not become ready for fetch within 120s; stdout: {captured_stdout:?}; stderr: {captured_stderr:?}; last fetch: {last_fetch_result}"
                );
            }

            let get_request = http::Request::builder()
                .method(http::Method::GET)
                .uri("http://guest.local/echo?q=1")
                .body(Bytes::new())
                .expect("build GET request");
            match fetch_tolerant_with_timeout(
                &os,
                port,
                get_request,
                std::time::Duration::from_secs(2),
            )
            .await
            {
                Some(Ok(response)) if response.status() == http::StatusCode::OK => break response,
                Some(Ok(response)) => {
                    last_fetch_result = format!("status {}", response.status());
                }
                Some(Err(error)) => {
                    last_fetch_result = error.to_string();
                }
                None => {
                    last_fetch_result = String::from("timed out after 2s");
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    };
    assert_eq!(
        response.status(),
        http::StatusCode::OK,
        "guest GET should return 200"
    );
    assert!(
        !response.body().is_empty(),
        "guest GET response body should not be empty"
    );

    // --- POST: the request body round-trips through the guest server ------------------------------
    let post_body = Bytes::from_static(b"fetch-post-body");
    let post_request = http::Request::builder()
        .method(http::Method::POST)
        .uri("http://guest.local/echo-body")
        .header("x-agentos-test", "header-value")
        .body(post_body.clone())
        .expect("build POST request");
    let response = fetch_tolerant(&os, port, post_request)
        .await
        .expect("fetch POST");
    assert_eq!(response.status(), http::StatusCode::OK, "guest POST → 200");
    // An echo server reflects the posted body; the custom header should be observable in the echoed
    // response (header round-trip) since the guest server echoes received headers back.
    let body_text = String::from_utf8_lossy(response.body());
    assert!(
        body_text.contains("fetch-post-body"),
        "guest echo server must reflect the POST body, got: {body_text}"
    );
    assert!(
        body_text.contains("header-value"),
        "the custom request header must reach the guest server (header round-trip)"
    );

    os.kill_process(server.pid).expect("kill guest HTTP server");
    os.shutdown().await.expect("shutdown");
}
