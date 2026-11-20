mod support;

use agentos_executor_v8_runtime::module_code_cache::module_code_cache;
use agentos_vm::wire::{
    BootstrapRootFilesystemRequest, ConfigureVmRequest, DisposeReason, DisposeVmRequest,
    GuestFilesystemCallRequest, GuestFilesystemOperation, GuestRuntimeKind, MountDescriptor,
    MountPluginDescriptor, RequestPayload, ResponsePayload, RootFilesystemEntry,
    RootFilesystemEntryEncoding, RootFilesystemEntryKind,
};
use agentos_vm::VmManager;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use support::{
    assert_node_available, authenticate_wire, collect_process_output_wire_with_timeout,
    create_vm_request_with_selected_wasm_backend, execute_wire, new_sidecar, open_session_wire,
    temp_dir, wire_permissions_allow_all, wire_request, wire_session, wire_vm, RecordingBridge,
};

/// A module large enough for the code cache that prints `marker`.
fn module_printing(marker: &str) -> String {
    format!(
        "// {}\nprocess.stdout.write({marker:?});\n",
        "x".repeat(8 * 1024)
    )
}

/// A module large enough for the code cache that a guest entry imports.
fn dependency_exporting(marker: &str) -> String {
    format!(
        "// {}\nexport const marker = {marker:?};\n",
        "y".repeat(8 * 1024)
    )
}

struct Vm {
    connection_id: String,
    session_id: String,
    vm_id: String,
}

fn create_vm_with_read_only_bundle(
    sidecar: &mut VmManager<RecordingBridge>,
    request_id: &mut i64,
    name: &str,
    bundle: &Path,
) -> Vm {
    let mut next = || {
        *request_id += 1;
        *request_id
    };
    let connection_id = authenticate_wire(sidecar, &format!("conn-{name}"));
    let session_id = open_session_wire(sidecar, next(), &connection_id);
    let cwd = temp_dir(&format!("module-code-cache-cwd-{name}"));
    let vm_id = create_snapshot_vm(
        sidecar,
        next(),
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        &cwd,
    );
    let vm = wire_vm(&connection_id, &session_id, &vm_id);
    sidecar
        .dispatch_wire_blocking(wire_request(
            next(),
            vm.clone(),
            RequestPayload::BootstrapRootFilesystemRequest(BootstrapRootFilesystemRequest {
                entries: vec![RootFilesystemEntry {
                    path: String::from("/mnt/bundle"),
                    kind: RootFilesystemEntryKind::Directory,
                    mode: None,
                    uid: None,
                    gid: None,
                    content: None,
                    encoding: None,
                    target: None,
                    executable: false,
                }],
            }),
        ))
        .expect("bootstrap bundle mountpoint");
    sidecar
        .dispatch_wire_blocking(wire_request(
            next(),
            vm,
            RequestPayload::ConfigureVmRequest(ConfigureVmRequest {
                mounts: vec![MountDescriptor {
                    guest_path: String::from("/mnt/bundle"),
                    guest_source: String::from("host_dir"),
                    guest_fstype: String::from("host_dir"),
                    read_only: true,
                    plugin: MountPluginDescriptor {
                        id: String::from("host_dir"),
                        config: serde_json::json!({
                            "hostPath": bundle.to_string_lossy(),
                            "readOnly": true,
                        })
                        .to_string(),
                    },
                }],
                software: Vec::new(),
                permissions: None,
                module_access_cwd: None,
                instructions: Vec::new(),
                projected_modules: Vec::new(),
                command_permissions: HashMap::new(),
                loopback_exempt_ports: Vec::new(),
                packages: Vec::new(),
                packages_mount_at: String::new(),
                bootstrap_commands: Vec::new(),
                host_function_shim_commands: Vec::new(),
            }),
        ))
        .expect("mount read-only bundle");
    Vm {
        connection_id,
        session_id,
        vm_id,
    }
}

