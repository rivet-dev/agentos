#[path = "../../bridge/tests/support.rs"]
mod bridge_support;

use agentos_bridge::{
    CreateJavascriptContextRequest, CreateWasmContextRequest, ExecutionEvent, ExecutionExited,
    ExecutionSignal, ExecutionSignalState, GuestKernelCall, GuestRuntime, KillExecutionRequest,
    LifecycleState, OutputChunk, PollExecutionEventRequest, SignalDispositionAction,
    SignalHandlerRegistration, StartExecutionRequest,
};
use agentos_kernel::kernel::KernelVmConfig;
use agentos_kernel::permissions::{
    NetworkAccessRequest, NetworkOperation, PermissionDecision, Permissions,
};
use agentos_kernel::resource_accounting::ResourceLimits;
use agentos_kernel::root_fs::FilesystemEntryKind;
use agentos_native_sidecar_browser::{
    BrowserProjectedAgentLaunch, BrowserProjectedCommand, BrowserProjectedPackageAgent,
    BrowserSidecar, BrowserSidecarConfig, BrowserWorkerBridge, BrowserWorkerEntrypoint,
    BrowserWorkerHandle, BrowserWorkerHandleRequest, BrowserWorkerOsConfig,
    BrowserWorkerProcessConfig, BrowserWorkerSpawnRequest, ExecutionOutput,
    PollExecutionOutputRequest, MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM,
};
use agentos_sidecar_protocol::wire::{
    FindBoundUdpRequest, FindListenerRequest, GuestKernelCallRequest, WasmPermissionTier,
};
use agentos_vm_config::{
    RootFilesystemConfig, RootFilesystemEntry, RootFilesystemEntryEncoding,
    RootFilesystemEntryKind, RootFilesystemLowerDescriptor, RootFilesystemMode,
};
use bridge_support::RecordingBridge;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

impl BrowserWorkerBridge for RecordingBridge {
    fn create_worker(
        &mut self,
        request: BrowserWorkerSpawnRequest,
    ) -> Result<BrowserWorkerHandle, Self::Error> {
        if let Some(error) = self.next_worker_create_error() {
            return Err(error);
        }

        let kind = match request.runtime {
            GuestRuntime::JavaScript => "js",
            GuestRuntime::WebAssembly => "wasm",
        };
        self.browser_worker_spawns.push(BTreeMap::from([
            (String::from("vm_id"), request.vm_id.clone()),
            (String::from("context_id"), request.context_id.clone()),
            (String::from("execution_id"), request.execution_id.clone()),
            (
                String::from("process_platform"),
                request.process_config.platform.clone(),
            ),
            (
                String::from("process_arch"),
                request.process_config.arch.clone(),
            ),
            (
                String::from("process_cwd"),
                request.process_config.cwd.clone(),
            ),
            (
                String::from("process_pid"),
                request.process_config.pid.to_string(),
            ),
            (
                String::from("process_uid"),
                request.process_config.uid.to_string(),
            ),
            (
                String::from("process_gid"),
                request.process_config.gid.to_string(),
            ),
            (
                String::from("process_env_base"),
                request
                    .process_config
                    .env
                    .get("BASE_ENV")
                    .cloned()
                    .unwrap_or_default(),
            ),
            (
                String::from("process_env_exec"),
                request
                    .process_config
                    .env
                    .get("EXEC_ENV")
                    .cloned()
                    .unwrap_or_default(),
            ),
            (
                String::from("os_platform"),
                request.os_config.platform.clone(),
            ),
            (
                String::from("os_cpu_count"),
                request.os_config.cpu_count.to_string(),
            ),
            (
                String::from("os_totalmem"),
                request.os_config.totalmem.to_string(),
            ),
            (
                String::from("os_freemem"),
                request.os_config.freemem.to_string(),
            ),
            (String::from("os_user"), request.os_config.user.clone()),
            (String::from("os_uid"), request.os_config.uid.to_string()),
            (String::from("os_gid"), request.os_config.gid.to_string()),
            (
                String::from("os_homedir"),
                request.os_config.homedir.clone(),
            ),
            (
                String::from("os_hostname"),
                request.os_config.hostname.clone(),
            ),
            (String::from("os_type"), request.os_config.r#type.clone()),
            (
                String::from("os_release"),
                request.os_config.release.clone(),
            ),
            (
                String::from("os_version"),
                request.os_config.version.clone(),
            ),
            (String::from("os_tmpdir"), request.os_config.tmpdir.clone()),
            (
                String::from("os_machine"),
                request.os_config.machine.clone(),
            ),
            (
                String::from("wasm_permission_tier"),
                request
                    .wasm_permission_tier
                    .map(|tier| format!("{tier:?}"))
                    .unwrap_or_default(),
            ),
        ]));

        Ok(BrowserWorkerHandle {
            worker_id: format!("{kind}-worker-{}", request.context_id),
            runtime: request.runtime,
        })
    }

    fn terminate_worker(&mut self, request: BrowserWorkerHandleRequest) -> Result<(), Self::Error> {
        self.terminated_workers
            .push((request.vm_id, request.execution_id, request.worker_id));
        if let Some(error) = self.next_worker_terminate_error() {
            return Err(error);
        }
        Ok(())
    }
}

#[test]
fn browser_sidecar_projects_real_aospkg_bytes_without_json_bootstrap() {
    let package_bytes = packed_browser_agent_fixture();
    let header = vfs::package_format::parse_aospkg_header(&package_bytes)
        .expect("parse fixture .aospkg header");
    let index =
        vfs::package_format::versioned::decode_mount_index(&package_bytes[header.index.clone()])
            .expect("decode fixture mount index");
    assert!(index
        .tar_entries
        .iter()
        .all(|entry| entry.path != "/agentos-package.json"));

    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(KernelVmConfig::new("vm-packed-agent"))
        .expect("create vm");
    let projection = sidecar
        .project_aospkg_bytes("vm-packed-agent", package_bytes)
        .expect("trusted sidecar projection must not require guest filesystem permission");

    assert_eq!(projection.name, "packed-agent");
    assert_eq!(projection.version, "1.2.3");
    assert_eq!(projection.commands, vec![String::from("packed-agent-acp")]);
    assert_eq!(
        projection.projected_commands,
        vec![BrowserProjectedCommand {
            name: String::from("packed-agent-acp"),
            guest_path: String::from("/opt/agentos/bin/packed-agent-acp"),
        }]
    );
    assert_eq!(
        projection.agent,
        Some(BrowserProjectedPackageAgent {
            id: String::from("packed-agent"),
            acp_entrypoint: String::from("packed-agent-acp"),
            adapter_entrypoint: String::from("/opt/agentos/bin/packed-agent-acp"),
            snapshot: false,
            env: [(String::from("PACKED_DEFAULT"), String::from("yes"))]
                .into_iter()
                .collect(),
            launch_args: vec![String::from("--packed-fixture")],
        })
    );
    assert_eq!(projection.applied_mounts, 4);
    assert_eq!(
        projection.provided_env,
        [(String::from("BASE_ENV"), String::from("from-package"))]
            .into_iter()
            .collect()
    );
    assert_eq!(projection.snapshot_bundle_path, None);
    let expected_agent = BrowserProjectedAgentLaunch {
        id: String::from("packed-agent"),
        adapter_entrypoint: String::from("/opt/agentos/bin/packed-agent-acp"),
        env: [(String::from("PACKED_DEFAULT"), String::from("yes"))]
            .into_iter()
            .collect(),
        launch_args: vec![String::from("--packed-fixture")],
    };
    assert_eq!(
        sidecar
            .resolve_projected_agent("vm-packed-agent", "packed-agent")
            .expect("resolve projected agent"),
        Some(expected_agent.clone())
    );
    assert_eq!(
        sidecar
            .list_projected_agents("vm-packed-agent")
            .expect("list projected agents"),
        vec![expected_agent]
    );
    assert_eq!(
        sidecar
            .provided_commands("vm-packed-agent")
            .expect("list provided commands"),
        vec![agentos_native_sidecar_browser::BrowserProvidedCommands {
            package_name: String::from("packed-agent"),
            commands: vec![String::from("packed-agent-acp")],
        }]
    );

    sidecar
        .configure_vm(
            "vm-packed-agent",
            Some(Permissions::allow_all()),
            None,
            None,
        )
        .expect("allow fixture reads after permission-independent projection");

    let entrypoint = b"export const fixture = 'packed';\n";
    assert_eq!(
        sidecar
            .read_file("vm-packed-agent", "/opt/agentos/bin/packed-agent-acp")
            .expect("read command alias"),
        entrypoint
    );
    assert_eq!(
        sidecar
            .read_file(
                "vm-packed-agent",
                "/opt/agentos/pkgs/packed-agent/current/bin/packed-agent-acp",
            )
            .expect("read current package alias"),
        entrypoint
    );
    assert_eq!(
        sidecar
            .read_file("vm-packed-agent", "/usr/local/share/packed/runtime.txt")
            .expect("read package-provided subtree"),
        b"runtime fixture\n"
    );
    assert!(sidecar
        .read_file(
            "vm-packed-agent",
            "/opt/agentos/pkgs/packed-agent/current/agentos-package.json",
        )
        .is_err());
    let write_error = sidecar
        .write_file(
            "vm-packed-agent",
            "/opt/agentos/bin/packed-agent-acp",
            b"mutated".to_vec(),
        )
        .expect_err("projected command must be read-only");
    assert!(write_error.to_string().contains("EROFS"));

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-packed-agent"),
            bootstrap_module: None,
        })
        .expect("create package env context");
    sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-packed-agent"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("env.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start execution with package-provided env");
    assert_eq!(
        sidecar
            .bridge()
            .browser_worker_spawns
            .last()
            .and_then(|spawn| spawn.get("process_env_base"))
            .map(String::as_str),
        Some("from-package")
    );
}

