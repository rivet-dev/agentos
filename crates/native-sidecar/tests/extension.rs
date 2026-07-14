mod support;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentos_bridge::{LoadFilesystemStateRequest, PersistenceBridge};
use agentos_native_sidecar::wire::{
    EventPayload, ExecuteRequest, ExtEnvelope, GuestFilesystemCallRequest,
    GuestFilesystemOperation, GuestRuntimeKind, ProcessSnapshotStatus, RequestPayload,
    ResponsePayload, SidecarRequestPayload, SidecarResponseFrame, SidecarResponsePayload,
    StreamChannel, VmLifecycleState,
};
use agentos_native_sidecar::{
    Extension, ExtensionContext, ExtensionFuture, ExtensionResponse, NativeSidecar,
    NativeSidecarConfig, SidecarError,
};
use support::{
    assert_node_available, authenticate_wire, create_vm_wire, dispose_vm_and_close_session_wire,
    new_sidecar, open_session_wire, temp_dir, wire_request, wire_vm, RecordingBridge,
};

const TEST_NAMESPACE: &str = "dev.rivet.secure-exec.extension-test";

struct EchoExtension;
struct VmLifetimeExtension;
struct BufferedExitIsolationExtension;
struct SilentExitHandoffExtension;
struct CountingExtension(Arc<AtomicUsize>);

impl Extension for CountingExtension {
    fn namespace(&self) -> &str {
        "dev.rivet.agentos.counting-extension"
    }

    fn handle_request<'a>(
        &'a self,
        _ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move { Ok(ExtensionResponse::new(payload)) })
    }
}

impl Extension for EchoExtension {
    fn namespace(&self) -> &str {
        TEST_NAMESPACE
    }

