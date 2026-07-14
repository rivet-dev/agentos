use std::fs;
use std::path::Path;

#[test]
fn production_v8_runtime_never_discovers_the_process_runtime() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut rust_files = Vec::new();
    collect_rust_files(&source_root, &mut rust_files);

    let forbidden = "SidecarRuntime::process_context";
    let mut violations = Vec::new();
    for path in rust_files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let compact: String = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        if compact.contains(forbidden) {
            violations.push(format!("{} contains {forbidden}", path.display()));
        }
    }

    assert!(
        violations.is_empty(),
        "v8-runtime must receive RuntimeContext explicitly; production source may not discover the process runtime:\n{}",
        violations.join("\n")
    );
}

#[test]
fn node_server_close_waits_for_accepted_connections_to_drain() {
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/build-tools/bridge-src/builtins/net.ts"),
    )
    .expect("read JavaScript net bridge source");
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact.contains("socket.once(\"close\",()=>{this._connections.delete(socket);")
            && compact.contains("this._emitCloseIfDrained();"),
        "accepted-socket teardown must re-check the Node server close drain gate"
    );
    assert!(
		compact.contains("Promise.resolve(_netServerCloseRaw(serverId)).then(")
			&& compact.contains("this._pendingTransportCloses-=1")
			&& compact.contains("this._emitCloseIfDrained();"),
		"listener teardown must complete asynchronously before entering the Node server close drain gate"
    );
    assert!(
		compact.contains("this._pendingTransportCloses!==0")
			&& compact.contains("this._connections.size!==0"),
		"the Node server close drain gate must wait for transport completion and every accepted socket"
    );
    assert!(
        !compact.contains("_netServerCloseRaw.applySync")
            && !compact.contains("queueMicrotask(()=>this._emit(\"close\"))"),
        "listener teardown must neither block V8 nor emit close before accepted sockets drain"
    );
}

#[test]
fn direct_bridge_responses_are_registered_with_the_blocking_event_loop_selector() {
    let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session.rs"))
        .expect("read V8 session source");
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(
        compact.contains("bridge_rx.map(|responses|(selector.recv(responses),responses))"),
        "the direct bridge-response lane must wake the blocking V8 event-loop selector"
    );
    assert!(
        compact.contains("selector.select_timeout(Duration::from_millis(1))"),
        "the V8 platform-work fallback may remain bounded without polling bridge responses"
    );
}

fn collect_rust_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("source directory entry").path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}
