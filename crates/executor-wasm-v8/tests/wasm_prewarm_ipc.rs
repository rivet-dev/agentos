use agentos_driver_tokio::{DriverConfig, TokioDriver};
use agentos_executor_contract::GuestRuntimeConfig;
use agentos_executor_wasm_v8::{
    CreateWasmContextRequest, StartWasmExecutionRequest, WasmExecutionEvent, WasmExecutionLimits,
    WasmPermissionTier, WasmV8Execution, WasmV8ExecutionEngine,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const CASE_TIMEOUT: Duration = Duration::from_secs(10);
const OUTPUT_LIMIT: usize = 64 * 1024;

// Also owns failures during start, before an execution handle is returned.
struct Case {
    engine: WasmV8ExecutionEngine,
    execution: Option<WasmV8Execution>,
    vm_id: String,
    exited: bool,
}

impl Drop for Case {
    fn drop(&mut self) {
        if let Some(execution) = self.execution.as_mut() {
            if !self.exited {
                if let Err(error) = execution.terminate() {
                    eprintln!("prewarm IPC test cleanup termination failed: {error}");
                }
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline {
                    match execution.poll_event_blocking(Duration::from_millis(100)) {
                        Ok(Some(WasmExecutionEvent::Exited(_))) => {
                            self.exited = true;
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("prewarm IPC test cleanup event failed: {error}");
                            break;
                        }
                    }
                }
                if !self.exited {
                    eprintln!("prewarm IPC test cleanup did not observe terminal exit within 2s");
                }
            }
        }
        drop(self.execution.take());
        self.engine.dispose_vm(&self.vm_id);
    }
}

fn cold_case(driver: &TokioDriver, async_start: bool, kernel_ipc: bool) {
    let temp = tempfile::tempdir().expect("fresh module directory");
    let module = temp.path().join("exit.wasm");
    std::fs::write(
        &module,
        wat::parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))
                (memory (export "memory") 1)
                (func (export "_start") i32.const 0 call $exit))"#,
        )
        .expect("compile real WASI fixture"),
    )
    .expect("write real WASI fixture");
    let vm_id = format!("prewarm-ipc-{}", temp.path().display());
    let mut case = Case {
        engine: WasmV8ExecutionEngine::new(driver.handle()),
        execution: None,
        vm_id: vm_id.clone(),
        exited: false,
    };
    let context = case.engine.create_context(CreateWasmContextRequest {
        vm_id: vm_id.clone(),
        module_path: Some(module.to_str().expect("UTF-8 temporary path").to_owned()),
    });
    let request = StartWasmExecutionRequest {
        vm_id,
        context_id: context.context_id,
        managed_kernel_host: kernel_ipc,
        argv: Vec::new(),
        env: BTreeMap::from([("AGENTOS_WASM_WARMUP_DEBUG".into(), "1".into())]),
        cwd: temp.path().to_owned(),
        permission_tier: WasmPermissionTier::Full,
        limits: WasmExecutionLimits {
            prewarm_timeout_ms: Some(5_000),
            wall_clock_limit_ms: Some(10_000),
            bridge_call_timeout_ms: Some(5_000),
            ..Default::default()
        },
        guest_runtime: GuestRuntimeConfig {
            kernel_ipc,
            ..Default::default()
        },
    };
    // Starting must complete before this host consumes any managed IPC request.
    // A fresh path participates in the warmup marker identity, so neither path
    // can borrow the other case's completed prewarm.
    let execution = if async_start {
        driver.block_on(
            case.engine
                .start_execution_with_runtime_async(request, driver.handle()),
        )
    } else {
        case.engine.start_execution(request)
    }
    .expect("cold prewarm must not require a managed IPC consumer");
    case.execution = Some(execution);
    let deadline = Instant::now() + CASE_TIMEOUT;
    let mut ipc_requests = 0;
    let mut descriptor_bootstraps = 0;
    let mut output_bytes = 0;
    let mut stderr = Vec::new();
    for _ in 0..128 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "actual WASI execution deadline");
        let execution = case.execution.as_mut().expect("started execution");
        let event = execution
            .poll_event_blocking(remaining)
            .expect("poll actual WASI execution")
            .expect("actual WASI execution must finish within deadline");
        match event {
            WasmExecutionEvent::SyncRpcRequest(request) => {
                let method = if request.method == "process.wasm_sync_rpc" {
                    request
                        .args
                        .first()
                        .and_then(Value::as_str)
                        .expect("wrapped RPC method")
                } else {
                    &request.method
                };
                let reply = match method {
                    "process.fd_bootstrap" => {
                        descriptor_bootstraps += 1;
                        assert!(kernel_ipc && descriptor_bootstraps == 1);
                        // The exit-only module needs no descriptors or preopens.
                        json!({"preopens": [], "descriptors": []})
                    }
                    "process.ipc_info" => {
                        ipc_requests += 1;
                        assert!(
                            kernel_ipc && ipc_requests == 1,
                            "only actual managed startup discovers IPC once"
                        );
                        Value::Null
                    }
                    "process.signal_mask" => json!({"signals": []}),
                    "process.signal_mask_scope_begin" => json!(1),
                    "process.signal_mask_scope_end"
                    | "process.signal_end"
                    | "process.take_signal"
                    | "process.signal_begin"
                    | "process.signal_state" => Value::Null,
                    other => panic!("unexpected host RPC: {other}"),
                };
                execution
                    .respond_sync_rpc_success(request.id, reply)
                    .expect("reply to actual startup RPC");
            }
            WasmExecutionEvent::Stdout(chunk) | WasmExecutionEvent::Stderr(chunk) => {
                output_bytes += chunk.len();
                assert!(output_bytes <= OUTPUT_LIMIT, "64KiB test output bound");
                // This program emits no stdout; the runner emits structured
                // warmup telemetry on stderr. Preserve both for failure context.
                stderr.extend_from_slice(&chunk);
            }
            WasmExecutionEvent::SignalState { .. } => {}
            WasmExecutionEvent::HostCall { request, .. } => {
                panic!("unexpected direct host-call route: {}", request.method)
            }
            WasmExecutionEvent::Exited(code) => {
                case.exited = true;
                assert_eq!(
                    code,
                    0,
                    "real WASI proc_exit(0): {}",
                    String::from_utf8_lossy(&stderr)
                );
                assert_eq!(
                    ipc_requests,
                    usize::from(kernel_ipc),
                    "actual execution preserves configured IPC discovery"
                );
                assert_eq!(descriptor_bootstraps, usize::from(kernel_ipc));
                let text = std::str::from_utf8(&stderr).expect("runner telemetry UTF-8");
                let metrics: Vec<Value> = text
                    .lines()
                    .filter_map(|line| line.strip_prefix("__AGENTOS_WASM_WARMUP_METRICS__:"))
                    .map(|line| serde_json::from_str(line).expect("structured prewarm telemetry"))
                    .collect();
                assert_eq!(metrics.len(), 1, "one actual prewarm result: {text}");
                assert_eq!(
                    metrics[0]["executed"],
                    json!(true),
                    "must exercise cold prewarm"
                );
                return;
            }
        }
    }
    panic!("actual WASI execution exceeded 128 events");
}

#[test]
fn cold_wasm_prewarm_defers_kernel_ipc_until_actual_execution() {
    let driver = TokioDriver::process(&DriverConfig::default()).expect("process Tokio driver");
    cold_case(driver, false, true);
    cold_case(driver, true, true);
    cold_case(driver, false, false);
}