fn create_snapshot_vm(
    sidecar: &mut VmManager<RecordingBridge>,
    request_id: i64,
    connection_id: &str,
    session_id: &str,
    runtime: GuestRuntimeKind,
    cwd: &Path,
) -> String {
    let mut request = create_vm_request_with_selected_wasm_backend(
        runtime,
        HashMap::from([(String::from("cwd"), cwd.to_string_lossy().into_owned())]),
        agentos_vm::wire::RootFilesystemDescriptor {
            mode: agentos_vm::wire::RootFilesystemMode::Ephemeral,
            disable_default_base_layer: false,
            lowers: Vec::new(),
            bootstrap_entries: Vec::new(),
        },
        Some(wire_permissions_allow_all()),
    );
    let mut config: serde_json::Value =
        serde_json::from_str(&request.config).expect("decode VM config");
    config["jsRuntime"]["snapshotEntrypoints"] =
        serde_json::json!(["/mnt/bundle/with-dep.mjs", "/tmp/guest-entry.mjs"]);
    request.config = config.to_string();
    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id,
            wire_session(connection_id, session_id),
            RequestPayload::CreateVmRequest(request),
        ))
        .expect("create snapshot-enabled VM");
    match result.response.payload {
        ResponsePayload::VmCreatedResponse(created) => created.vm_id,
        other => panic!("unexpected snapshot VM response: {other:?}"),
    }
}