#[test]
fn browser_sidecar_rejects_invalid_package_batch_before_publishing_catalog() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-invalid-package"))
        .expect("create vm");

    let error = sidecar
        .project_aospkg_batch_bytes(
            "vm-invalid-package",
            vec![packed_browser_agent_fixture(), b"not-an-aospkg".to_vec()],
        )
        .expect_err("invalid staged package must reject whole batch");
    assert!(error.to_string().contains("invalid .aospkg"));
    assert!(sidecar
        .list_projected_agents("vm-invalid-package")
        .expect("list projected agents after rejection")
        .is_empty());
    assert!(sidecar
        .read_file("vm-invalid-package", "/opt/agentos/bin/packed-agent-acp")
        .is_err());
}

#[test]
fn browser_sidecar_replaces_and_clears_package_projection_at_custom_root() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-replace-package"))
        .expect("create vm");
    sidecar
        .project_aospkg_bytes("vm-replace-package", packed_browser_agent_fixture())
        .expect("project old package");

    let projected = sidecar
        .replace_aospkg_batch_bytes(
            "vm-replace-package",
            vec![packed_alternate_agent_fixture()],
            Some("/custom/agentos"),
        )
        .expect("replace package projection");
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].name, "alternate-agent");
    assert_eq!(
        projected[0].agent.as_ref().map(|agent| (
            agent.acp_entrypoint.as_str(),
            agent.adapter_entrypoint.as_str()
        )),
        Some((
            "alternate-agent-acp",
            "/custom/agentos/bin/alternate-agent-acp"
        ))
    );
    assert!(sidecar
        .read_file("vm-replace-package", "/opt/agentos/bin/packed-agent-acp")
        .is_err());
    assert_eq!(
        sidecar
            .read_file(
                "vm-replace-package",
                "/custom/agentos/bin/alternate-agent-acp",
            )
            .expect("read replacement command"),
        b"export const fixture = 'alternate';\n"
    );
    assert_eq!(
        sidecar
            .provided_commands("vm-replace-package")
            .expect("replacement provided commands"),
        vec![agentos_native_sidecar_browser::BrowserProvidedCommands {
            package_name: String::from("alternate-agent"),
            commands: vec![String::from("alternate-agent-acp")],
        }]
    );

    let linked = sidecar
        .project_aospkg_bytes("vm-replace-package", packed_browser_agent_fixture())
        .expect("link package at retained custom root");
    assert_eq!(
        linked
            .agent
            .as_ref()
            .map(|agent| agent.adapter_entrypoint.as_str()),
        Some("/custom/agentos/bin/packed-agent-acp")
    );
    assert_eq!(
        sidecar
            .read_file("vm-replace-package", "/custom/agentos/bin/packed-agent-acp",)
            .expect("read dynamically linked command at custom root"),
        b"export const fixture = 'packed';\n"
    );
    assert!(sidecar
        .read_file("vm-replace-package", "/opt/agentos/bin/packed-agent-acp")
        .is_err());

    assert!(sidecar
        .replace_aospkg_batch_bytes("vm-replace-package", Vec::new(), None)
        .expect("clear package projection")
        .is_empty());
    assert!(sidecar
        .list_projected_agents("vm-replace-package")
        .expect("agents after clear")
        .is_empty());
    assert!(sidecar
        .provided_commands("vm-replace-package")
        .expect("commands after clear")
        .is_empty());
    assert!(sidecar
        .read_file(
            "vm-replace-package",
            "/custom/agentos/bin/alternate-agent-acp",
        )
        .is_err());
}

#[test]
fn browser_sidecar_preserves_projection_when_replacement_staging_or_mount_fails() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm_with_root_filesystem(
            permissive_config("vm-replace-rollback"),
            RootFilesystemConfig {
                disable_default_base_layer: Some(true),
                bootstrap_entries: Some(vec![RootFilesystemEntry {
                    path: String::from("/blocker"),
                    kind: RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(String::from("not a directory")),
                    encoding: Some(RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }]),
                ..RootFilesystemConfig::default()
            },
        )
        .expect("create vm");
    sidecar
        .project_aospkg_bytes("vm-replace-rollback", packed_browser_agent_fixture())
        .expect("project old package");

    sidecar
        .replace_aospkg_batch_bytes(
            "vm-replace-rollback",
            vec![b"not-an-aospkg".to_vec()],
            Some("/custom/agentos"),
        )
        .expect_err("invalid replacement must fail before swap");
    sidecar
        .replace_aospkg_batch_bytes(
            "vm-replace-rollback",
            vec![packed_alternate_agent_fixture()],
            Some("relative/root"),
        )
        .expect_err("relative mount root must fail before swap");
    sidecar
        .replace_aospkg_batch_bytes(
            "vm-replace-rollback",
            vec![packed_alternate_agent_fixture()],
            Some("/custom/agentos"),
        )
        .expect_err("mount failure must roll back to old projection");

    assert_eq!(
        sidecar
            .read_file("vm-replace-rollback", "/opt/agentos/bin/packed-agent-acp",)
            .expect("old package mount survives failed replacement"),
        b"export const fixture = 'packed';\n"
    );
    assert_eq!(
        sidecar
            .list_projected_agents("vm-replace-rollback")
            .expect("old catalog survives failed replacement")[0]
            .id,
        "packed-agent"
    );
    assert!(sidecar
        .read_file(
            "vm-replace-rollback",
            "/custom/agentos/bin/alternate-agent-acp",
        )
        .is_err());
}