    fn handle_request<'a>(
        &'a self,
        mut ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            let callback =
                ctx.invoke_callback(b"callback-input".to_vec(), Duration::from_secs(1))?;
            let payload = String::from_utf8(payload).map_err(|error| {
                SidecarError::InvalidState(format!("invalid extension test entrypoint: {error}"))
            })?;
            let mut payload_lines = payload.lines();
            let entrypoint = payload_lines
                .next()
                .ok_or_else(|| {
                    SidecarError::InvalidState(String::from("missing extension process entrypoint"))
                })?
                .to_string();
            let lifecycle_entrypoint = payload_lines
                .next()
                .ok_or_else(|| {
                    SidecarError::InvalidState(String::from(
                        "missing extension lifecycle entrypoint",
                    ))
                })?
                .to_string();
            let process_id = "extension-process";
            ctx.start_buffering_process_output(process_id).await?;
            ctx.guest_filesystem_call_wire(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::WriteFile,
                path: String::from("/tmp/extension-fs.txt"),
                destination_path: None,
                target: None,
                content: Some(String::from("extension fs primitive")),
                encoding: None,
                recursive: None,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            })
            .await?;
            let fs_read = ctx
                .guest_filesystem_call_wire(GuestFilesystemCallRequest {
                    operation: GuestFilesystemOperation::ReadFile,
                    path: String::from("/tmp/extension-fs.txt"),
                    destination_path: None,
                    target: None,
                    content: None,
                    encoding: None,
                    recursive: None,
                    max_depth: None,
                    mode: None,
                    uid: None,
                    gid: None,
                    atime_ms: None,
                    mtime_ms: None,
                    len: None,
                    offset: None,
                })
                .await?;
            assert_eq!(fs_read.content.as_deref(), Some("extension fs primitive"));

            let started = ctx
                .spawn_process_wire(ExecuteRequest {
                    process_id: Some(process_id.to_string()),
                    command: None,
                    runtime: Some(GuestRuntimeKind::JavaScript),
                    entrypoint: Some(entrypoint),
                    args: Vec::new(),
                    env: None,
                    cwd: None,
                    wasm_permission_tier: None,
                    pty: None,
                    shell_command: None,
                    keep_stdin_open: None,
                    timeout_ms: None,
                    capture_output: None,
                })
                .await?;
            assert_eq!(started.process_id, process_id);
            let handoff = ctx
                .handoff_buffered_process_output(
                    "extension-buffered-session",
                    process_id,
                    Duration::from_secs(5),
                )
                .await?;
            assert!(String::from_utf8_lossy(&handoff.stdout).contains("extension-buffered-output"));
            assert!(!handoff.stdout_truncated);
            let lifecycle_process_id = "extension-lifecycle-process";
            let lifecycle_started = ctx
                .spawn_process_wire(ExecuteRequest {
                    process_id: Some(lifecycle_process_id.to_string()),
                    command: None,
                    runtime: Some(GuestRuntimeKind::JavaScript),
                    entrypoint: Some(lifecycle_entrypoint),
                    args: Vec::new(),
                    env: None,
                    cwd: None,
                    wasm_permission_tier: None,
                    pty: None,
                    shell_command: None,
                    keep_stdin_open: None,
                    timeout_ms: None,
                    capture_output: None,
                })
                .await?;
            assert_eq!(lifecycle_started.process_id, lifecycle_process_id);
            ctx.bind_process_to_session("extension-lifecycle-session", lifecycle_process_id)
                .await?;
            ctx.dispose_session_resources("extension-lifecycle-session")
                .await?;

            let mut stdout = handoff.stdout;
            let mut exit_code = None;
            let mut lifecycle_exit_code = None;
            while exit_code.is_none() || lifecycle_exit_code.is_none() {
                let event = ctx
                    .poll_event_wire(Duration::from_secs(5))
                    .await?
                    .ok_or_else(|| {
                        SidecarError::InvalidState(String::from(
                            "timed out waiting for extension process event",
                        ))
                    })?;
                match event.payload {
                    EventPayload::ProcessOutputEvent(output)
                        if output.process_id == process_id
                            && output.channel == StreamChannel::Stdout =>
                    {
                        stdout.extend(output.chunk);
                    }
                    EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                        exit_code = Some(exited.exit_code);
                    }
                    EventPayload::ProcessExitedEvent(exited)
                        if exited.process_id == lifecycle_process_id =>
                    {
                        lifecycle_exit_code = Some(exited.exit_code);
                    }
                    EventPayload::ProcessOutputEvent(_)
                    | EventPayload::ProcessExitedEvent(_)
                    | EventPayload::CronDispatchEvent(_)
                    | EventPayload::VmLifecycleEvent(_)
                    | EventPayload::StructuredEvent(_)
                    | EventPayload::ExtEnvelope(_) => {}
                }
            }

            let stdout = String::from_utf8(stdout).map_err(|error| {
                SidecarError::InvalidState(format!("invalid extension process stdout: {error}"))
            })?;
            let process_summary = format!(
                "{}:{}:{}",
                String::from_utf8_lossy(&callback),
                stdout.trim().replace('\n', "|"),
                exit_code.expect("exit code set before loop exits"),
            );
            ExtensionResponse::with_wire_events(
                process_summary.clone().into_bytes(),
                vec![ctx.ext_event_wire(format!("extension-event:{process_summary}").into_bytes())?],
            )
        })
    }
}

impl Extension for VmLifetimeExtension {
    fn namespace(&self) -> &str {
        "dev.rivet.secure-exec.extension-vm-lifetime-test"
    }

    fn handle_request<'a>(
        &'a self,
        mut ctx: ExtensionContext<'a>,
        _payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            ctx.bind_vm_to_session("extension-vm-session").await?;
            let outcome = ctx
                .dispose_session_resources_wire("extension-vm-session")
                .await?;
            if let Some(error) = outcome.error {
                return Err(error);
            }
            ExtensionResponse::with_wire_events(b"vm-disposed".to_vec(), outcome.events)
        })
    }
}

impl Extension for BufferedExitIsolationExtension {
    fn namespace(&self) -> &str {
        "dev.rivet.agentos.buffered-exit-isolation-test"
    }

    fn handle_request<'a>(
        &'a self,
        mut ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            let payload = String::from_utf8(payload).map_err(|error| {
                SidecarError::InvalidState(format!("invalid buffered-exit test payload: {error}"))
            })?;
            let (target_entrypoint, sibling_entrypoint) =
                payload.split_once('\n').ok_or_else(|| {
                    SidecarError::InvalidState(String::from(
                        "buffered-exit test requires target and sibling entrypoints",
                    ))
                })?;
            let target_process_id = "buffered-target-process";
            let sibling_process_id = "ordinary-sibling-process";

