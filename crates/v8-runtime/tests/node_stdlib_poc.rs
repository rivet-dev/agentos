//! POC: run Node.js's REAL lib/path.js and lib/buffer.js inside the agentOS
//! embedded V8 runtime, with `internalBinding()` provided by JS shims
//! (tests/node_stdlib_poc/bootstrap.js) instead of node's C++ core.
//!
//! Node lib sources are read from NODE_SRC_DIR (default /home/nathan/misc/node);
//! the test is skipped when the checkout is absent. See
//! tests/node_stdlib_poc/preflight.mjs for a fast host-node iteration loop over
//! the same bootstrap + checks.

use agentos_v8_runtime::embedded_runtime::EmbeddedV8Runtime;
use agentos_v8_runtime::runtime_protocol::{RuntimeCommand, RuntimeEvent, SessionMessage};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BOOTSTRAP_JS: &str = include_str!("node_stdlib_poc/bootstrap.js");
const CHECKS_JS: &str = include_str!("node_stdlib_poc/checks.js");
const CONSTANTS_JSON: &str = include_str!("node_stdlib_poc/constants.json");

/// Stage 2.5: simdutf compiled to wasm32-wasip1 with the agentOS patched
/// sysroot (build with tests/node_stdlib_poc/simdutf/build-simdutf-poc.sh).
/// Optional: when absent the bootstrap keeps its JS codec implementations.
fn simdutf_wasm_base64() -> String {
    let path = std::env::var("AGENTOS_SIMDUTF_POC_WASM")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/node_stdlib_poc/simdutf/build/simdutf-poc.wasm")
        });
    match std::fs::read(&path) {
        Ok(bytes) => base64_encode(&bytes),
        Err(_) => {
            eprintln!(
                "note: simdutf POC wasm not found at {} — running with JS codec fallback \
                 (build it with tests/node_stdlib_poc/simdutf/build-simdutf-poc.sh)",
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

fn node_src_dir() -> Option<PathBuf> {
    let dir = std::env::var("NODE_SRC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/home/nathan/misc/node"));
    dir.join("lib").is_dir().then_some(dir)
}

fn collect_lib_sources(lib_dir: &Path, base: &Path, out: &mut BTreeMap<String, String>) {
    for entry in std::fs::read_dir(lib_dir).expect("read node lib dir") {
        let path = entry.expect("read node lib entry").path();
        if path.is_dir() {
            collect_lib_sources(&path, base, out);
        } else if path.extension().is_some_and(|ext| ext == "js") {
            let id = path
                .strip_prefix(base)
                .expect("lib path under base")
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            let source = std::fs::read_to_string(&path).expect("read node lib source");
            out.insert(id, source);
        }
    }
}

fn wait_for_execution_result(
    receiver: &mpsc::Receiver<RuntimeEvent>,
    session_id: &str,
) -> RuntimeEvent {
    // Generous: the session compiles ~5MB of injected node lib sources.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for execution result");
        let event = receiver
            .recv_timeout(remaining)
            .expect("runtime event should arrive before timeout");
        if matches!(
            &event,
            RuntimeEvent::ExecutionResult { session_id: event_session_id, .. }
                if event_session_id == session_id
        ) {
            return event;
        }
    }
}

#[test]
fn real_node_path_and_buffer_run_in_isolate() {
    let Some(node_src) = node_src_dir() else {
        eprintln!("skipping: node checkout not found (set NODE_SRC_DIR or clone to /home/nathan/misc/node)");
        return;
    };

    let lib_dir = node_src.join("lib");
    let mut sources = BTreeMap::new();
    collect_lib_sources(&lib_dir, &lib_dir, &mut sources);
    assert!(
        sources.contains_key("path") && sources.contains_key("buffer"),
        "node lib sources should include path and buffer"
    );

    let sources_json = serde_json::to_string(&sources).expect("serialize node lib sources");
    let simdutf_b64 = simdutf_wasm_base64();
    // Executed as an ES module (mode 1) so the trailing top-level await makes
    // the ExecutionResult wait for — and report failures from — the async fs
    // checks (callbacks, promises, streams). An event loop that dropped the
    // in-flight work would surface here as a pending/failed evaluation.
    let user_code = format!(
        "globalThis.__nodeSources = {sources_json};\n\
         globalThis.__nodeConstants = {CONSTANTS_JSON};\n\
         globalThis.__pocSimdutfWasmBase64 = \"{simdutf_b64}\";\n\
         {BOOTSTRAP_JS}\n\
         {CHECKS_JS}\n\
         await globalThis.__pocAsync;\n"
    );

    let runtime = Arc::new(EmbeddedV8Runtime::new(Some(1)).expect("create embedded runtime"));
    let session_id = "node-stdlib-poc";
    let receiver = runtime
        .register_session(session_id)
        .expect("register session");
    runtime
        .dispatch(RuntimeCommand::CreateSession {
            session_id: session_id.to_owned(),
            heap_limit_mb: None,
            cpu_time_limit_ms: None,
            wall_clock_limit_ms: None,
            warm_hint: None,
        })
        .expect("create session");

    runtime
        .dispatch(RuntimeCommand::SendToSession {
            session_id: session_id.to_owned(),
            message: SessionMessage::Execute {
                mode: 1,
                file_path: String::new(),
                bridge_code: "(function() {})();".to_owned(),
                post_restore_script: String::new(),
                userland_code: String::new(),
                high_resolution_time: false,
                user_code,
                wasm_module_bytes: None,
            },
        })
        .expect("dispatch execute");

    match wait_for_execution_result(&receiver, session_id) {
        RuntimeEvent::ExecutionResult {
            exit_code, error, ..
        } => {
            if let Some(error) = &error {
                panic!(
                    "node stdlib POC failed in isolate: {} {}\n{}",
                    error.error_type, error.message, error.stack
                );
            }
            assert_eq!(exit_code, 0, "expected successful execution");
        }
        other => panic!("expected execution result, got {other:?}"),
    }

    runtime
        .dispatch(RuntimeCommand::DestroySession {
            session_id: session_id.to_owned(),
        })
        .expect("destroy session");
    runtime.unregister_session(session_id);
}
