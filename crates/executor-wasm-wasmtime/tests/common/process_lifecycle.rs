//! Wasmtime process ABI regressions against authoritative host process state.

use agentos_driver_tokio::{DriverConfig, TokioDriver};
use agentos_executor_contract::backend::{
    bounded_execution_event_channel, ExecutionEvent, PayloadLimit,
};
use agentos_executor_contract::host::{
    FilesystemOperation, HostOperation, HostProcessContext, ProcessHostCapabilitySet,
    ProcessOperation, SignalOperation,
};
use agentos_executor_contract::GuestRuntimeConfig;
use agentos_executor_wasm_abi::{
    StartWasmExecutionRequest, WasmExecutionEvent, WasmExecutionLimits, WasmPermissionTier,
};
use agentos_executor_wasm_wasmtime::WasmtimeExecution;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) struct Outcome {
    pub(crate) exit_code: i32,
    pub(crate) stderr: String,
    pub(crate) exec_commits: usize,
}

// Shared with the threaded sidecar suite, which calls run_with_threads directly.
#[allow(dead_code)]
pub(crate) fn run(initial: &[u8], replacement: Option<&[u8]>) -> Outcome {
    run_with_threads(initial, replacement, false)
}

pub(crate) fn run_with_threads(
    initial: &[u8],
    replacement: Option<&[u8]>,
    threaded: bool,
) -> Outcome {
    run_with_threads_and_fuel(initial, replacement, threaded, None)
}

pub(crate) fn run_with_threads_and_fuel(
    initial: &[u8],
    replacement: Option<&[u8]>,
    threaded: bool,
    fuel: Option<u64>,
) -> Outcome {
    let driver = TokioDriver::process(&DriverConfig::default()).expect("driver");
    let (submission, events) = bounded_execution_event_channel(
        HostProcessContext {
            generation: 1,
            pid: 42,
        },
        32,
        PayloadLimit::new("limits.process.pendingEventBytes", 1024 * 1024).unwrap(),
        Arc::new(|| {}),
    )
    .unwrap();
    let execution = WasmtimeExecution::spawn(
        "process-regression".into(),
        "/initial.wasm".into(),
        StartWasmExecutionRequest {
            vm_id: "vm-regression".into(),
            context_id: "context-regression".into(),
            managed_kernel_host: true,
            argv: vec!["/initial.wasm".into()],
            env: BTreeMap::new(),
            cwd: PathBuf::from("/"),
            permission_tier: WasmPermissionTier::Full,
            limits: WasmExecutionLimits {
                deterministic_fuel: fuel,
                max_threads: Some(2),
                max_memory_bytes: Some(2 * 65536),
                ..WasmExecutionLimits::default()
            },
            guest_runtime: GuestRuntimeConfig {
                virtual_ppid: Some(7),
                ..Default::default()
            },
        },
        driver.handle(),
        None,
        false,
        threaded,
    )
    .unwrap();
    execution.configure_host_services(ProcessHostCapabilitySet::from_event_submission(submission));
    let mut opens = 0;
    let mut parent_reads = 0;
    let mut thread_exited = false;
    let mut exec_commits = 0;
    let mut stderr = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() >= deadline {
            execution.terminate();
            panic!("process regression timed out: {stderr}");
        }
        while let Some(event) = events.try_recv().expect("host events") {
            let ExecutionEvent::HostCall { operation, reply } = event else {
                panic!("unexpected event: {event:?}");
            };
            match operation {
                HostOperation::Filesystem(FilesystemOperation::CanonicalPreopens) => {
                    reply.succeed_json(Value::Null).unwrap();
                }
                HostOperation::Process(ProcessOperation::OpenExecutableImage { .. }) => {
                    opens += 1;
                    let image = if opens == 1 {
                        initial
                    } else {
                        replacement.unwrap()
                    };
                    reply.succeed_json(json!({
                        "handle": opens.to_string(),
                        "size": image.len(),
                        "argv": if opens == 1 { Value::Null } else { json!(["/replacement.wasm"]) },
                    })).unwrap();
                }
                HostOperation::Process(ProcessOperation::ReadExecutableImage {
                    handle,
                    offset,
                    max_bytes,
                }) => {
                    let image = if handle == 1 {
                        initial
                    } else {
                        replacement.unwrap()
                    };
                    let start = offset as usize;
                    let end = (start + max_bytes.get()).min(image.len());
                    reply.succeed_raw(image[start..end].to_vec()).unwrap();
                }
                HostOperation::Process(ProcessOperation::CloseExecutableImage { .. }) => {
                    reply.succeed_json(Value::Null).unwrap();
                }
                HostOperation::Process(ProcessOperation::GetParentPid) => {
                    // The kernel reparents the process between the two calls.
                    parent_reads += 1;
                    reply
                        .succeed_json(json!(if threaded {
                            if thread_exited || exec_commits > 0 {
                                1
                            } else {
                                7
                            }
                        } else if parent_reads == 1 {
                            7
                        } else {
                            1
                        }))
                        .unwrap();
                }
                HostOperation::Process(ProcessOperation::Exec(_)) => {
                    exec_commits += 1;
                    reply.succeed_json(Value::Null).unwrap();
                }
                HostOperation::Signal(SignalOperation::RegisterThread { .. }) => {
                    reply.succeed_json(Value::Null).unwrap();
                }
                HostOperation::Signal(SignalOperation::UnregisterThread { .. }) => {
                    thread_exited = true;
                    reply.succeed_json(Value::Null).unwrap();
                }
                HostOperation::Signal(
                    SignalOperation::UpdateMask { .. }
                    | SignalOperation::UpdateMaskForThread { .. },
                ) => {
                    reply.succeed_json(json!({ "signals": [] })).unwrap();
                }
                other => panic!("unexpected operation: {other:?}"),
            }
        }
        match execution
            .poll_event_blocking(Duration::from_millis(1))
            .unwrap()
        {
            Some(WasmExecutionEvent::Exited(exit_code)) => {
                return Outcome {
                    exit_code,
                    stderr,
                    exec_commits,
                };
            }
            Some(WasmExecutionEvent::Stderr(bytes)) => {
                stderr.push_str(&String::from_utf8_lossy(&bytes))
            }
            Some(WasmExecutionEvent::HostCall { request, reply }) => {
                match request.method.as_str() {
                    "process.exec_image_open" | "process.exec_image_open_fd" => {
                        opens += 1;
                        let image = replacement.expect("replacement image");
                        reply
                            .succeed_json(json!({
                                "handle": opens.to_string(),
                                "size": image.len(),
                                "argv": ["/replacement.wasm"],
                            }))
                            .unwrap();
                    }
                    "process.exec" | "process.exec_fd_image_commit" => {
                        exec_commits += 1;
                        reply.succeed_json(Value::Null).unwrap();
                    }
                    "process.getppid" => {
                        parent_reads += 1;
                        reply
                            .succeed_json(json!(if threaded {
                                if thread_exited || exec_commits > 0 {
                                    1
                                } else {
                                    7
                                }
                            } else if parent_reads == 1 {
                                7
                            } else {
                                1
                            }))
                            .unwrap();
                    }
                    other => panic!("unexpected compatibility call: {other}"),
                }
            }
            Some(other) => panic!("unexpected executor event: {other:?}"),
            None => {}
        }
    }
}