#[test]
fn browser_package_byte_cap_is_an_aggregate_sidecar_resource_bound() {
    assert_eq!(MAX_BROWSER_PROJECTED_PACKAGE_BYTES_PER_VM, 64 * 1024 * 1024);
}

fn packed_browser_agent_fixture() -> Vec<u8> {
    let mut source = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut source,
        "agentos-package.json",
        br#"{
          "name": "packed-agent",
          "version": "1.2.3",
          "agent": {
            "acpEntrypoint": "packed-agent-acp",
            "env": { "PACKED_DEFAULT": "yes" },
            "launchArgs": ["--packed-fixture"]
          },
          "provides": {
            "env": { "BASE_ENV": "from-package" },
            "files": [
              { "source": "share/runtime", "target": "/usr/local/share/packed" }
            ]
          }
        }"#,
        0o644,
        tar::EntryType::Regular,
    );
    append_tar_entry(
        &mut source,
        "bin/packed-agent-acp",
        b"export const fixture = 'packed';\n",
        0o755,
        tar::EntryType::Regular,
    );
    append_tar_entry(
        &mut source,
        "share/runtime/runtime.txt",
        b"runtime fixture\n",
        0o644,
        tar::EntryType::Regular,
    );
    let source = source.into_inner().expect("finish source tar");
    vfs::package_format::pack::pack_aospkg_from_tar_bytes(&source)
        .expect("pack fixture .aospkg")
        .0
}

fn packed_alternate_agent_fixture() -> Vec<u8> {
    let mut source = tar::Builder::new(Vec::new());
    append_tar_entry(
        &mut source,
        "agentos-package.json",
        br#"{
          "name": "alternate-agent",
          "version": "2.0.0",
          "agent": { "acpEntrypoint": "alternate-agent-acp" },
          "provides": {
            "files": [{ "source": "share/alternate", "target": "/blocker/target" }]
          }
        }"#,
        0o644,
        tar::EntryType::Regular,
    );
    append_tar_entry(
        &mut source,
        "share/alternate/data.txt",
        b"alternate data\n",
        0o644,
        tar::EntryType::Regular,
    );
    append_tar_entry(
        &mut source,
        "bin/alternate-agent-acp",
        b"export const fixture = 'alternate';\n",
        0o755,
        tar::EntryType::Regular,
    );
    let source = source.into_inner().expect("finish alternate source tar");
    vfs::package_format::pack::pack_aospkg_from_tar_bytes(&source)
        .expect("pack alternate fixture .aospkg")
        .0
}

fn append_tar_entry(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    contents: &[u8],
    mode: u32,
    entry_type: tar::EntryType,
) {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(entry_type);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(contents.len() as u64);
    header.set_cksum();
    builder
        .append_data(&mut header, path, Cursor::new(contents))
        .expect("append source tar entry");
}

fn permissive_config(vm_id: &str) -> KernelVmConfig {
    let mut config = KernelVmConfig::new(vm_id);
    config.permissions = Permissions::allow_all();
    config
}

fn test_process_config() -> BrowserWorkerProcessConfig {
    BrowserWorkerProcessConfig {
        cwd: String::from("/workspace"),
        env: BTreeMap::new(),
        argv: vec![String::from("node")],
        platform: String::from("linux"),
        arch: String::from("x64"),
        version: String::from("v22.0.0"),
        pid: 1,
        ppid: 0,
        uid: 1000,
        gid: 1000,
    }
}

fn test_os_config() -> BrowserWorkerOsConfig {
    BrowserWorkerOsConfig {
        platform: String::from("linux"),
        arch: String::from("x64"),
        r#type: String::from("Linux"),
        release: String::from("6.8.0-secure-exec"),
        version: String::from("#1 SMP PREEMPT_DYNAMIC secure-exec"),
        cpu_count: 1,
        totalmem: 1024 * 1024 * 1024,
        freemem: 512 * 1024 * 1024,
        hostname: String::from("secure-exec"),
        homedir: String::from("/home/user"),
        tmpdir: String::from("/tmp"),
        machine: String::from("x86_64"),
        user: String::from("user"),
        shell: String::from("/bin/sh"),
        uid: 1000,
        gid: 1000,
    }
}

#[test]
fn browser_sidecar_runs_guest_javascript_from_main_thread_workers() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start JavaScript execution");
    let execution_id = started.execution_id.clone();
    let worker_id = format!("js-worker-{}", context.context_id);

    assert_eq!(sidecar.sidecar_id(), "agentos-native-sidecar-browser");
    assert_eq!(sidecar.vm_count(), 1);
    assert_eq!(sidecar.context_count("vm-browser"), 1);
    assert_eq!(sidecar.active_worker_count("vm-browser"), 1);

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: execution_id.clone(),
            exit_code: 0,
        }));
    let event = sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll execution event");

    assert!(matches!(
        event,
        Some(ExecutionEvent::Exited(ExecutionExited {
            execution_id,
            exit_code: 0,
            ..
        })) if execution_id == started.execution_id
    ));
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);

    let bridge = sidecar.into_bridge();
    assert_eq!(
        bridge.terminated_workers,
        vec![(String::from("vm-browser"), execution_id, worker_id)]
    );
    let states = bridge
        .lifecycle_events
        .iter()
        .map(|event| event.state)
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        vec![
            LifecycleState::Starting,
            LifecycleState::Ready,
            LifecycleState::Busy,
            LifecycleState::Ready,
        ]
    );
    let structured_names = bridge
        .structured_events
        .iter()
        .map(|event| event.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        structured_names,
        vec![
            "browser.context.created",
            "browser.worker.spawned",
            "browser.worker.reaped",
        ]
    );
}

#[test]
fn browser_sidecar_abort_releases_worker_after_bridge_kill_failure() {
    let mut bridge = RecordingBridge::default();
    bridge.push_execution_kill_error("forced bridge kill failure");
    let mut sidecar = BrowserSidecar::new(bridge, BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: None,
        })
        .expect("create context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start execution");
    let execution_id = started.execution_id;

    let error = sidecar
        .abort_execution("vm-browser", &execution_id)
        .expect_err("bridge kill failure must surface after release");
    assert!(error.to_string().contains("forced bridge kill failure"));
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);

    let bridge = sidecar.into_bridge();
    assert_eq!(bridge.killed_executions.len(), 1);
    assert_eq!(
        bridge.terminated_workers,
        vec![(
            String::from("vm-browser"),
            execution_id,
            format!("js-worker-{}", context.context_id),
        )]
    );
}

