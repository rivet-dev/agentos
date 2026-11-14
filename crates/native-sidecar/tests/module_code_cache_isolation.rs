mod support;

use agentos_native_sidecar::wire::{
    BootstrapRootFilesystemRequest, ConfigureVmRequest, DisposeReason, DisposeVmRequest,
    GuestFilesystemCallRequest, GuestFilesystemOperation, GuestRuntimeKind, MountDescriptor,
    MountPluginDescriptor, RequestPayload, ResponsePayload, RootFilesystemEntry,
    RootFilesystemEntryEncoding, RootFilesystemEntryKind,
};
use agentos_native_sidecar::NativeSidecar;
use agentos_v8_runtime::module_code_cache::module_code_cache;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use support::{
    assert_node_available, authenticate_wire, collect_process_output_wire_with_timeout,
    create_vm_wire, execute_wire, new_sidecar, open_session_wire, temp_dir, wire_request, wire_vm,
    RecordingBridge,
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
    sidecar: &mut NativeSidecar<RecordingBridge>,
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
    let (vm_id, _) = create_vm_wire(
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

fn write_guest_file(
    sidecar: &mut NativeSidecar<RecordingBridge>,
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
    sidecar: &mut NativeSidecar<RecordingBridge>,
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

/// The sidecar-wide module code cache is the only state that crosses VMs when
/// guests load modules. A module a guest can write must never enter it (a VM
/// could otherwise time its own compile to learn what another VM loaded),
/// while a module from a read-only host mount must, so every VM that mounts
/// the bundle skips the compile.
///
/// One top-level test in its own binary: the cache is process-wide, and V8
/// integration binaries share one isolate runtime per process.
#[test]
fn module_code_cache_shares_only_read_only_mount_modules_across_vms() {
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
    write_guest_file(
        &mut sidecar,
        &mut request_id,
        &first,
        "/tmp/guest-entry.mjs",
        format!(
            "import {{ marker }} from './guest-dep.mjs';\n// {}\nprocess.stdout.write(marker);\n",
            "w".repeat(8 * 1024)
        ),
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

    // Read-only bundle modules: the first VM's load makes them available to
    // every VM, and the second VM runs them correctly from the cache.
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
    let after_first_vm = module_code_cache().total_bytes();
    assert!(
        after_first_vm > 0,
        "read-only bundle modules must be shared through the code cache"
    );
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
    assert!(
        after_entries > after_first_vm,
        "a second bundle entry is cached too"
    );
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
    assert!(
        module_code_cache().total_bytes() > after_entries,
        "a bundle module imported dynamically from a classic script is cached too"
    );

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