            ctx.start_buffering_process_output(target_process_id)
                .await?;
            for (process_id, entrypoint) in [
                (target_process_id, target_entrypoint),
                (sibling_process_id, sibling_entrypoint),
            ] {
                let started = ctx
                    .spawn_process_wire(ExecuteRequest {
                        process_id: Some(process_id.to_string()),
                        command: None,
                        runtime: Some(GuestRuntimeKind::JavaScript),
                        entrypoint: Some(entrypoint.to_string()),
                        args: Vec::new(),
                        env: None,
                        cwd: None,
                        wasm_permission_tier: None,
                        pty: None,
                        shell_command: None,
                        keep_stdin_open: None,
                        timeout_ms: None,
                        capture_output: None,
                    })
                    .await?;
                assert_eq!(started.process_id, process_id);
            }

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut target_stdout = Vec::new();
            let mut target_exit_code = None;
            while target_exit_code.is_none() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(SidecarError::InvalidState(String::from(
                        "timed out waiting for buffered target exit",
                    )));
                }
                let output = ctx
                    .drain_buffered_process_output(
                        target_process_id,
                        remaining.min(Duration::from_millis(250)),
                    )
                    .await?;
                target_stdout.extend(output.stdout);
                target_exit_code = output.exit_code.or(target_exit_code);
            }

            let mut sibling_stdout = Vec::new();
            let mut sibling_exit_code = None;
            while sibling_exit_code.is_none() {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(SidecarError::InvalidState(String::from(
                        "timed out waiting for ordinary sibling exit",
                    )));
                }
                let event = ctx
                    .poll_event_wire(remaining.min(Duration::from_millis(250)))
                    .await?;
                match event.map(|event| event.payload) {
                    Some(EventPayload::ProcessOutputEvent(output))
                        if output.process_id == sibling_process_id
                            && output.channel == StreamChannel::Stdout =>
                    {
                        sibling_stdout.extend(output.chunk);
                    }
                    Some(EventPayload::ProcessExitedEvent(exited))
                        if exited.process_id == sibling_process_id =>
                    {
                        sibling_exit_code = Some(exited.exit_code);
                    }
                    Some(
                        EventPayload::ProcessOutputEvent(_)
                        | EventPayload::ProcessExitedEvent(_)
                        | EventPayload::CronDispatchEvent(_)
                        | EventPayload::VmLifecycleEvent(_)
                        | EventPayload::StructuredEvent(_)
                        | EventPayload::ExtEnvelope(_),
                    )
                    | None => {}
                }
            }

            ctx.stop_buffering_process_output(target_process_id).await?;
            let summary = format!(
                "{}:{}|{}:{}",
                String::from_utf8_lossy(&target_stdout).trim(),
                target_exit_code.expect("target exited before summary"),
                String::from_utf8_lossy(&sibling_stdout).trim(),
                sibling_exit_code.expect("sibling exited before summary"),
            );
            Ok(ExtensionResponse::new(summary.into_bytes()))
        })
    }
}

impl Extension for SilentExitHandoffExtension {
    fn namespace(&self) -> &str {
        "dev.rivet.agentos.silent-exit-handoff-test"
    }

    fn handle_request<'a>(
        &'a self,
        mut ctx: ExtensionContext<'a>,
        payload: Vec<u8>,
    ) -> ExtensionFuture<'a, ExtensionResponse> {
        Box::pin(async move {
            let entrypoint = String::from_utf8(payload).map_err(|error| {
                SidecarError::InvalidState(format!(
                    "invalid silent-exit handoff entrypoint: {error}"
                ))
            })?;
            let process_id = "silent-buffered-process";
            ctx.start_buffering_process_output(process_id).await?;
            ctx.spawn_process_wire(ExecuteRequest {
                process_id: Some(process_id.to_string()),
                command: None,
                runtime: Some(GuestRuntimeKind::JavaScript),
                entrypoint: Some(entrypoint),
                args: Vec::new(),
                env: None,
                cwd: None,
                wasm_permission_tier: None,
                pty: None,
                shell_command: None,
                keep_stdin_open: None,
                timeout_ms: None,
                capture_output: None,
            })
            .await?;

            let buffered = ctx
                .handoff_buffered_process_output(
                    "must-not-bind-terminal-process",
                    process_id,
                    Duration::from_secs(5),
                )
                .await?;
            let exit_code = buffered.exit_code.ok_or_else(|| {
                SidecarError::InvalidState(String::from(
                    "silent buffered process handoff completed without its exit code",
                ))
            })?;
            if !buffered.stdout.is_empty() || !buffered.stderr.is_empty() {
                return Err(SidecarError::InvalidState(String::from(
                    "silent buffered process unexpectedly emitted output",
                )));
            }

            // A second start would conflict if the terminal handoff retained its
            // buffer. Remove this fresh probe before returning.
            ctx.start_buffering_process_output(process_id).await?;
            ctx.stop_buffering_process_output(process_id).await?;
            Ok(ExtensionResponse::new(exit_code.to_string().into_bytes()))
        })
    }
}