#[test]
fn browser_sidecar_diagnostic_failures_do_not_orphan_execution_or_context() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-diagnostic-failure"))
        .expect("create vm");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-diagnostic-failure"),
            bootstrap_module: None,
        })
        .expect("create context");

    sidecar
        .bridge_mut()
        .push_structured_event_error("injected start diagnostic failure");
    sidecar
        .bridge_mut()
        .push_lifecycle_event_error("injected busy lifecycle failure");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-diagnostic-failure"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("diagnostic failure must not fail a committed execution");
    assert_eq!(sidecar.active_worker_count("vm-diagnostic-failure"), 1);

    sidecar
        .bridge_mut()
        .push_structured_event_error("injected release diagnostic failure");
    sidecar
        .bridge_mut()
        .push_lifecycle_event_error("injected ready lifecycle failure");
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-diagnostic-failure"),
            execution_id: started.execution_id.clone(),
            exit_code: 0,
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-diagnostic-failure"),
        })
        .expect_err("cleanup diagnostics are tracked retryable phases");
    assert_eq!(sidecar.active_worker_count("vm-diagnostic-failure"), 0);
    sidecar
        .release_execution(&started.execution_id, "browser.worker.reaped")
        .expect("retry emits only the failed cleanup diagnostics");

    sidecar
        .bridge_mut()
        .push_structured_event_error("injected context-release diagnostic failure");
    sidecar
        .release_context("vm-diagnostic-failure", &context.context_id)
        .expect("context cleanup is committed before its diagnostic");
    assert_eq!(sidecar.context_count("vm-diagnostic-failure"), 0);
}

#[test]
fn filtered_execution_output_preserves_other_executions_and_central_events() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: None,
        })
        .expect("create context");
    let start = |sidecar: &mut BrowserSidecar<RecordingBridge>, name: &str| {
        sidecar
            .start_execution(StartExecutionRequest {
                vm_id: String::from("vm-browser"),
                context_id: context.context_id.clone(),
                argv: vec![name.to_string()],
                env: BTreeMap::new(),
                cwd: String::from("/workspace"),
            })
            .expect("start execution")
            .execution_id
    };
    let execution_a = start(&mut sidecar, "agent-a");
    let execution_b = start(&mut sidecar, "agent-b");

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::GuestRequest(GuestKernelCall {
            vm_id: String::from("vm-browser"),
            execution_id: execution_a.clone(),
            operation: String::from("fs.read"),
            payload: b"request".to_vec(),
        }));
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::SignalState(ExecutionSignalState {
            vm_id: String::from("vm-browser"),
            execution_id: execution_a.clone(),
            signal: 15,
            registration: SignalHandlerRegistration {
                action: SignalDispositionAction::User,
                mask: vec![],
                flags: 0,
            },
        }));
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Stdout(OutputChunk {
            vm_id: String::from("vm-browser"),
            execution_id: execution_b.clone(),
            chunk: b"from-b".to_vec(),
        }));
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Stdout(OutputChunk {
            vm_id: String::from("vm-browser"),
            execution_id: execution_a.clone(),
            chunk: b"from-a".to_vec(),
        }));

    let poll_a = || PollExecutionOutputRequest {
        vm_id: String::from("vm-browser"),
        execution_id: execution_a.clone(),
    };
    assert_eq!(
        sidecar
            .poll_execution_output(poll_a())
            .expect("defer guest request"),
        None
    );
    assert_eq!(
        sidecar
            .poll_execution_output(poll_a())
            .expect("defer signal state"),
        None
    );
    assert_eq!(
        sidecar
            .poll_execution_output(poll_a())
            .expect("defer B output"),
        None
    );
    assert!(matches!(
        sidecar.poll_execution_output(poll_a()).expect("poll A output"),
        Some(ExecutionOutput::Stdout(OutputChunk { chunk, .. })) if chunk == b"from-a"
    ));

    assert!(matches!(
        sidecar
            .poll_execution_event(PollExecutionEventRequest {
                vm_id: String::from("vm-browser"),
            })
            .expect("central guest request poll"),
        Some(ExecutionEvent::GuestRequest(GuestKernelCall { operation, .. }))
            if operation == "fs.read"
    ));
    assert!(matches!(
        sidecar
            .poll_execution_event(PollExecutionEventRequest {
                vm_id: String::from("vm-browser"),
            })
            .expect("central signal poll"),
        Some(ExecutionEvent::SignalState(ExecutionSignalState {
            signal: 15,
            ..
        }))
    ));
    assert!(matches!(
        sidecar
            .poll_execution_output(PollExecutionOutputRequest {
                vm_id: String::from("vm-browser"),
                execution_id: execution_b.clone(),
            })
            .expect("poll preserved B output"),
        Some(ExecutionOutput::Stdout(OutputChunk { chunk, .. })) if chunk == b"from-b"
    ));

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: execution_b.clone(),
            exit_code: 2,
        }));
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: execution_a.clone(),
            exit_code: 1,
        }));
    assert_eq!(
        sidecar
            .poll_execution_output(poll_a())
            .expect("defer B exit"),
        None
    );
    assert!(matches!(
        sidecar
            .poll_execution_output(poll_a())
            .expect("poll A exit"),
        Some(ExecutionOutput::Exited(ExecutionExited {
            exit_code: 1,
            ..
        }))
    ));
    assert!(matches!(
        sidecar
            .poll_execution_output(PollExecutionOutputRequest {
                vm_id: String::from("vm-browser"),
                execution_id: execution_b,
            })
            .expect("poll preserved B exit"),
        Some(ExecutionOutput::Exited(ExecutionExited {
            exit_code: 2,
            ..
        }))
    ));
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);
}

#[test]
fn filtered_execution_output_backpressures_before_consuming_past_its_bound() {
    let mut sidecar = BrowserSidecar::new(
        RecordingBridge::default(),
        BrowserSidecarConfig {
            max_deferred_execution_events_per_vm: 1,
            ..BrowserSidecarConfig::default()
        },
    );
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: None,
        })
        .expect("create context");
    let execution_a = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("agent-a")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start A")
        .execution_id;
    let execution_b = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("agent-b")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start B")
        .execution_id;
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::GuestRequest(GuestKernelCall {
            vm_id: String::from("vm-browser"),
            execution_id: execution_a.clone(),
            operation: String::from("fs.read"),
            payload: vec![],
        }));
    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Stdout(OutputChunk {
            vm_id: String::from("vm-browser"),
            execution_id: execution_b.clone(),
            chunk: b"still-on-bridge".to_vec(),
        }));
    let poll_a = PollExecutionOutputRequest {
        vm_id: String::from("vm-browser"),
        execution_id: execution_a,
    };
    assert_eq!(
        sidecar
            .poll_execution_output(poll_a.clone())
            .expect("defer guest request"),
        None
    );
    let error = sidecar
        .poll_execution_output(poll_a)
        .expect_err("full deferred queue must backpressure before another bridge poll");
    assert!(matches!(
        error,
        agentos_native_sidecar_browser::BrowserSidecarError::LimitExceeded {
            limit: "max_deferred_execution_events_per_vm",
            capacity: 1,
            ..
        }
    ));

    assert!(matches!(
        sidecar
            .poll_execution_event(PollExecutionEventRequest {
                vm_id: String::from("vm-browser"),
            })
            .expect("drain guest request"),
        Some(ExecutionEvent::GuestRequest(_))
    ));
    assert!(matches!(
        sidecar
            .poll_execution_output(PollExecutionOutputRequest {
                vm_id: String::from("vm-browser"),
                execution_id: execution_b,
            })
            .expect("B output was not consumed on overflow"),
        Some(ExecutionOutput::Stdout(OutputChunk { chunk, .. }))
            if chunk == b"still-on-bridge"
    ));
}

