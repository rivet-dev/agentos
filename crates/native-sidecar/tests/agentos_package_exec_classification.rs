mod support;

use agentos_native_sidecar::wire::{
    ConfigureVmRequest, ExecuteRequest, GuestFilesystemCallRequest, GuestFilesystemOperation,
    GuestRuntimeKind, MountDescriptor, MountPluginDescriptor, PackageDescriptor, RequestPayload,
    ResponsePayload, RootFilesystemEntryEncoding,
};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use support::{
    assert_node_available, authenticate_wire, collect_process_output_wire_with_timeout,
    new_sidecar, open_session_wire, temp_dir, wasm_stdout_module, wire_request, wire_vm,
    write_fixture,
};

/// Shadow roots are `{vm_id}-{nanos}`. Parallel first-VM creates can share
/// `/etc/ssl/cert.pem` when they land in the same nanosecond. Without this,
/// the tests would fail while succeeding individually.
static VM_CREATE_LOCK: Mutex<()> = Mutex::new(());

struct PackageVmHarness {
    sidecar: agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: String,
    session_id: String,
    vm_id: String,
}

fn write_package_tar(package: &Path) {
    let tar_path = package.join("package.tar");
    let _ = fs::remove_file(&tar_path);
    let file = fs::File::create(&tar_path).expect("create package tar");
    let mut builder = tar::Builder::new(file);
    builder.follow_symlinks(false);
    append_package_tree(&mut builder, package, package).expect("append package tree");
    builder.finish().expect("finish package tar");
    builder
        .into_inner()
        .expect("finish package tar file")
        .flush()
        .expect("flush package tar");
}

fn append_package_tree(
    builder: &mut tar::Builder<fs::File>,
    root: &Path,
    path: &Path,
) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();
        if entry_path.file_name().and_then(|name| name.to_str()) == Some("package.tar") {
            continue;
        }
        let name = entry_path
            .strip_prefix(root)
            .expect("package-relative path");
        if entry_path.is_dir() {
            builder.append_dir(name, &entry_path)?;
            append_package_tree(builder, root, &entry_path)?;
        } else {
            builder.append_path_with_name(&entry_path, name)?;
        }
    }
    Ok(())
}

fn registry_command_root() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    for candidate in [
        repo_root.join("packages/runtime-core/commands"),
        repo_root.join("toolchain/target/wasm32-wasip1/release/commands"),
        repo_root.join("packages/core/commands"),
    ] {
        if candidate.join("sh").is_file() {
            return Some(candidate);
        }
    }
    None
}

fn mount_agentos_package(package: &Path, with_registry_commands: bool) -> PackageVmHarness {
    write_fixture(
        &package.join("agentos-package.json"),
        r#"{"name":"exec-classify","version":"1.0.0"}"#,
    );
    write_package_tar(package);

    let mut sidecar = new_sidecar("agentos-package-exec-classification");
    let connection_id = authenticate_wire(&mut sidecar, "exec-classify-connection");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = temp_dir("agentos-package-exec-classification-cwd");
    let (vm_id, _) = {
        let _create_guard = VM_CREATE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        support::create_vm_wire(
            &mut sidecar,
            3,
            &connection_id,
            &session_id,
            GuestRuntimeKind::JavaScript,
            &cwd,
        )
    };

    let mut mounts = Vec::new();
    if with_registry_commands {
        let command_root =
            registry_command_root().expect("registry commands required for this test");
        mounts.push(MountDescriptor {
            guest_path: String::from("/__agentos/commands/0"),
            guest_source: String::from("agentos"),
            guest_fstype: String::from("agentos"),
            read_only: true,
            plugin: MountPluginDescriptor {
                id: String::from("host_dir"),
                config: json!({
                    "hostPath": command_root,
                    "readOnly": true,
                })
                .to_string(),
            },
        });
    }

    let response = sidecar
        .dispatch_wire_blocking(wire_request(
            4,
            wire_vm(&connection_id, &session_id, &vm_id),
            RequestPayload::ConfigureVmRequest(ConfigureVmRequest {
                mounts,
                software: Vec::new(),
                permissions: None,
                module_access_cwd: None,
                instructions: Vec::new(),
                projected_modules: Vec::new(),
                command_permissions: HashMap::new(),
                loopback_exempt_ports: Vec::new(),
                packages: vec![PackageDescriptor {
                    path: package.to_string_lossy().into_owned(),
                }],
                packages_mount_at: String::from("/opt/agentos"),
                bootstrap_commands: Vec::new(),
                binding_shim_commands: Vec::new(),
            }),
        ))
        .expect("configure agentos package mount");

    match response.response.payload {
        ResponsePayload::VmConfiguredResponse(response) => {
            assert!(
                response.applied_mounts >= 1,
                "expected package mounts, got {}",
                response.applied_mounts
            );
        }
        other => panic!("unexpected configure response: {other:?}"),
    }

    PackageVmHarness {
        sidecar,
        connection_id,
        session_id,
        vm_id,
    }
}