#[test]
fn registered_extension_round_trips_ext_request_callback_and_event() {
    assert_node_available();
    let mut sidecar = new_sidecar("extension-roundtrip");
    sidecar
        .register_extension(Box::new(EchoExtension))
        .expect("register extension");
    sidecar.set_wire_sidecar_request_handler(|frame| match frame.payload {
        SidecarRequestPayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, TEST_NAMESPACE);
            assert_eq!(envelope.payload, b"callback-input");
            Ok(SidecarResponseFrame {
                schema: frame.schema,
                request_id: frame.request_id,
                ownership: frame.ownership,
                payload: SidecarResponsePayload::ExtEnvelope(ExtEnvelope {
                    namespace: envelope.namespace,
                    payload: b"callback-output".to_vec(),
                }),
            })
        }
        other => panic!("unexpected sidecar request payload: {other:?}"),
    });

    let connection_id = authenticate_wire(&mut sidecar, "extension-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("extension-process-cwd");
    let entrypoint = cwd.join("extension-entrypoint.mjs");
    let lifecycle_entrypoint = cwd.join("extension-lifecycle-entrypoint.mjs");
    fs::write(
        &entrypoint,
        "console.log('extension-buffered-output');\nsetTimeout(() => {\n  console.log('extension-process-output');\n  process.exit(0);\n}, 50);\n",
    )
    .expect("write extension entrypoint");
    fs::write(&lifecycle_entrypoint, "setInterval(() => {}, 1000);\n")
        .expect("write extension lifecycle entrypoint");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: TEST_NAMESPACE.to_string(),
                payload: format!(
                    "{}\n{}",
                    entrypoint.to_string_lossy(),
                    lifecycle_entrypoint.to_string_lossy()
                )
                .into_bytes(),
            }),
        ))
        .expect("dispatch extension request");

    match result.response.payload {
        ResponsePayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, TEST_NAMESPACE);
            assert_eq!(
                envelope.payload,
                b"callback-output:extension-buffered-output|extension-process-output:0"
            );
        }
        other => panic!("unexpected extension response: {other:?}"),
    }

    assert_eq!(result.events.len(), 1);
    match &result.events[0].payload {
        EventPayload::ExtEnvelope(envelope) => {
            assert_eq!(envelope.namespace, TEST_NAMESPACE);
            assert_eq!(
                envelope.payload,
                b"extension-event:callback-output:extension-buffered-output|extension-process-output:0",
            );
        }
        other => panic!("unexpected extension event: {other:?}"),
    }
}

#[test]
fn extension_session_resources_can_dispose_bound_vm() {
    assert_node_available();
    let mut sidecar = new_sidecar("extension-vm-lifetime");
    sidecar
        .register_extension(Box::new(VmLifetimeExtension))
        .expect("register vm lifetime extension");

    let connection_id = authenticate_wire(&mut sidecar, "extension-vm-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("extension-vm-lifetime-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from("dev.rivet.secure-exec.extension-vm-lifetime-test"),
                payload: Vec::new(),
            }),
        ))
        .expect("dispatch vm lifetime extension request");

    match result.response.payload {
        ResponsePayload::ExtEnvelope(envelope) => {
            assert_eq!(
                envelope.namespace,
                "dev.rivet.secure-exec.extension-vm-lifetime-test"
            );
            assert_eq!(envelope.payload, b"vm-disposed");
        }
        other => panic!("unexpected extension response: {other:?}"),
    }
    assert!(result.events.iter().any(|event| {
        matches!(&event.payload, EventPayload::VmLifecycleEvent(event) if event.state == VmLifecycleState::Disposed)
    }));

    let rejected = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::GuestFilesystemCallRequest(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::Exists,
                path: String::from("/tmp/extension-fs.txt"),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: None,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            }),
        ))
        .expect("dispatch call against disposed vm");
    match rejected.response.payload {
        ResponsePayload::RejectedResponse(rejected) => {
            assert_eq!(rejected.code, "invalid_state");
            assert!(rejected.message.contains(&vm_id));
        }
        other => panic!("unexpected disposed-vm response: {other:?}"),
    }

    sidecar
        .with_bridge_mut(|bridge: &mut RecordingBridge| {
            let snapshot = bridge
                .load_filesystem_state(LoadFilesystemStateRequest {
                    vm_id: vm_id.clone(),
                })
                .expect("load persisted snapshot");
            assert!(
                snapshot.is_some(),
                "extension-bound vm disposal should flush a filesystem snapshot"
            );
        })
        .expect("inspect persistence bridge");
}