#[test]
fn browser_worker_spawn_receives_virtual_identity_config() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    let mut config = permissive_config("vm-browser");
    config
        .env
        .insert(String::from("BASE_ENV"), String::from("base"));
    config.user.username = Some(String::from("runner"));
    config.user.uid = Some(501);
    config.user.gid = Some(20);
    config.user.homedir = Some(String::from("/home/runner"));
    config.user.shell = Some(String::from("/bin/bash"));
    config.resources = ResourceLimits {
        virtual_cpu_count: Some(4),
        max_wasm_memory_bytes: Some(256 * 1024 * 1024),
        ..ResourceLimits::default()
    };
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: None,
        })
        .expect("create JavaScript context");
    sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("identity.js")],
            env: BTreeMap::from([(String::from("EXEC_ENV"), String::from("exec"))]),
            cwd: String::from("/workspace"),
        })
        .expect("start execution");

    let bridge = sidecar.into_bridge();
    let spawn = bridge
        .browser_worker_spawns
        .last()
        .expect("worker spawn should be recorded");
    assert_eq!(
        spawn.get("process_platform").map(String::as_str),
        Some("linux")
    );
    assert_eq!(spawn.get("process_arch").map(String::as_str), Some("x64"));
    assert_eq!(
        spawn.get("process_cwd").map(String::as_str),
        Some("/workspace")
    );
    assert_eq!(
        spawn.get("execution_id").map(String::as_str),
        Some("exec-1")
    );
    assert_eq!(
        spawn.get("process_env_base").map(String::as_str),
        Some("base")
    );
    assert_eq!(
        spawn.get("process_env_exec").map(String::as_str),
        Some("exec")
    );
    assert_eq!(spawn.get("os_platform").map(String::as_str), Some("linux"));
    assert_eq!(spawn.get("os_cpu_count").map(String::as_str), Some("4"));
    assert_eq!(
        spawn.get("os_totalmem").map(String::as_str),
        Some("268435456")
    );
    assert_eq!(
        spawn.get("os_freemem").map(String::as_str),
        Some("268435456")
    );
    assert_eq!(spawn.get("os_user").map(String::as_str), Some("runner"));
    assert_eq!(
        spawn.get("os_homedir").map(String::as_str),
        Some("/home/runner")
    );
    assert_eq!(
        spawn.get("os_hostname").map(String::as_str),
        Some("secure-exec")
    );
    assert_eq!(spawn.get("os_type").map(String::as_str), Some("Linux"));
    assert_eq!(
        spawn.get("os_release").map(String::as_str),
        Some("6.8.0-secure-exec")
    );
    assert_eq!(
        spawn.get("os_version").map(String::as_str),
        Some("#1 SMP PREEMPT_DYNAMIC secure-exec")
    );
    assert_eq!(spawn.get("os_tmpdir").map(String::as_str), Some("/tmp"));
    assert_eq!(spawn.get("os_machine").map(String::as_str), Some("x86_64"));
    assert!(
        spawn
            .get("process_pid")
            .and_then(|pid| pid.parse::<u32>().ok())
            .is_some_and(|pid| pid > 0),
        "worker process pid should come from the kernel"
    );
    assert_eq!(spawn.get("process_uid").map(String::as_str), Some("501"));
    assert_eq!(spawn.get("process_gid").map(String::as_str), Some("20"));
    assert_eq!(spawn.get("os_uid").map(String::as_str), Some("501"));
    assert_eq!(spawn.get("os_gid").map(String::as_str), Some("20"));
}

#[test]
fn browser_sidecar_runs_guest_wasm_from_main_thread_workers() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");

    let context = sidecar
        .create_wasm_context(CreateWasmContextRequest {
            vm_id: String::from("vm-browser"),
            module_path: Some(String::from("/workspace/app.wasm")),
        })
        .expect("create WebAssembly context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("wasm"), String::from("/workspace/app.wasm")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start WebAssembly execution");

    assert_eq!(sidecar.context_count("vm-browser"), 1);
    assert_eq!(sidecar.active_worker_count("vm-browser"), 1);

    sidecar
        .kill_execution(KillExecutionRequest {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id,
            signal: ExecutionSignal::Kill,
        })
        .expect("kill execution");
    sidecar.dispose_vm("vm-browser").expect("dispose vm");

    assert_eq!(sidecar.vm_count(), 0);

    let bridge = sidecar.into_bridge();
    assert_eq!(bridge.killed_executions.len(), 1);
    assert_eq!(
        bridge
            .browser_worker_spawns
            .first()
            .and_then(|spawn| spawn.get("wasm_permission_tier"))
            .map(String::as_str),
        Some("Full")
    );
    assert_eq!(
        bridge
            .lifecycle_events
            .last()
            .expect("final lifecycle event")
            .state,
        LifecycleState::Terminated
    );
    assert!(bridge.structured_events.iter().any(|event| {
        event.name == "browser.worker.spawned"
            && event.fields.get("runtime") == Some(&String::from("webassembly"))
    }));
}

#[test]
fn browser_worker_spawn_requests_preserve_browser_entrypoints() {
    let javascript = BrowserWorkerSpawnRequest {
        vm_id: String::from("vm-browser"),
        context_id: String::from("ctx-js"),
        execution_id: String::from("exec-js"),
        runtime: GuestRuntime::JavaScript,
        entrypoint: BrowserWorkerEntrypoint::JavaScript {
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        },
        wasm_permission_tier: None,
        process_config: test_process_config(),
        os_config: test_os_config(),
    };
    let wasm = BrowserWorkerSpawnRequest {
        vm_id: String::from("vm-browser"),
        context_id: String::from("ctx-wasm"),
        execution_id: String::from("exec-wasm"),
        runtime: GuestRuntime::WebAssembly,
        entrypoint: BrowserWorkerEntrypoint::WebAssembly {
            module_path: Some(String::from("/workspace/app.wasm")),
        },
        wasm_permission_tier: Some(WasmPermissionTier::ReadOnly),
        process_config: test_process_config(),
        os_config: test_os_config(),
    };

    assert!(matches!(
        javascript.entrypoint,
        BrowserWorkerEntrypoint::JavaScript {
            bootstrap_module: Some(_)
        }
    ));
    assert!(matches!(
        wasm.entrypoint,
        BrowserWorkerEntrypoint::WebAssembly {
            module_path: Some(_)
        }
    ));
}