fn execute(harness: &mut PackageVmHarness, process_id: &str, command: &str, args: &[&str]) {
    let response = harness
        .sidecar
        .dispatch_wire_blocking(wire_request(
            10,
            wire_vm(&harness.connection_id, &harness.session_id, &harness.vm_id),
            RequestPayload::ExecuteRequest(ExecuteRequest {
                process_id: process_id.to_owned(),
                command: Some(command.to_owned()),
                runtime: None,
                entrypoint: None,
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                env: HashMap::new(),
                cwd: Some(String::from("/")),
                wasm_permission_tier: None,
            }),
        ))
        .expect("dispatch execute");

    match response.response.payload {
        ResponsePayload::ProcessStartedResponse(response) => {
            assert_eq!(response.process_id, process_id);
        }
        other => panic!("unexpected execute response: {other:?}"),
    }
}

#[test]
fn extensionless_wasm_execute() {
    assert_node_available();
    let package = temp_dir("agentos-package-wasm-bin");
    let bin = package.join("bin/wasmcmd");
    write_fixture(&bin, wasm_stdout_module());
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod wasm bin");

    let mut harness = mount_agentos_package(&package, false);
    execute(
        &mut harness,
        "proc-wasm-exec",
        "/opt/agentos/bin/wasmcmd",
        &[],
    );
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-wasm-exec",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("wasm:ready"),
        "expected wasm output, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stderr.contains("ENOEXEC"),
        "extensionless wasm execute must not fail exec validation: {stderr}"
    );
}

