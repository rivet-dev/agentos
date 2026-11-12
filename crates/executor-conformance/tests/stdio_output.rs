mod support;

use agentos_executor_conformance::{
    CreateJavascriptContextRequest, GuestRuntimeConfig, JavascriptExecutionEvent,
    StartJavascriptExecutionRequest,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[test]
fn standalone_output_and_kernel_descriptor_output_keep_distinct_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let mut engine = support::javascript_engine();
    for kernel_stdio in [false, true] {
        let context = engine.create_context(CreateJavascriptContextRequest {
            vm_id: "stdio-owner-control".into(),
            bootstrap_module: None,
            compile_cache_root: None,
        });
        let mut execution = engine.start_execution(StartJavascriptExecutionRequest {
            limits: Default::default(),
            guest_runtime: GuestRuntimeConfig { kernel_stdio, ..Default::default() },
            vm_id: "stdio-owner-control".into(),
            context_id: context.context_id,
            argv: vec!["./entry.mjs".into()],
            argv0: None,
            env: BTreeMap::new(),
            cwd: temp.path().to_path_buf(),
            wasm_module_bytes: None,
            inline_code: Some("globalThis.__runtimeKernelStdin = false; process.stdout.write(Buffer.from([0,188,255,65])); process.stderr.write(Buffer.from([254,0,66]));".into()),
        }).expect("start stdio ownership control");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut calls = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "kernel output control timed out");
            match execution
                .poll_event_blocking(Duration::from_millis(100))
                .expect("poll output")
            {
                Some(JavascriptExecutionEvent::SyncRpcRequest(request)) => {
                    if execution
                        .try_service_standalone_module_sync_rpc(&request)
                        .unwrap()
                    {
                        continue;
                    }
                    assert!(kernel_stdio, "standalone unexpected host RPC: {request:?}");
                    if request.method == "__kernel_isatty" {
                        execution
                            .respond_sync_rpc_success(request.id, json!(false))
                            .unwrap();
                        continue;
                    }
                    assert_eq!(request.method, "__kernel_stdio_write");
                    let bytes = request
                        .raw_bytes_args
                        .get(&1)
                        .expect("binary stdio payload");
                    let count = bytes.len().min(2);
                    match request.args[0].as_u64() {
                        Some(1) => stdout.extend_from_slice(&bytes[..count]),
                        Some(2) => stderr.extend_from_slice(&bytes[..count]),
                        fd => panic!("unexpected stdio fd {fd:?}"),
                    }
                    calls += 1;
                    execution
                        .respond_sync_rpc_success(request.id, json!(count))
                        .unwrap();
                }
                Some(JavascriptExecutionEvent::Exited(code)) => {
                    assert_eq!(code, 0);
                    break;
                }
                Some(JavascriptExecutionEvent::Stdout(bytes)) if !kernel_stdio => {
                    stdout.extend(bytes)
                }
                Some(JavascriptExecutionEvent::Stderr(bytes)) if !kernel_stdio => {
                    stderr.extend(bytes)
                }
                None => {}
                event => panic!("kernel output must not bypass descriptor RPC: {event:?}"),
            }
        }
        assert_eq!(
            calls,
            if kernel_stdio { 4 } else { 0 },
            "only VM output must retry partial kernel writes"
        );
        assert_eq!(stdout, [0, 188, 255, 65]);
        assert_eq!(stderr, [254, 0, 66]);
    }
}