#[test]
fn browser_sidecar_routes_kernel_filesystem_and_execution_state_through_vm_state() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");

    assert_eq!(
        sidecar.kernel_state("vm-browser").expect("kernel ready"),
        LifecycleState::Ready
    );

    sidecar
        .mkdir("vm-browser", "/workspace", true)
        .expect("create workspace");
    sidecar
        .write_file("vm-browser", "/workspace/hello.txt", b"hello".to_vec())
        .expect("write kernel file");
    assert_eq!(
        sidecar
            .read_file("vm-browser", "/workspace/hello.txt")
            .expect("read kernel file"),
        b"hello".to_vec()
    );
    assert_eq!(
        sidecar
            .read_dir("vm-browser", "/workspace")
            .expect("read workspace"),
        vec![String::from("hello.txt")]
    );

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::from([(String::from("MODE"), String::from("browser"))]),
            cwd: String::from("/workspace"),
        })
        .expect("start execution");

    assert_eq!(
        sidecar.kernel_state("vm-browser").expect("kernel busy"),
        LifecycleState::Busy
    );

    sidecar
        .write_stdin(agentos_bridge::WriteExecutionStdinRequest {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id.clone(),
            chunk: b"input".to_vec(),
        })
        .expect("write stdin");
    assert_eq!(
        sidecar
            .read_execution_stdin(
                "vm-browser",
                &started.execution_id,
                16,
                Duration::from_millis(5),
            )
            .expect("read stdin"),
        Some(b"input".to_vec())
    );

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::GuestRequest(GuestKernelCall {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id.clone(),
            operation: String::from("fs.read"),
            payload: b"{\"path\":\"/workspace/input.txt\"}".to_vec(),
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll guest kernel call");
    let guest_call_events = sidecar
        .bridge()
        .structured_events
        .iter()
        .filter(|event| event.name == "guest.kernel_call.unsupported")
        .collect::<Vec<_>>();
    assert_eq!(guest_call_events.len(), 1);
    assert_eq!(
        guest_call_events[0].fields["execution_id"],
        started.execution_id
    );
    assert_eq!(guest_call_events[0].fields["operation"], "fs.read");
    assert_eq!(
        guest_call_events[0].fields["payload_size_bytes"],
        b"{\"path\":\"/workspace/input.txt\"}".len().to_string()
    );

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::SignalState(ExecutionSignalState {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id.clone(),
            signal: 15,
            registration: SignalHandlerRegistration {
                action: SignalDispositionAction::User,
                mask: vec![2],
                flags: 0,
            },
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll signal state");
    let signal_state = sidecar
        .signal_state("vm-browser", &started.execution_id)
        .expect("signal state");
    let sigterm = signal_state.get(&15).expect("SIGTERM handler");
    assert_eq!(
        sigterm.action,
        agentos_sidecar_protocol::wire::SignalDispositionAction::User
    );
    assert_eq!(sigterm.mask, vec![2]);

    sidecar
        .create_kernel_tcp_listener_for_execution(
            "vm-browser",
            &started.execution_id,
            "127.0.0.1",
            34567,
            16,
        )
        .expect("create listener owned by execution");
    sidecar
        .create_kernel_bound_udp_for_execution(
            "vm-browser",
            &started.execution_id,
            "127.0.0.1",
            34568,
        )
        .expect("create UDP binding owned by execution");
    assert!(sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34567),
                path: None,
            },
        )
        .expect("find listener before exit")
        .is_some());
    assert!(sidecar
        .find_bound_udp(
            "vm-browser",
            &FindBoundUdpRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34568),
            },
        )
        .expect("find UDP binding before exit")
        .is_some());

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id.clone(),
            exit_code: 0,
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll exit");

    assert_eq!(
        sidecar
            .kernel_state("vm-browser")
            .expect("kernel ready after exit"),
        LifecycleState::Ready
    );
    assert!(sidecar
        .signal_state("vm-browser", &started.execution_id)
        .expect("signal state after exit")
        .is_empty());
    assert!(sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34567),
                path: None,
            },
        )
        .expect("find listener after exit")
        .is_none());
    assert!(sidecar
        .find_bound_udp(
            "vm-browser",
            &FindBoundUdpRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34568),
            },
        )
        .expect("find UDP binding after exit")
        .is_none());
}

#[test]
fn browser_sidecar_routes_guest_kernel_call_net_loopback_through_vm_state() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start execution");

    // The synchronous guest kernel-call path (`GuestKernelCallRequest` ->
    // shared `handle_guest_kernel_call` -> kernel sockets) is the converged
    // replacement for the fire-and-forget `GuestRequest` bail. Drive a full
    // loopback TCP exchange through it to prove the wiring end to end.
    let call = |sidecar: &mut BrowserSidecar<RecordingBridge>,
                operation: &str,
                request: serde_json::Value|
     -> serde_json::Value {
        let result = sidecar
            .guest_kernel_call(
                "vm-browser",
                GuestKernelCallRequest {
                    execution_id: started.execution_id.clone(),
                    operation: String::from(operation),
                    payload: serde_json::to_vec(&request).expect("encode request"),
                },
            )
            .expect("guest kernel call");
        serde_json::from_slice(&result.payload).expect("decode response")
    };

    let listener = call(
        &mut sidecar,
        "net.listen",
        serde_json::json!({ "host": "127.0.0.1", "port": 39221 }),
    );
    let listener_id = listener["socketId"].as_u64().expect("listener socket id");

    let client = call(
        &mut sidecar,
        "net.connect",
        serde_json::json!({ "host": "127.0.0.1", "port": 39221 }),
    );
    let client_id = client["socketId"].as_u64().expect("client socket id");

    let accepted = call(
        &mut sidecar,
        "net.accept",
        serde_json::json!({ "socketId": listener_id }),
    );
    let accepted_id = accepted["socketId"].as_u64().expect("accepted socket id");

    // base64("hello") == "aGVsbG8=" (asserted directly so no decode is needed).
    let write = call(
        &mut sidecar,
        "net.write",
        serde_json::json!({ "socketId": client_id, "data": "aGVsbG8=" }),
    );
    assert_eq!(write["written"].as_u64(), Some(5));

    let read = call(
        &mut sidecar,
        "net.read",
        serde_json::json!({ "socketId": accepted_id }),
    );
    assert_eq!(read["closed"].as_bool(), Some(false));
    assert_eq!(read["data"].as_str(), Some("aGVsbG8="));

    // Unknown operations surface as a rejected guest kernel call rather than a
    // silent fail-open.
    let error = sidecar
        .guest_kernel_call(
            "vm-browser",
            GuestKernelCallRequest {
                execution_id: started.execution_id.clone(),
                operation: String::from("net.teleport"),
                payload: b"{}".to_vec(),
            },
        )
        .expect_err("unsupported operation rejected");
    assert!(error
        .to_string()
        .contains("unsupported guest kernel call operation: net.teleport"));
}

