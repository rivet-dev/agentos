use agentos_driver_tokio::{DriverConfig, TokioDriver};
use agentos_executor_contract::backend::{
    bounded_execution_event_channel, ExecutionEvent, HostCallReply, HostServiceError, PayloadLimit,
};
use agentos_executor_contract::host::{
    FilesystemOperation, HostOperation, HostProcessContext, NetworkOperation,
    ProcessHostCapabilitySet,
};
use agentos_executor_obscura_experiment::ExecutorBrowserHost;
use base64::Engine;
use obscura_browser::{BrowserContext, Page};
use obscura_net::host::BrowserHost;
use obscura_net::ObscuraHttpClient;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

// This is a transport-contract test, not an assertion that the full Obscura
// executor lifecycle or native memory accounting is ready for hostile input.
#[test]
fn real_obscura_uses_executor_capabilities_and_shared_v8() {
    agentos_executor_v8_runtime::isolate::init_v8_platform();
    agentos_executor_v8_runtime::isolate::prepare_current_thread();
    let driver = TokioDriver::process(&DriverConfig::default()).expect("shared driver");
    let local = tokio::task::LocalSet::new();
    driver.block_on(local.run_until(async {
        let context = HostProcessContext {
            generation: 17,
            pid: 41,
        };
        let notify = Arc::new(Notify::new());
        let wake = notify.clone();
        let (events, receiver) = bounded_execution_event_channel(
            context,
            8,
            PayloadLimit::new("test.queueBytes", 1024 * 1024).unwrap(),
            Arc::new(move || wake.notify_one()),
        )
        .unwrap();
        let host = Arc::new(
            ExecutorBrowserHost::new(
                ProcessHostCapabilitySet::from_event_submission(events),
                512 * 1024,
            )
            .unwrap(),
        );
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
        let serve = async {
            let mut files = Vec::new();
            let mut requests = Vec::new();
            loop {
                while let Some(event) = receiver.try_recv().unwrap() {
                    let ExecutionEvent::HostCall { operation, reply } = event else {
                        panic!("unexpected event");
                    };
                    assert_eq!(reply.identity().pid, 41);
                    assert_eq!(reply.identity().generation, 17);
                    let response = match operation {
                        HostOperation::Filesystem(FilesystemOperation::ReadFileAt {
                            path,
                            max_bytes,
                            ..
                        }) => {
                            assert!(max_bytes.get() <= 512 * 1024);
                            files.push(path.as_str().to_owned());
                            if path.as_str() == "/virtual/page.html" {
                                Ok(HostCallReply::Raw(
                                    b"<!doctype html><title>Virtual page</title><p id='proof'>kernel bytes</p><script>document.title = 'Executed';</script>"
                                        .to_vec(),
                                ))
                            } else {
                                Err(HostServiceError::new("EACCES", "fixture file denied"))
                            }
                        }
                        HostOperation::Network(NetworkOperation::HttpRequest { url, .. }) => {
                            requests.push(url.as_str().to_owned());
                            if url.as_str() == "https://obscura.gameinc.io/fixture" {
                                Ok(HostCallReply::Json(serde_json::json!({
                                    "status": 200,
                                    "url": url.as_str(),
                                    "headers": { "content-type": "text/html" },
                                    "bodyBase64": base64::engine::general_purpose::STANDARD.encode(
                                        b"<title>Network fixture</title><script>window.answer = 6 * 7;</script>"
                                    ),
                                })))
                            } else if url.as_str() == "https://obscura.gameinc.io/script-fetch" {
                                Ok(HostCallReply::Json(serde_json::json!({
                                    "status": 200,
                                    "headers": { "content-type": "text/plain" },
                                    "bodyBase64": base64::engine::general_purpose::STANDARD.encode(b"script fetched through executor"),
                                })))
                            } else if url.as_str() == "https://obscura.gameinc.io/redirect" {
                                Ok(HostCallReply::Json(serde_json::json!({
                                    "status": 302,
                                    "headers": { "location": "https://denied.gameinc.io/redirect-target" },
                                    "bodyBase64": "",
                                })))
                            } else {
                                Err(HostServiceError::new("EACCES", "fixture network denied"))
                            }
                        }
                        _ => panic!("unexpected host operation"),
                    };
                    match response {
                        Ok(value) => reply.succeed(value).unwrap(),
                        Err(error) => reply.fail(error).unwrap(),
                    }
                }
                tokio::select! {
                    _ = notify.notified() => {}
                    _ = &mut done_rx => break,
                }
            }
            (files, requests)
        };
        let browse = async {
            let mut context = BrowserContext::new("test".into());
            context.allow_file_access = true;
            context.allow_private_network = true;
            context.http_client = Arc::new(ObscuraHttpClient::with_host(
                context.cookie_jar.clone(),
                host.clone(),
            ));
            let mut page = Page::new("test".into(), Arc::new(context));
            page.navigate("file:///virtual/page.html").await.unwrap();
            assert_eq!(page.evaluate("document.title"), "Executed");
            assert_eq!(
                page.evaluate("document.querySelector('#proof').textContent"),
                "kernel bytes"
            );
            let denied = page.navigate("file:///not-granted.html").await.unwrap_err();
            assert!(denied.to_string().contains("EACCES"), "{denied}");
            page.navigate("https://obscura.gameinc.io/fixture").await.unwrap();
            assert_eq!(page.evaluate("window.answer").as_f64(), Some(42.0));
            let value = page.evaluate_for_cdp_with_timeout(
                "fetch('/script-fetch').then(response => response.text())",
                true,
                true,
                1000,
            ).await.unwrap();
            assert!(!value.thrown, "{value:?}");
            assert_eq!(value.value, Some(serde_json::json!("script fetched through executor")));
            let denied = page.navigate("https://obscura.gameinc.io/redirect").await.unwrap_err();
            assert!(denied.to_string().contains("EACCES"), "{denied}");
            let denied = host
                .request(
                    reqwest::Client::new()
                        .get("https://denied.gameinc.io/")
                        .build()
                        .unwrap(),
                )
                .await
                .unwrap_err();
            assert!(denied.to_string().contains("EACCES"), "{denied}");

            // A browser client without a VM authority must not fall back to
            // reqwest or the real filesystem, even for explicitly local input.
            let unbound = ObscuraHttpClient::new();
            let denied = unbound
                .fetch(&url::Url::parse("file:///etc/hosts").unwrap())
                .await
                .unwrap_err();
            assert!(denied.to_string().contains("HOST_REQUIRED"), "{denied}");

            // Each request must be admitted before it can reach the queue.
            let oversized = host
                .request(
                    reqwest::Client::new()
                        .post("https://denied.gameinc.io/")
                        .body(vec![0; 512 * 1024 + 1])
                        .build()
                        .unwrap(),
                )
                .await
                .unwrap_err();
            assert!(oversized.to_string().contains("maxPayloadBytes"), "{oversized}");
            done_tx.send(()).unwrap();
        };
        let (_, (files, requests)) = tokio::time::timeout(Duration::from_secs(30), async {
            tokio::join!(browse, serve)
        })
        .await
        .expect("bounded browser fixture completion");
        assert_eq!(files, ["/virtual/page.html", "/not-granted.html"]);
        assert_eq!(requests, [
            "https://obscura.gameinc.io/fixture",
            "https://obscura.gameinc.io/script-fetch",
            "https://obscura.gameinc.io/redirect",
            "https://denied.gameinc.io/redirect-target",
            "https://denied.gameinc.io/",
        ]);
    }));

    // Exercise the ordinary engine after Obscura's Deno isolate has dropped.
    // Both engines must use the same V8 library and process platform owner.
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(scope, v8::ContextOptions::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let code = v8::String::new(
        scope,
        "new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])); 40+2",
    )
    .unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    assert_eq!(script.run(scope).unwrap().integer_value(scope), Some(42));
}