#[test]
fn extension_vm_cleanup_limit_rejects_before_detaching_vm() {
    let root = temp_dir("extension-vm-cleanup-limit");
    let mut sidecar = NativeSidecar::with_config(
        RecordingBridge::default(),
        NativeSidecarConfig {
            sidecar_id: String::from("extension-vm-cleanup-limit"),
            compile_cache_root: Some(root.join("cache")),
            max_extension_session_cleanup_events: 1,
            ..NativeSidecarConfig::default()
        },
    )
    .expect("create bounded sidecar");
    sidecar
        .register_extension(Box::new(VmLifetimeExtension))
        .expect("register VM lifetime extension");
    let connection_id = authenticate_wire(&mut sidecar, "extension-limit-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("extension-vm-cleanup-limit-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from("dev.rivet.secure-exec.extension-vm-lifetime-test"),
                payload: Vec::new(),
            }),
        ))
        .expect("cleanup-event reservation returns a wire rejection");
    let ResponsePayload::RejectedResponse(rejected) = result.response.payload else {
        panic!(
            "unexpected cleanup-limit response: {:?}",
            result.response.payload
        );
    };
    assert_eq!(rejected.code, "cleanup_failed");
    assert!(rejected.message.contains("limit_exceeded"));
    assert!(rejected
        .message
        .contains("max_extension_session_cleanup_events"));
    assert!(result.events.is_empty());
    let still_live = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::GuestFilesystemCallRequest(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::Exists,
                path: String::from("/tmp"),
                destination_path: None,
                target: None,
                content: None,
                encoding: None,
                recursive: None,
                max_depth: None,
                mode: None,
                uid: None,
                gid: None,
                atime_ms: None,
                mtime_ms: None,
                len: None,
                offset: None,
            }),
        ))
        .expect("VM remains routable after non-mutating cleanup rejection");
    assert!(!matches!(
        still_live.response.payload,
        ResponsePayload::RejectedResponse(_)
    ));
}