#[test]
fn extensionless_wasm_execve() {
    assert_node_available();
    let package = temp_dir("agentos-package-wasm-bin");
    let bin = package.join("bin/wasmexec");
    write_fixture(&bin, wasm_stdout_module());
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod wasm bin");

    let mut harness = mount_agentos_package(&package, false);
    let write = harness
        .sidecar
        .dispatch_wire_blocking(wire_request(
            11,
            wire_vm(&harness.connection_id, &harness.session_id, &harness.vm_id),
            RequestPayload::GuestFilesystemCallRequest(GuestFilesystemCallRequest {
                operation: GuestFilesystemOperation::WriteFile,
                path: String::from("/execve.mjs"),
                destination_path: None,
                target: None,
                content: Some(String::from(
                    r#"
import childProcess from "node:child_process";

const result = childProcess.spawnSync("/opt/agentos/bin/wasmexec", [], {
  encoding: "utf8",
});
if (result.stdout) {
  process.stdout.write(result.stdout);
}
if (result.stderr) {
  process.stderr.write(result.stderr);
}
process.exit(result.status ?? 1);
"#,
                )),
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
        .expect("write guest javascript entry");
    match write.response.payload {
        ResponsePayload::GuestFilesystemResultResponse(_) => {}
        other => panic!("unexpected guest write response: {other:?}"),
    }

    execute(
        &mut harness,
        "proc-wasm-execve-parent",
        "node",
        &["/execve.mjs"],
    );
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-wasm-execve-parent",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("wasm:ready"),
        "expected wasm output from guest execve, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stderr.contains("ENOEXEC"),
        "extensionless wasm execve must not fail exec validation: {stderr}"
    );
}

#[test]
fn extensionless_js_bin() {
    assert_node_available();
    let package = temp_dir("agentos-package-extless-js");
    let bin = package.join("bin/foo");
    write_fixture(
        &bin,
        "// extensionless package bin\nfunction main() {\n  console.log('extless-js-ok');\n}\nmain();\n",
    );
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod js bin");

    let mut harness = mount_agentos_package(&package, false);
    execute(&mut harness, "proc-extless-js", "/opt/agentos/bin/foo", &[]);
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-extless-js",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("extless-js-ok"),
        "expected JS output, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stderr.contains("ENOEXEC"),
        "extensionless JS package bin must not fail classification: {stderr}"
    );
}

#[test]
fn js_symlink_stays_js() {
    assert_node_available();
    let package = temp_dir("agentos-package-js-symlink");
    let adapter = package.join("adapter.mjs");
    write_fixture(
        &adapter,
        "#!/usr/bin/env node\nconsole.log('js-symlink-ok');\n",
    );
    fs::set_permissions(&adapter, fs::Permissions::from_mode(0o755)).expect("chmod adapter");
    fs::create_dir_all(package.join("bin")).expect("create bin");
    std::os::unix::fs::symlink("../adapter.mjs", package.join("bin/x"))
        .expect("symlink bin launcher");

    let mut harness = mount_agentos_package(&package, false);
    execute(&mut harness, "proc-js-symlink", "/opt/agentos/bin/x", &[]);
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-js-symlink",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("js-symlink-ok"),
        "expected JS output, got stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn shell_script_not_v8() {
    assert_node_available();
    if registry_command_root().is_none() {
        eprintln!("skipping shell classification test: registry WASM commands are not built");
        return;
    }
    let package = temp_dir("agentos-package-shell");
    let bin = package.join("bin/run-sh");
    write_fixture(&bin, "#!/bin/sh\nprintf 'shell-ok\\n'\n");
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod shell bin");

    let mut harness = mount_agentos_package(&package, true);
    execute(&mut harness, "proc-shell", "/opt/agentos/bin/run-sh", &[]);
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-shell",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("shell-ok"),
        "expected shell output, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stderr.contains("Cannot use import statement"),
        "shell script must not run in V8: {stderr}"
    );
}

#[test]
fn wasm_extension_with_js_content() {
    assert_node_available();
    let package = temp_dir("agentos-package-fake-wasm-ext");
    let bin = package.join("bin/fake.wasm");
    write_fixture(
        &bin,
        "#!/usr/bin/env node\nconsole.log('fake-wasm-ext-js-ok');\n",
    );
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod fake wasm");

    let mut harness = mount_agentos_package(&package, false);
    execute(
        &mut harness,
        "proc-fake-wasm-ext",
        "/opt/agentos/bin/fake.wasm",
        &[],
    );
    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
        &mut harness.sidecar,
        &harness.connection_id,
        &harness.session_id,
        &harness.vm_id,
        "proc-fake-wasm-ext",
        Duration::from_secs(30),
    );

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(
        stdout.contains("fake-wasm-ext-js-ok"),
        "expected JS output, got stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stderr.contains("WebAssembly.Module"),
        "misnamed JS must not reach wasm compile: {stderr}"
    );
}

#[test]
fn elf_rejected() {
    assert_node_available();
    let package = temp_dir("agentos-package-elf");
    let bin = package.join("bin/native");
    write_fixture(&bin, [0x7f, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00]);
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).expect("chmod elf bin");

    let mut harness = mount_agentos_package(&package, false);
    let response = harness
        .sidecar
        .dispatch_wire_blocking(wire_request(
            12,
            wire_vm(&harness.connection_id, &harness.session_id, &harness.vm_id),
            RequestPayload::ExecuteRequest(ExecuteRequest {
                process_id: String::from("proc-elf"),
                command: Some(String::from("/opt/agentos/bin/native")),
                runtime: None,
                entrypoint: None,
                args: Vec::new(),
                env: HashMap::new(),
                cwd: Some(String::from("/")),
                wasm_permission_tier: None,
            }),
        ))
        .expect("dispatch elf execute");

    match response.response.payload {
        ResponsePayload::RejectedResponse(rejected) => {
            assert!(
                rejected.message.contains("ERR_NATIVE_BINARY_NOT_SUPPORTED"),
                "expected native rejection, got: {}",
                rejected.message
            );
        }
        other => panic!("expected rejected execute for ELF package command, got {other:?}"),
    }
}
