mod support;

use agentos_native_sidecar::wire;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use support::{
    authenticate_wire, create_vm_wire, dispose_vm_and_close_session_wire, new_sidecar,
    open_session_wire, temp_dir, wire_request, wire_session, wire_vm,
};

fn process_options(execution_id: Option<String>) -> wire::ProcessExecutionOptions {
    wire::ProcessExecutionOptions {
        identity: wire::ExecutionIdentityOptions {
            execution_id,
            create_if_missing: None,
        },
        detached: Some(true),
        cwd: None,
        env: Some(HashMap::new()),
        args: Vec::new(),
        stdin: None,
        timeout_ms: Some(30_000),
        pty: None,
    }
}

fn accepted_execution_id(result: wire::WireDispatchResult) -> String {
    match result.response.payload {
        wire::ResponsePayload::ExecutionAcceptedResponse(response) => {
            response.execution.execution_id
        }
        other => panic!("unexpected language execution response: {other:?}"),
    }
}

fn wait_for_execution(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    execution_id: &str,
) -> wire::ExecutionCompletedResponse {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let event = sidecar
            .poll_event_wire_blocking(
                &wire_session(connection_id, session_id),
                Duration::from_millis(100),
            )
            .expect("poll execution event");
        if let Some(event) = event {
            if let wire::EventPayload::ExecutionCompletedEvent(completed) = event.payload {
                if completed.execution_id == execution_id {
                    break;
                }
            }
        }
        assert!(Instant::now() < deadline, "language execution timed out");
    }

    let response = sidecar
        .dispatch_wire_blocking(wire_request(
            90,
            wire_vm(connection_id, session_id, vm_id),
            wire::RequestPayload::WaitExecutionRequest(wire::WaitExecutionRequest {
                execution_id: execution_id.to_owned(),
            }),
        ))
        .expect("wait for execution result");
    match response.response.payload {
        wire::ResponsePayload::ExecutionCompletedResponse(result) => result,
        other => panic!("unexpected wait response: {other:?}"),
    }
}

fn reset_execution(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    execution_id: &str,
) {
    let response = sidecar
        .dispatch_wire_blocking(wire_request(
            91,
            wire_vm(connection_id, session_id, vm_id),
            wire::RequestPayload::ResetExecutionRequest(wire::ResetExecutionRequest {
                execution_id: execution_id.to_owned(),
            }),
        ))
        .expect("reset retained execution");
    match response.response.payload {
        wire::ResponsePayload::ExecutionDescriptorResponse(response) => {
            assert_eq!(response.execution.state, wire::ExecutionState::Idle);
            assert_eq!(response.execution.retained_language, None);
        }
        other => panic!("unexpected reset response: {other:?}"),
    }
}

