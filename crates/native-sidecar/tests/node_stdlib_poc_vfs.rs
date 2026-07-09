//! POC: node's REAL lib/fs.js sync ops running inside a full sidecar VM,
//! with `internalBinding('fs')` mapped onto the `_fs*` sync bridge globals —
//! i.e. against the actual kernel/ChunkedVFS, not an in-memory stand-in.
//!
//! Shares the bootstrap/checks assets with
//! crates/v8-runtime/tests/node_stdlib_poc (which runs the same checks against
//! the in-memory backend in a bare embedded-runtime session).

mod support;

use agentos_native_sidecar::wire::{
    CreateVmRequest, EventPayload, GuestRuntimeKind, RequestPayload, ResponsePayload,
    RootFilesystemDescriptor, RootFilesystemMode, StreamChannel,
};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};
use support::{
    authenticate_wire, dispose_vm_and_close_session_wire, execute_wire, new_sidecar,
    open_session_wire, temp_dir, wire_permissions_allow_all, wire_request, wire_session, wire_vm,
    write_fixture,
};

const BOOTSTRAP_JS: &str = include_str!("../../v8-runtime/tests/node_stdlib_poc/bootstrap.js");
const CHECKS_JS: &str = include_str!("../../v8-runtime/tests/node_stdlib_poc/checks.js");
const CONSTANTS_JSON: &str = include_str!("../../v8-runtime/tests/node_stdlib_poc/constants.json");

fn node_lib_sources() -> Option<String> {
    let dir = std::env::var("NODE_SRC_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/home/nathan/misc/node"));
    let lib = dir.join("lib");
    if !lib.is_dir() {
        return None;
    }
    let mut sources = BTreeMap::new();
    collect(&lib, &lib, &mut sources);
    Some(serde_json::to_string(&sources).expect("serialize node lib sources"))
}

fn collect(dir: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(dir).expect("read node lib dir") {
        let path = entry.expect("read node lib entry").path();
        if path.is_dir() {
            collect(&path, base, out);
        } else if path.extension().is_some_and(|ext| ext == "js") {
            let id = path
                .strip_prefix(base)
                .expect("lib path under base")
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            out.insert(
                id,
                std::fs::read_to_string(&path).expect("read node lib source"),
            );
        }
    }
}

/// Stage 2.5 (optional): simdutf wasm compiled with the agentOS patched
/// sysroot; built by crates/v8-runtime/tests/node_stdlib_poc/simdutf/
/// build-simdutf-poc.sh. Absent → bootstrap keeps its JS codecs.
fn simdutf_wasm_base64() -> String {
    let path = std::env::var("AGENTOS_SIMDUTF_POC_WASM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../v8-runtime/tests/node_stdlib_poc/simdutf/build/simdutf-poc.wasm")
        });
    match std::fs::read(&path) {
        Ok(bytes) => base64_encode(&bytes),
        Err(_) => {
            eprintln!(
                "note: simdutf POC wasm not found at {} — running with JS codec fallback",
                path.display()
            );
            String::new()
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn collect_process_output(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
) -> (String, String, i32) {
    let ownership = wire_session(connection_id, session_id);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit = None;

    loop {
        let event = sidecar
            .poll_event_wire_blocking(&ownership, Duration::from_millis(100))
            .expect("poll wire event");
        if let Some(event) = event {
            assert_eq!(event.ownership, wire_vm(connection_id, session_id, vm_id));
            match event.payload {
                EventPayload::ProcessOutputEvent(output) if output.process_id == process_id => {
                    let chunk = String::from_utf8_lossy(&output.chunk);
                    match output.channel {
                        StreamChannel::Stdout => stdout.push_str(&chunk),
                        StreamChannel::Stderr => stderr.push_str(&chunk),
                    }
                }
                EventPayload::ProcessExitedEvent(exited) if exited.process_id == process_id => {
                    exit = Some((exited.exit_code, Instant::now()));
                }
                _ => {}
            }
        }

        if let Some((exit_code, seen_at)) = exit {
            // Drain trailing output events before returning.
            if Instant::now().duration_since(seen_at) >= Duration::from_millis(200) {
                return (stdout, stderr, exit_code);
            }
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for POC guest process\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

#[test]
fn real_node_fs_sync_ops_hit_the_kernel_vfs() {
    let Some(sources_json) = node_lib_sources() else {
        eprintln!("skipping: node checkout not found (set NODE_SRC_DIR or clone to /home/nathan/misc/node)");
        return;
    };

    let cwd = temp_dir("node-stdlib-poc-vfs");
    let entrypoint = cwd.join("poc-entrypoint.js");
    let simdutf_b64 = simdutf_wasm_base64();
    write_fixture(
        &entrypoint,
        &format!(
            "globalThis.__nodeSources = {sources_json};\n\
             globalThis.__nodeConstants = {CONSTANTS_JSON};\n\
             globalThis.__pocUseBridgeFs = true;\n\
             globalThis.__pocSimdutfWasmBase64 = \"{simdutf_b64}\";\n\
             {BOOTSTRAP_JS}\n\
             {CHECKS_JS}\n"
        ),
    );

    let mut sidecar = new_sidecar("node-stdlib-poc-vfs");
    let connection_id = authenticate_wire(&mut sidecar, "conn-node-stdlib-poc-vfs");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);

    let mut metadata: HashMap<String, String> = HashMap::new();
    metadata.insert(String::from("cwd"), cwd.to_string_lossy().into_owned());

    let result = sidecar
        .dispatch_wire_blocking(wire_request(
            3,
            wire_session(&connection_id, &session_id),
            RequestPayload::CreateVmRequest(CreateVmRequest::legacy_test_config(
                GuestRuntimeKind::JavaScript,
                metadata,
                RootFilesystemDescriptor {
                    mode: RootFilesystemMode::Ephemeral,
                    disable_default_base_layer: false,
                    lowers: Vec::new(),
                    bootstrap_entries: Vec::new(),
                },
                Some(wire_permissions_allow_all()),
            )),
        ))
        .expect("create sidecar VM through wire");
    let vm_id = match result.response.payload {
        ResponsePayload::VmCreatedResponse(response) => response.vm_id,
        other => panic!("unexpected wire vm create response: {other:?}"),
    };

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-node-stdlib-poc-vfs",
        GuestRuntimeKind::JavaScript,
        &entrypoint,
        Vec::new(),
    );

    let (stdout, stderr, exit_code) = collect_process_output(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        "proc-node-stdlib-poc-vfs",
    );
    dispose_vm_and_close_session_wire(&mut sidecar, &connection_id, &session_id, &vm_id);

    assert_eq!(
        exit_code, 0,
        "node stdlib POC failed against the kernel VFS\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("all sync+async checks passed"),
        "expected POC success marker in stdout\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