#[test]
fn exact_extension_output_buffer_preserves_sibling_events_and_cleans_captured_exit() {
    assert_node_available();
    let mut sidecar = new_sidecar("extension-buffered-exit-isolation");
    sidecar
        .register_extension(Box::new(BufferedExitIsolationExtension))
        .expect("register buffered-exit extension");

    let connection_id = authenticate_wire(&mut sidecar, "extension-buffered-exit-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("extension-buffered-exit-cwd");
    let target_entrypoint = cwd.join("buffered-target.mjs");
    let sibling_entrypoint = cwd.join("ordinary-sibling.mjs");
    fs::write(&target_entrypoint, "console.log('buffered-target');\n")
        .expect("write buffered target entrypoint");
    fs::write(&sibling_entrypoint, "console.log('ordinary-sibling');\n")
        .expect("write ordinary sibling entrypoint");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from("dev.rivet.agentos.buffered-exit-isolation-test"),
                payload: format!(
                    "{}\n{}",
                    target_entrypoint.to_string_lossy(),
                    sibling_entrypoint.to_string_lossy()
                )
                .into_bytes(),
            }),
        ))
        .expect("dispatch buffered-exit extension request");

    let ResponsePayload::ExtEnvelope(envelope) = result.response.payload else {
        panic!(
            "unexpected buffered-exit response: {:?}",
            result.response.payload
        );
    };
    assert_eq!(
        envelope.payload, b"buffered-target:0|ordinary-sibling:0",
        "exact buffering must neither consume sibling events nor skip captured-exit cleanup"
    );
    assert!(result.events.is_empty());

    let snapshot = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::GetProcessSnapshotRequest,
        ))
        .expect("query process snapshot before VM disposal");
    let ResponsePayload::ProcessSnapshotResponse(snapshot) = snapshot.response.payload else {
        panic!(
            "unexpected process snapshot response: {:?}",
            snapshot.response.payload
        );
    };
    for process_id in ["buffered-target-process", "ordinary-sibling-process"] {
        let process = snapshot
            .processes
            .iter()
            .find(|process| process.process_id == process_id)
            .unwrap_or_else(|| panic!("missing terminal process snapshot for {process_id}"));
        assert_eq!(
            process.status,
            ProcessSnapshotStatus::Exited,
            "captured and ordinary exits must both finish authoritative process cleanup before VM disposal"
        );
    }

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn silent_buffered_exit_completes_handoff_without_binding_or_leaking_the_buffer() {
    assert_node_available();
    let mut sidecar = new_sidecar("extension-silent-exit-handoff");
    sidecar
        .register_extension(Box::new(SilentExitHandoffExtension))
        .expect("register silent-exit handoff extension");

    let connection_id = authenticate_wire(&mut sidecar, "extension-silent-exit-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("extension-silent-exit-cwd");
    let entrypoint = cwd.join("silent-exit.mjs");
    fs::write(&entrypoint, "process.exit(23);\n").expect("write silent-exit entrypoint");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );

    let started_at = Instant::now();
    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ExtEnvelope(ExtEnvelope {
                namespace: String::from("dev.rivet.agentos.silent-exit-handoff-test"),
                payload: entrypoint.to_string_lossy().into_owned().into_bytes(),
            }),
        ))
        .expect("dispatch silent-exit handoff request");
    assert!(
        started_at.elapsed() < Duration::from_secs(4),
        "terminal output must make handoff ready before its five-second timeout"
    );

    let ResponsePayload::ExtEnvelope(envelope) = result.response.payload else {
        panic!(
            "unexpected silent-exit handoff response: {:?}",
            result.response.payload
        );
    };
    assert_eq!(envelope.payload, b"23");
    assert!(result.events.is_empty());

    let snapshot = sidecar
        .dispatch_wire_blocking(wire_request(
            5,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::GetProcessSnapshotRequest,
        ))
        .expect("query silent process snapshot before VM disposal");
    let ResponsePayload::ProcessSnapshotResponse(snapshot) = snapshot.response.payload else {
        panic!(
            "unexpected silent process snapshot response: {:?}",
            snapshot.response.payload
        );
    };
    let process = snapshot
        .processes
        .iter()
        .find(|process| process.process_id == "silent-buffered-process")
        .expect("silent process remains queryable as terminal history");
    assert_eq!(process.status, ProcessSnapshotStatus::Exited);

    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);
}

#[test]
fn extension_dispatch_rejects_unknown_vm_before_invoking_or_retaining_extension_state() {
    let mut sidecar = new_sidecar("extension-forged-owner");
    let calls = Arc::new(AtomicUsize::new(0));
    sidecar
        .register_extension(Box::new(CountingExtension(Arc::clone(&calls))))
        .expect("register counting extension");
    let connection_id = authenticate_wire(&mut sidecar, "extension-forged-owner-client");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);

    for request_id in 3..103 {
        let result = sidecar
            .dispatch_wire_blocking(wire_request(
                request_id,
                wire_vm(
                    &connection_id,
                    &session_id,
                    &format!("missing-vm-{request_id}"),
                ),
                RequestPayload::ExtEnvelope(ExtEnvelope {
                    namespace: String::from("dev.rivet.agentos.counting-extension"),
                    payload: b"must not run".to_vec(),
                }),
            ))
            .expect("forged extension dispatch should return a rejection frame");
        let ResponsePayload::RejectedResponse(rejected) = result.response.payload else {
            panic!(
                "unexpected forged extension response: {:?}",
                result.response.payload
            );
        };
        assert_eq!(rejected.code, "invalid_state");
        assert!(rejected.message.contains("unknown sidecar VM"));
    }

    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn duplicate_extension_namespaces_are_rejected() {
    let mut sidecar = new_sidecar("extension-duplicate");
    sidecar
        .register_extension(Box::new(EchoExtension))
        .expect("register first extension");

    let error = sidecar
        .register_extension(Box::new(EchoExtension))
        .expect_err("duplicate extension namespace should fail");
    assert!(matches!(error, SidecarError::Conflict(_)));
}