#[test]
fn browser_sidecar_keeps_kernel_sockets_scoped_per_execution() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(permissive_config("vm-browser"))
        .expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");

    let first = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node"), String::from("first.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start first execution");
    let second = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("second.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start second execution");

    sidecar
        .create_kernel_tcp_listener_for_execution(
            "vm-browser",
            &first.execution_id,
            "127.0.0.1",
            34601,
            16,
        )
        .expect("create first listener");
    sidecar
        .create_kernel_tcp_listener_for_execution(
            "vm-browser",
            &second.execution_id,
            "127.0.0.1",
            34602,
            16,
        )
        .expect("create second listener");
    sidecar
        .create_kernel_bound_udp_for_execution(
            "vm-browser",
            &first.execution_id,
            "127.0.0.1",
            34611,
        )
        .expect("create first UDP binding");
    sidecar
        .create_kernel_bound_udp_for_execution(
            "vm-browser",
            &second.execution_id,
            "127.0.0.1",
            34612,
        )
        .expect("create second UDP binding");

    let second_listener = sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34602),
                path: None,
            },
        )
        .expect("find second listener before exit")
        .expect("second listener exists");
    assert_eq!(second_listener.process_id, second.execution_id);

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: first.execution_id.clone(),
            exit_code: 0,
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll first exit");

    assert!(sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34601),
                path: None,
            },
        )
        .expect("find first listener after exit")
        .is_none());
    assert!(sidecar
        .find_bound_udp(
            "vm-browser",
            &FindBoundUdpRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34611),
            },
        )
        .expect("find first UDP binding after exit")
        .is_none());

    let second_listener = sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34602),
                path: None,
            },
        )
        .expect("find second listener after first exit")
        .expect("second listener remains");
    assert_eq!(second_listener.process_id, second.execution_id);
    let second_udp = sidecar
        .find_bound_udp(
            "vm-browser",
            &FindBoundUdpRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34612),
            },
        )
        .expect("find second UDP binding after first exit")
        .expect("second UDP binding remains");
    assert_eq!(second_udp.process_id, second.execution_id);
    assert_eq!(
        sidecar.kernel_state("vm-browser").expect("kernel state"),
        LifecycleState::Busy
    );
}

#[test]
fn browser_sidecar_kernel_tcp_listener_obeys_vm_network_policy() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_callback = Arc::clone(&requests);
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions {
        network: Some(Arc::new(move |request: &NetworkAccessRequest| {
            requests_for_callback
                .lock()
                .expect("request log lock")
                .push(request.clone());
            PermissionDecision::deny("network disabled")
        })),
        ..Permissions::allow_all()
    };

    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");
    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("server.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start execution");

    let error = sidecar
        .create_kernel_tcp_listener_for_execution(
            "vm-browser",
            &started.execution_id,
            "127.0.0.1",
            34621,
            16,
        )
        .expect_err("TCP listener should be denied by network policy");

    assert!(
        error.to_string().contains("EACCES"),
        "unexpected error: {error}"
    );
    assert!(sidecar
        .find_listener(
            "vm-browser",
            &FindListenerRequest {
                host: Some(String::from("127.0.0.1")),
                port: Some(34621),
                path: None,
            },
        )
        .expect("find listener after denied bind")
        .is_none());
    assert_eq!(
        *requests.lock().expect("request log lock"),
        vec![NetworkAccessRequest {
            vm_id: String::from("vm-browser"),
            op: NetworkOperation::Listen,
            resource: String::from("tcp://127.0.0.1:34621"),
        }]
    );
}

#[test]
fn browser_sidecar_reaps_kernel_process_after_normal_execution_exit() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_processes: Some(1),
        ..ResourceLimits::default()
    };
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");

    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("start first execution");

    sidecar
        .bridge_mut()
        .push_execution_event(ExecutionEvent::Exited(ExecutionExited {
            vm_id: String::from("vm-browser"),
            execution_id: started.execution_id,
            exit_code: 0,
        }));
    sidecar
        .poll_execution_event(PollExecutionEventRequest {
            vm_id: String::from("vm-browser"),
        })
        .expect("poll exit");

    sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("completed execution should not leak the one-process limit");
}

#[test]
fn browser_sidecar_preserves_default_deny_kernel_permissions() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm(KernelVmConfig::new("vm-browser"))
        .expect("create vm");

    let error = sidecar
        .write_file("vm-browser", "/workspace/denied.txt", b"denied".to_vec())
        .expect_err("default permissions should deny filesystem writes");

    // Under default deny-all the write's parent-directory resolution is the
    // first denied operation, so the error reports the read denial on the
    // parent; the write itself never proceeds.
    assert_eq!(
        error.to_string(),
        "EACCES: permission denied, read '/workspace'"
    );
}

#[test]
fn browser_sidecar_builds_mount_table_root_from_root_filesystem_config() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm_with_root_filesystem(
            permissive_config("vm-browser"),
            RootFilesystemConfig {
                disable_default_base_layer: Some(true),
                lowers: Some(vec![
                    RootFilesystemLowerDescriptor::Snapshot {
                        entries: vec![
                            RootFilesystemEntry {
                                path: String::from("/workspace/shared.txt"),
                                kind: RootFilesystemEntryKind::File,
                                mode: None,
                                uid: None,
                                gid: None,
                                content: Some(String::from("higher")),
                                encoding: Some(RootFilesystemEntryEncoding::Utf8),
                                target: None,
                                executable: false,
                            },
                            RootFilesystemEntry {
                                path: String::from("/workspace/higher-only.txt"),
                                kind: RootFilesystemEntryKind::File,
                                mode: None,
                                uid: None,
                                gid: None,
                                content: Some(String::from("higher-only")),
                                encoding: Some(RootFilesystemEntryEncoding::Utf8),
                                target: None,
                                executable: false,
                            },
                        ],
                    },
                    RootFilesystemLowerDescriptor::Snapshot {
                        entries: vec![
                            RootFilesystemEntry {
                                path: String::from("/workspace/shared.txt"),
                                kind: RootFilesystemEntryKind::File,
                                mode: None,
                                uid: None,
                                gid: None,
                                content: Some(String::from("lower")),
                                encoding: Some(RootFilesystemEntryEncoding::Utf8),
                                target: None,
                                executable: false,
                            },
                            RootFilesystemEntry {
                                path: String::from("/workspace/lower-only.txt"),
                                kind: RootFilesystemEntryKind::File,
                                mode: None,
                                uid: None,
                                gid: None,
                                content: Some(String::from("lower-only")),
                                encoding: Some(RootFilesystemEntryEncoding::Utf8),
                                target: None,
                                executable: false,
                            },
                        ],
                    },
                ]),
                bootstrap_entries: Some(vec![
                    RootFilesystemEntry {
                        path: String::from("/workspace/shared.txt"),
                        kind: RootFilesystemEntryKind::File,
                        mode: None,
                        uid: None,
                        gid: None,
                        content: Some(String::from("upper")),
                        encoding: Some(RootFilesystemEntryEncoding::Utf8),
                        target: None,
                        executable: false,
                    },
                    RootFilesystemEntry {
                        path: String::from("/workspace/upper-only.txt"),
                        kind: RootFilesystemEntryKind::File,
                        mode: None,
                        uid: None,
                        gid: None,
                        content: Some(String::from("upper-only")),
                        encoding: Some(RootFilesystemEntryEncoding::Utf8),
                        target: None,
                        executable: false,
                    },
                ]),
                ..RootFilesystemConfig::default()
            },
        )
        .expect("create vm with root filesystem config");

    for (path, expected) in [
        ("/workspace/shared.txt", b"upper".as_slice()),
        ("/workspace/higher-only.txt", b"higher-only".as_slice()),
        ("/workspace/lower-only.txt", b"lower-only".as_slice()),
        ("/workspace/upper-only.txt", b"upper-only".as_slice()),
    ] {
        assert_eq!(
            sidecar
                .read_file("vm-browser", path)
                .unwrap_or_else(|error| panic!("read {path}: {error}")),
            expected.to_vec()
        );
    }
    sidecar
        .write_file("vm-browser", "/workspace/new.txt", b"new-upper".to_vec())
        .expect("write upper entry");

    let snapshot = sidecar
        .snapshot_root_filesystem("vm-browser")
        .expect("snapshot root filesystem");
    assert!(snapshot.entries.iter().any(|entry| {
        entry.path == "/workspace/new.txt"
            && entry.kind == FilesystemEntryKind::File
            && entry.content.as_deref() == Some(b"new-upper".as_slice())
    }));
}