fn write_guest_file(
    sidecar: &mut VmManager<RecordingBridge>,
    request_id: &mut i64,
    vm: &Vm,
    path: &str,
    content: String,
) {
    *request_id += 1;
    let write = sidecar
        .dispatch_wire_blocking(wire_request(
            *request_id,
            wire_vm(&vm.connection_id, &vm.session_id, &vm.vm_id),
            RequestPayload::GuestFilesystemCallRequest(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::WriteFile,
                path: path.to_owned(),
                destination_path: None,
                target: None,
                content: Some(content),
                encoding: Some(RootFilesystemEntryEncoding::Utf8),
                recursive: false,
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
        .expect("write guest file");
    assert!(matches!(
        write.response.payload,
        ResponsePayload::GuestFilesystemResultResponse(_)
    ));
}

fn run_node(
    sidecar: &mut VmManager<RecordingBridge>,
    request_id: &mut i64,
    vm: &Vm,
    process_id: &str,
    entrypoint: &str,
) -> String {
    *request_id += 1;
    execute_wire(
        sidecar,
        *request_id,
        &vm.connection_id,
        &vm.session_id,
        &vm.vm_id,
        process_id,
        GuestRuntimeKind::JavaScript,
        Path::new(entrypoint),
        Vec::new(),
    );
    let (stdout, stderr, exit) = collect_process_output_wire_with_timeout(
        sidecar,
        &vm.connection_id,
        &vm.session_id,
        &vm.vm_id,
        process_id,
        Duration::from_secs(20),
    );
    assert_eq!(exit, 0, "{entrypoint} failed; stderr: {stderr}");
    stdout
}

// Keep this in a separate test binary because the module cache and graph
// registry are process-wide.
#[test]
fn module_code_cache_excludes_guest_writable_modules() {
    exercise_module_code_cache(false);
}

#[test]
#[ignore = "live module bytecode reuse requires a kernel source-provenance response"]
fn module_code_cache_shares_read_only_mount_modules_across_vms() {
    exercise_module_code_cache(true);
}

fn exercise_module_code_cache(require_bytecode_reuse: bool) {
    use agentos_executor_v8_runtime::module_snapshot::{module_graph_candidate, source_digest};
    assert_node_available();
    let mut sidecar = new_sidecar("module-code-cache-isolation");
    let mut request_id = 0;

    let bundle = temp_dir("module-code-cache-bundle");
    fs::write(bundle.join("entry.mjs"), module_printing("bundle-entry")).unwrap();
    fs::write(
        bundle.join("with-dep.mjs"),
        format!(
            "import {{ marker }} from './dep.mjs';\n// {}\nprocess.stdout.write(marker);\n",
            "z".repeat(8 * 1024)
        ),
    )
    .unwrap();
    fs::write(bundle.join("dep.mjs"), dependency_exporting("bundle-dep")).unwrap();
    // A classic-script entry that reaches the bundle only through `import()`.
    fs::write(
        bundle.join("loader.cjs"),
        "import('./dyn-dep.mjs').then((m) => process.stdout.write(m.marker));\n",
    )
    .unwrap();
    fs::write(
        bundle.join("dyn-dep.mjs"),
        dependency_exporting("bundle-dyn"),
    )
    .unwrap();

    let first = create_vm_with_read_only_bundle(&mut sidecar, &mut request_id, "first", &bundle);
    let second = create_vm_with_read_only_bundle(&mut sidecar, &mut request_id, "second", &bundle);

    // Guest-written entry and guest-written import: nothing is shared.
    write_guest_file(
        &mut sidecar,
        &mut request_id,
        &first,
        "/tmp/guest-dep.mjs",
        dependency_exporting("guest-dep"),
    );
    let guest_entry = format!(
        "import {{ marker }} from './guest-dep.mjs';\n// {}\nprocess.stdout.write(marker);\n",
        "w".repeat(8 * 1024)
    );
    write_guest_file(
        &mut sidecar,
        &mut request_id,
        &first,
        "/tmp/guest-entry.mjs",
        guest_entry.clone(),
    );
    for round in 0..2 {
        let stdout = run_node(
            &mut sidecar,
            &mut request_id,
            &first,
            &format!("guest-{round}"),
            "/tmp/guest-entry.mjs",
        );
        assert_eq!(stdout, "guest-dep");
    }
    assert_eq!(
        module_code_cache().total_bytes(),
        0,
        "guest-writable modules must not enter the cross-VM code cache"
    );

    assert!(
        module_graph_candidate("/tmp/guest-entry.mjs", &source_digest(&guest_entry)).is_none(),
        "an explicitly configured writable module cannot be captured"
    );

    // Both VMs also load read-only bundle modules through their authorized
    // live resolver, including dynamically imported modules.
    assert_eq!(
        run_node(
            &mut sidecar,
            &mut request_id,
            &first,
            "bundle-first",
            "/mnt/bundle/with-dep.mjs"
        ),
        "bundle-dep"
    );
    let entry =
        fs::read_to_string(bundle.join("with-dep.mjs")).expect("read snapshot fixture entry");
    assert!(
        module_graph_candidate("/mnt/bundle/with-dep.mjs", &source_digest(&entry)).is_some(),
        "the kernel-authorized read-only module graph should be captured"
    );
    let after_first_vm = module_code_cache().total_bytes();
    if require_bytecode_reuse {
        assert!(
            after_first_vm > 0,
            "read-only bundle modules should enter the live code cache"
        );
    }
    assert_eq!(
        run_node(
            &mut sidecar,
            &mut request_id,
            &second,
            "bundle-second",
            "/mnt/bundle/with-dep.mjs"
        ),
        "bundle-dep"
    );
    assert_eq!(
        run_node(
            &mut sidecar,
            &mut request_id,
            &second,
            "bundle-entry",
            "/mnt/bundle/entry.mjs"
        ),
        "bundle-entry"
    );
    let after_entries = module_code_cache().total_bytes();
    if require_bytecode_reuse {
        assert!(
            after_entries > after_first_vm,
            "another read-only entry should be cached"
        );
    }
    assert_eq!(
        run_node(
            &mut sidecar,
            &mut request_id,
            &first,
            "bundle-dynamic",
            "/mnt/bundle/loader.cjs"
        ),
        "bundle-dyn"
    );

    if require_bytecode_reuse {
        assert!(
            module_code_cache().total_bytes() > after_entries,
            "a dynamic read-only import should be cached"
        );
    }

    for vm in [first, second] {
        request_id += 1;
        sidecar
            .dispatch_wire_blocking(wire_request(
                request_id,
                wire_vm(&vm.connection_id, &vm.session_id, &vm.vm_id),
                RequestPayload::DisposeVmRequest(DisposeVmRequest {
                    reason: DisposeReason::Requested,
                }),
            ))
            .expect("dispose vm");
    }
}