#[test]
fn javascript_execution_reuses_retained_context() {
    let mut sidecar = new_sidecar("language-execution-retained-js");
    let connection_id = authenticate_wire(&mut sidecar, "language-execution-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-retained-js-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let first = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptExecutionRequest(wire::JavaScriptExecutionRequest {
                process: process_options(None),
                source: String::from(
                    "import { sep } from 'node:path'; let retainedAnswer = sep === '/' ? 41 : 0;",
                ),
                format: Some(wire::JavaScriptModuleFormat::Module),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start first JavaScript operation");
    let execution_id = accepted_execution_id(first);
    let first_result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(first_result.outcome, wire::ExecutionOutcome::Succeeded);

    let mut fresh_process_options = process_options(Some(execution_id.clone()));
    fresh_process_options.args = vec![String::from("-e"), String::from("void 0")];
    let process = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::ArgvExecutionRequest(wire::ArgvExecutionRequest {
                process: fresh_process_options,
                command: String::from("node"),
            }),
        ))
        .expect("start fresh process between retained operations");
    assert!(
        process.events.is_empty(),
        "interleaved process failed during admission: {:?}",
        process.events
    );
    assert_eq!(accepted_execution_id(process), execution_id);
    let process_result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(process_result.outcome, wire::ExecutionOutcome::Succeeded);

    let typescript = sidecar
        .dispatch_wire_blocking(wire_request(
            6,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::TypeScriptExecutionRequest(wire::TypeScriptExecutionRequest {
                process: process_options(Some(execution_id.clone())),
                source: String::from(
                    "const typedAnswer: number = sep === '/' ? retainedAnswer + 1 : 0;",
                ),
                file_path: Some(String::from("retained-cell.ts")),
                tsconfig_path: None,
                compiler_options: None,
                inputs: None,
            }),
        ))
        .expect("start retained TypeScript operation");
    assert_eq!(accepted_execution_id(typescript), execution_id);
    let typescript_result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(typescript_result.outcome, wire::ExecutionOutcome::Succeeded);

    let second = sidecar
        .dispatch_wire_blocking(wire_request(
            7,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptEvaluationRequest(wire::JavaScriptEvaluationRequest {
                process: process_options(Some(execution_id.clone())),
                expression: String::from("typedAnswer"),
                format: Some(wire::JavaScriptModuleFormat::Module),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start retained JavaScript evaluation");
    assert_eq!(accepted_execution_id(second), execution_id);
    let second_result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(second_result.outcome, wire::ExecutionOutcome::Succeeded);
    assert!(second_result.outputs.contains("42"));

    reset_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn typescript_check_reports_semantic_diagnostics() {
    let mut sidecar = new_sidecar("language-execution-typescript-check");
    let connection_id = authenticate_wire(&mut sidecar, "typescript-check-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-typescript-check-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let check = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::TypeScriptCheckRequest(wire::TypeScriptCheckRequest {
                identity: wire::ExecutionIdentityOptions {
                    execution_id: None,
                    create_if_missing: None,
                },
                source: String::from("const answer: string = 42;"),
                cwd: None,
                file_path: Some(String::from("answer.ts")),
                tsconfig_path: None,
                compiler_options: None,
                timeout_ms: Some(30_000),
            }),
        ))
        .expect("start TypeScript check");
    let execution_id = accepted_execution_id(check);
    let result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(
        result.outcome,
        wire::ExecutionOutcome::Succeeded,
        "TypeScript check failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let outputs: serde_json::Value =
        serde_json::from_str(&result.outputs).expect("decode TypeScript check outputs");
    assert_eq!(outputs[0]["data"]["hasErrors"], true);
    assert!(outputs[0]["data"]["diagnostics"]
        .as_array()
        .is_some_and(|diagnostics| diagnostics.iter().any(|item| item["code"] == 2322)));

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn python_execution_reuses_retained_globals() {
    let mut sidecar = new_sidecar("language-execution-retained-python");
    let connection_id = authenticate_wire(&mut sidecar, "retained-python-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-retained-python-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::Python,
        &cwd,
    );
    let first = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::PythonExecutionRequest(wire::PythonExecutionRequest {
                process: process_options(None),
                source: String::from(
                    "import asyncio\nawait asyncio.sleep(0)\nretained_answer = 41",
                ),
                inputs: None,
            }),
        ))
        .expect("start first Python operation");
    let execution_id = accepted_execution_id(first);
    assert_eq!(
        wait_for_execution(
            &mut sidecar,
            &connection_id,
            &session_id,
            &vm_id,
            &execution_id,
        )
        .outcome,
        wire::ExecutionOutcome::Succeeded
    );

    let second = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::PythonEvaluationRequest(wire::PythonEvaluationRequest {
                process: process_options(Some(execution_id.clone())),
                expression: String::from("retained_answer + 1"),
                inputs: None,
            }),
        ))
        .expect("start retained Python evaluation");
    assert_eq!(accepted_execution_id(second), execution_id);
    let result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(result.outcome, wire::ExecutionOutcome::Succeeded);
    assert!(result.outputs.contains("42"));

    reset_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn execution_timeout_is_enforced_by_the_sidecar() {
    let mut sidecar = new_sidecar("language-execution-timeout");
    let connection_id = authenticate_wire(&mut sidecar, "execution-timeout-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-timeout-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let mut options = process_options(None);
    options.timeout_ms = Some(100);
    let started_at = Instant::now();
    let started = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptExecutionRequest(wire::JavaScriptExecutionRequest {
                process: options,
                source: String::from("while (true) {}"),
                format: Some(wire::JavaScriptModuleFormat::CommonJs),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start timed JavaScript operation");
    let execution_id = accepted_execution_id(started);
    let result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(result.outcome, wire::ExecutionOutcome::TimedOut);
    assert_eq!(
        result.error.as_ref().map(|error| error.code.as_str()),
        Some("execution_timed_out")
    );
    assert!(
        started_at.elapsed() < Duration::from_secs(5),
        "sidecar timeout did not terminate the guest promptly"
    );

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn evaluation_rejects_non_json_values_with_a_structured_result() {
    let mut sidecar = new_sidecar("language-execution-json-evaluation");
    let connection_id = authenticate_wire(&mut sidecar, "json-evaluation-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-json-evaluation-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let started = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptEvaluationRequest(wire::JavaScriptEvaluationRequest {
                process: process_options(None),
                expression: String::from("undefined"),
                format: Some(wire::JavaScriptModuleFormat::CommonJs),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start non-JSON JavaScript evaluation");
    let execution_id = accepted_execution_id(started);
    let result = wait_for_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );
    assert_eq!(result.outcome, wire::ExecutionOutcome::Failed);
    assert_eq!(
        result.error.as_ref().map(|error| error.code.as_str()),
        Some("evaluation_serialization_failed")
    );
    assert!(result
        .error
        .as_ref()
        .is_some_and(|error| error.message.contains("JSON-serializable")));
    assert_eq!(result.outputs, "[]");

    reset_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn package_mutations_are_exclusive_across_executions() {
    let mut sidecar = new_sidecar("language-execution-package-mutation");
    let connection_id = authenticate_wire(&mut sidecar, "package-mutation-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-package-mutation-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::Python,
        &cwd,
    );
    let first = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::PythonInstallRequest(wire::PythonInstallRequest {
                identity: wire::ExecutionIdentityOptions {
                    execution_id: None,
                    create_if_missing: None,
                },
                cwd: None,
                env: None,
                timeout_ms: Some(30_000),
                packages: vec![String::from("agentos-package-mutation-test")],
                upgrade: None,
                requirements_file: None,
                index_url: Some(String::from("http://127.0.0.1:9/simple")),
                extra_index_urls: Vec::new(),
            }),
        ))
        .expect("start first package mutation");
    let first_execution_id = accepted_execution_id(first);

    let second = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::NpmProjectInstallRequest(wire::NpmProjectInstallRequest {
                identity: wire::ExecutionIdentityOptions {
                    execution_id: Some(String::from("second-package-mutation")),
                    create_if_missing: Some(true),
                },
                cwd: None,
                env: None,
                timeout_ms: Some(30_000),
                frozen: None,
            }),
        ))
        .expect("reject concurrent package mutation");
    match second.response.payload {
        wire::ResponsePayload::RejectedResponse(rejected) => {
            assert_eq!(rejected.code, "execution_busy");
            assert!(rejected.message.contains(&first_execution_id));
            assert!(rejected.message.contains("serialized at VM scope"));
        }
        other => panic!("expected package mutation rejection, got {other:?}"),
    }

    sidecar
        .dispatch_wire_blocking(wire_request(
            6,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::CancelExecutionRequest(wire::CancelExecutionRequest {
                execution_id: first_execution_id.clone(),
            }),
        ))
        .expect("cancel first package mutation");
    assert_eq!(
        wait_for_execution(
            &mut sidecar,
            &connection_id,
            &session_id,
            &vm_id,
            &first_execution_id,
        )
        .outcome,
        wire::ExecutionOutcome::Cancelled
    );

    let resumed = sidecar
        .dispatch_wire_blocking(wire_request(
            7,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::NpmProjectInstallRequest(wire::NpmProjectInstallRequest {
                identity: wire::ExecutionIdentityOptions {
                    execution_id: Some(String::from("second-package-mutation")),
                    create_if_missing: Some(true),
                },
                cwd: None,
                env: None,
                timeout_ms: Some(30_000),
                frozen: None,
            }),
        ))
        .expect("start package mutation after the prior one completed");
    let resumed_execution_id = accepted_execution_id(resumed);
    sidecar
        .dispatch_wire_blocking(wire_request(
            8,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::CancelExecutionRequest(wire::CancelExecutionRequest {
                execution_id: resumed_execution_id.clone(),
            }),
        ))
        .expect("cancel resumed package mutation");
    assert_eq!(
        wait_for_execution(
            &mut sidecar,
            &connection_id,
            &session_id,
            &vm_id,
            &resumed_execution_id,
        )
        .outcome,
        wire::ExecutionOutcome::Cancelled
    );

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn detached_lifecycle_replays_cancels_resets_and_deletes() {
    let mut sidecar = new_sidecar("language-execution-detached-lifecycle");
    let connection_id = authenticate_wire(&mut sidecar, "detached-lifecycle-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("language-execution-detached-lifecycle-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        wire::GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let started = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptExecutionRequest(wire::JavaScriptExecutionRequest {
                process: process_options(None),
                source: String::from("console.log('replay-me')"),
                format: Some(wire::JavaScriptModuleFormat::CommonJs),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start detached JavaScript operation");
    let execution_id = accepted_execution_id(started);
    assert_eq!(
        wait_for_execution(
            &mut sidecar,
            &connection_id,
            &session_id,
            &vm_id,
            &execution_id,
        )
        .outcome,
        wire::ExecutionOutcome::Succeeded
    );

    let replay = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::ReadExecutionOutputRequest(wire::ReadExecutionOutputRequest {
                execution_id: execution_id.clone(),
                cursor: None,
                limit: Some(1),
            }),
        ))
        .expect("read retained execution output");
    match replay.response.payload {
        wire::ResponsePayload::ExecutionOutputPageResponse(page) => {
            assert!(!page.truncated);
            assert_eq!(page.events.len(), 1);
            assert!(String::from_utf8_lossy(&page.events[0].chunk).contains("replay-me"));
            assert!(!page.next_cursor.is_empty());
        }
        other => panic!("expected execution output page, got {other:?}"),
    }

    reset_execution(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        &execution_id,
    );

    let mut cancel_options = process_options(Some(execution_id.clone()));
    cancel_options.timeout_ms = Some(30_000);
    let cancellable = sidecar
        .dispatch_wire_blocking(wire_request(
            6,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::JavaScriptExecutionRequest(wire::JavaScriptExecutionRequest {
                process: cancel_options,
                source: String::from("while (true) {}"),
                format: Some(wire::JavaScriptModuleFormat::CommonJs),
                file_path: None,
                inputs: None,
            }),
        ))
        .expect("start cancellable execution");
    assert_eq!(accepted_execution_id(cancellable), execution_id);
    sidecar
        .dispatch_wire_blocking(wire_request(
            7,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::CancelExecutionRequest(wire::CancelExecutionRequest {
                execution_id: execution_id.clone(),
            }),
        ))
        .expect("cancel execution");
    assert_eq!(
        wait_for_execution(
            &mut sidecar,
            &connection_id,
            &session_id,
            &vm_id,
            &execution_id,
        )
        .outcome,
        wire::ExecutionOutcome::Cancelled
    );

    let deleted = sidecar
        .dispatch_wire_blocking(wire_request(
            8,
            wire_vm(&connection_id, &session_id, &vm_id),
            wire::RequestPayload::DeleteExecutionRequest(wire::DeleteExecutionRequest {
                execution_id: execution_id.clone(),
            }),
        ))
        .expect("delete idle execution");
    match deleted.response.payload {
        wire::ResponsePayload::ExecutionDeletedResponse(response) => {
            assert_eq!(response.execution_id, execution_id);
        }
        other => panic!("expected execution deletion, got {other:?}"),
    }

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}
