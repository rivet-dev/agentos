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

use agentos_client::AgentOs;
use bytes::Bytes;
use futures::StreamExt;

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

#[tokio::test]
async fn fetch_surface_get_post_and_headers() {
    if !common::require_sidecar("fetch_surface_get_post_and_headers") {
        return;
    }
    let os = common::new_vm_with_wasm_commands().await;

    // --- Runtime-independent: fetch reaches the sidecar and handles a no-listener port ------------
    // Nothing is bound on this guest port, so the port-based fetch must surface an error or a
    // non-success response (never a hang or 2xx). This exercises the full client -> VmFetch ->
    // sidecar wire path without needing a guest HTTP server.
    let probe = http::Request::builder()
        .method(http::Method::GET)
        .uri("http://guest.local/none")
        .body(Bytes::new())
        .expect("build probe request");
    match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        fetch_tolerant(&os, 18079, probe),
    )
    .await
    {
        Ok(Ok(response)) => assert!(
            !response.status().is_success(),
            "fetch to an unbound port must not return a success status, got {}",
            response.status()
        ),
        Ok(Err(_)) => { /* an error is the expected no-listener outcome */ }
        Err(_) => panic!("fetch to an unbound port did not resolve within 8s"),
    }

    if !common::require_wasm_commands(&os, "fetch_surface_get_post_and_headers").await {
        os.shutdown().await.expect("shutdown after local skip");
        return;
    }

    let port: u16 = 18080;
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
    res.end([req.method, req.url, req.headers["x-agent-os-test"] || "", Buffer.concat(chunks).toString()].join("\n"));
  }});
}});
server.listen({port}, "127.0.0.1", () => console.log("READY"));
"#
                ),
            ],
            Default::default(),
        )
        .expect("spawn guest HTTP server");

    let mut server_stdout = os
        .on_process_stdout(server.pid)
        .expect("subscribe guest HTTP server stdout");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut stdout = Vec::new();
        while !String::from_utf8_lossy(&stdout).contains("READY") {
            let Some(chunk) = server_stdout.next().await else {
                panic!("guest HTTP server stdout closed before READY");
            };
            stdout.extend_from_slice(&chunk);
        }
    })
    .await
    .expect("guest HTTP server did not report READY");

    // --- GET: the guest server's response body/status reach the caller ---------------------------
    let get_request = http::Request::builder()
        .method(http::Method::GET)
        .uri("http://guest.local/echo?q=1")
        .body(Bytes::new())
        .expect("build GET request");
    let response = fetch_tolerant(&os, port, get_request)
        .await
        .expect("fetch GET");
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
        .header("x-agent-os-test", "header-value")
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