#[test]
fn browser_sidecar_locks_read_only_root_after_bootstrap() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    sidecar
        .create_vm_with_root_filesystem(
            permissive_config("vm-browser"),
            RootFilesystemConfig {
                mode: Some(RootFilesystemMode::ReadOnly),
                disable_default_base_layer: Some(true),
                bootstrap_entries: Some(vec![RootFilesystemEntry {
                    path: String::from("/workspace/bootstrap.txt"),
                    kind: RootFilesystemEntryKind::File,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: Some(String::from("bootstrapped")),
                    encoding: Some(RootFilesystemEntryEncoding::Utf8),
                    target: None,
                    executable: false,
                }]),
                ..RootFilesystemConfig::default()
            },
        )
        .expect("create read-only VM with bootstrap entries");

    assert_eq!(
        sidecar
            .read_file("vm-browser", "/workspace/bootstrap.txt")
            .expect("read bootstrap entry"),
        b"bootstrapped".to_vec()
    );

    let error = sidecar
        .write_file("vm-browser", "/workspace/new.txt", b"new".to_vec())
        .expect_err("read-only root should reject writes after bootstrap");
    assert_eq!(
        error.to_string(),
        "EROFS: read-only filesystem: /workspace/new.txt"
    );
}

#[test]
fn browser_sidecar_reaps_pending_kernel_process_when_worker_startup_fails() {
    let mut bridge = RecordingBridge::default();
    bridge.push_worker_create_error("worker startup failed");

    let mut sidecar = BrowserSidecar::new(bridge, BrowserSidecarConfig::default());
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_processes: Some(1),
        ..ResourceLimits::default()
    };
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");

    let failed = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect_err("worker creation should fail");

    assert!(failed.to_string().contains("worker startup failed"));
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);
    assert_eq!(sidecar.bridge().killed_executions.len(), 1);
    assert_eq!(sidecar.bridge().killed_executions[0].execution_id, "exec-1");
    assert_eq!(
        sidecar.kernel_state("vm-browser").expect("kernel ready"),
        LifecycleState::Ready
    );

    let started = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("node"), String::from("script.js")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("leaked pending process would exhaust the one-process limit");

    assert_eq!(started.execution_id, "exec-2");
    assert_eq!(sidecar.active_worker_count("vm-browser"), 1);
}

#[test]
fn browser_sidecar_retains_partial_startup_cleanup_and_charges_admission() {
    let mut bridge = RecordingBridge::default();
    bridge.push_worker_create_error("worker startup failed");
    bridge.push_execution_kill_error("first rollback kill failed");
    bridge.push_execution_kill_error("retry rollback kill failed");
    let mut sidecar = BrowserSidecar::new(
        bridge,
        BrowserSidecarConfig {
            max_pending_execution_cleanups_per_vm: 1,
            ..BrowserSidecarConfig::default()
        },
    );
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions::allow_all();
    sidecar.create_vm(config).expect("create VM");
    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("entry.js")),
        })
        .expect("create context");
    let request = StartExecutionRequest {
        vm_id: String::from("vm-browser"),
        context_id: context.context_id,
        argv: vec![String::from("node"), String::from("entry.js")],
        env: BTreeMap::new(),
        cwd: String::from("/workspace"),
    };

    let first = sidecar
        .start_execution(request.clone())
        .expect_err("worker and rollback failure retain cleanup");
    assert!(first.to_string().contains("first rollback kill failed"));
    assert_eq!(sidecar.pending_execution_cleanup_count("vm-browser"), 1);
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);

    let retry_error = sidecar
        .start_execution(request.clone())
        .expect_err("failed cleanup retry blocks replacement startup");
    assert!(retry_error
        .to_string()
        .contains("retry rollback kill failed"));
    assert_eq!(sidecar.pending_execution_cleanup_count("vm-browser"), 1);

    let started = sidecar
        .start_execution(request)
        .expect("successful cleanup retry releases admission");
    assert_eq!(started.execution_id, "exec-2");
    assert_eq!(sidecar.pending_execution_cleanup_count("vm-browser"), 0);
    assert_eq!(sidecar.active_worker_count("vm-browser"), 1);
}

#[test]
fn browser_sidecar_reaps_pending_kernel_process_when_stdio_setup_fails() {
    let mut sidecar =
        BrowserSidecar::new(RecordingBridge::default(), BrowserSidecarConfig::default());
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_processes: Some(1),
        max_pipes: Some(0),
        ..ResourceLimits::default()
    };
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_javascript_context(CreateJavascriptContextRequest {
            vm_id: String::from("vm-browser"),
            bootstrap_module: Some(String::from("@rivet-dev/agentos-runtime-browser")),
        })
        .expect("create JavaScript context");

    for _ in 0..2 {
        let failed = sidecar
            .start_execution(StartExecutionRequest {
                vm_id: String::from("vm-browser"),
                context_id: context.context_id.clone(),
                argv: vec![String::from("node"), String::from("script.js")],
                env: BTreeMap::new(),
                cwd: String::from("/workspace"),
            })
            .expect_err("stdio setup should fail before worker creation");

        assert!(failed.to_string().contains("maximum pipe count reached"));
        assert_eq!(sidecar.active_worker_count("vm-browser"), 0);
        assert_eq!(
            sidecar.kernel_state("vm-browser").expect("kernel ready"),
            LifecycleState::Ready
        );
    }
}

#[test]
fn browser_sidecar_reaps_pending_kernel_process_when_bridge_execution_start_fails() {
    let mut bridge = RecordingBridge::default();
    bridge.push_execution_start_error("execution start failed");

    let mut sidecar = BrowserSidecar::new(bridge, BrowserSidecarConfig::default());
    let mut config = KernelVmConfig::new("vm-browser");
    config.permissions = Permissions::allow_all();
    config.resources = ResourceLimits {
        max_processes: Some(1),
        ..ResourceLimits::default()
    };
    sidecar.create_vm(config).expect("create vm");

    let context = sidecar
        .create_wasm_context(CreateWasmContextRequest {
            vm_id: String::from("vm-browser"),
            module_path: Some(String::from("/workspace/app.wasm")),
        })
        .expect("create WebAssembly context");

    let failed = sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id.clone(),
            argv: vec![String::from("wasm"), String::from("/workspace/app.wasm")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect_err("execution start should fail");

    assert!(failed.to_string().contains("execution start failed"));
    assert_eq!(sidecar.active_worker_count("vm-browser"), 0);
    assert_eq!(
        sidecar.kernel_state("vm-browser").expect("kernel ready"),
        LifecycleState::Ready
    );
    assert!(sidecar.bridge().terminated_workers.is_empty());

    sidecar
        .start_execution(StartExecutionRequest {
            vm_id: String::from("vm-browser"),
            context_id: context.context_id,
            argv: vec![String::from("wasm"), String::from("/workspace/app.wasm")],
            env: BTreeMap::new(),
            cwd: String::from("/workspace"),
        })
        .expect("leaked pending process would exhaust the one-process limit");
}
