//! Filesystem e2e against a real `agent-os-sidecar`. Filesystem ops go straight through the kernel
//! VFS (no V8/WASM), so this is a clean, client-focused surface.
//!
//! One VM, many assertions (quality over quantity): text + binary round-trips, batch (never-rejects),
//! recursive mkdir, readdir(_recursive), stat, exists, move (file + dir), delete (recursive), and the
//! path-guard error contract.

mod common;

use agent_os_client::fs::{BatchWriteEntry, DeleteOptions, FileContent, MkdirOptions};
use agent_os_client::ClientError;

#[tokio::test]
async fn base_layer_exposes_agentos_instructions() {
    if !common::sidecar_available() {
        eprintln!("skipping base_layer_exposes_agentos_instructions: sidecar binary not built");
        return;
    }
    let os = common::new_vm().await;

    // A known base-layer file reads (sanity that the bundled base is applied).
    let hostname = os.read_file("/etc/hostname").await.expect("read /etc/hostname");
    assert!(!hostname.is_empty());

    // The baked OS instructions must be present so createSession can read them.
    let instructions = os
        .read_file("/etc/agentos/instructions.md")
        .await
        .expect("read /etc/agentos/instructions.md");
    assert!(
        String::from_utf8_lossy(&instructions).contains("agentOS"),
        "instructions content should be the baked system prompt"
    );
}

#[tokio::test]
async fn filesystem_surface_round_trips() {
    if !common::require_sidecar("filesystem_surface_round_trips") {
        return;
    }
    let os = common::new_vm().await;

    // Text write/read.
    os.write_file("/tmp/a.txt", FileContent::Text("hello".to_string()))
        .await
        .expect("write text");
    assert_eq!(
        os.read_file("/tmp/a.txt").await.expect("read text"),
        b"hello"
    );

    // Binary write/read with non-UTF-8 bytes. This proves the `chunk: str` -> BARE `data` fix end to
    // end: a lossy UTF-8 path would corrupt these bytes.
    let blob: Vec<u8> = vec![0, 159, 146, 150, 255, 254, 0, 1, 2];
    os.write_file("/tmp/blob.bin", FileContent::Bytes(blob.clone()))
        .await
        .expect("write binary");
    assert_eq!(
        os.read_file("/tmp/blob.bin").await.expect("read binary"),
        blob,
        "binary content must round-trip byte-for-byte"
    );

    // Recursive mkdir + exists + stat.
    os.mkdir("/tmp/d1/d2", MkdirOptions { recursive: true })
        .await
        .expect("mkdir -p");
    assert!(os.exists("/tmp/d1/d2").await.expect("exists dir"));
    assert!(os.stat("/tmp/d1/d2").await.expect("stat dir").is_directory);

    // Batch write: auto-creates parents, never rejects, reports per-entry success/error.
    let results = os
        .write_files(vec![
            BatchWriteEntry {
                path: "/tmp/d1/d2/x.txt".to_string(),
                content: FileContent::Text("x".to_string()),
            },
            BatchWriteEntry {
                // Relative path fails the guard but must not reject the whole batch.
                path: "relative-bad".to_string(),
                content: FileContent::Text("y".to_string()),
            },
        ])
        .await;
    assert!(results[0].success, "valid entry should succeed");
    assert!(
        !results[1].success && results[1].error.is_some(),
        "guarded entry should fail per-entry, not reject the batch"
    );

    // readdir sees the batch-written file.
    let entries = os.readdir("/tmp/d1/d2").await.expect("readdir");
    assert!(entries.iter().any(|e| e == "x.txt"));

    // readdir_recursive finds the nested file.
    let recursive = os
        .readdir_recursive("/tmp/d1", Default::default())
        .await
        .expect("readdir_recursive");
    assert!(recursive.iter().any(|e| e.path == "/tmp/d1/d2/x.txt"));

    // Move (file) then delete (file).
    os.move_path("/tmp/a.txt", "/tmp/a2.txt")
        .await
        .expect("move file");
    assert!(!os.exists("/tmp/a.txt").await.expect("old gone"));
    assert!(os.exists("/tmp/a2.txt").await.expect("new present"));
    os.delete("/tmp/a2.txt", DeleteOptions { recursive: false })
        .await
        .expect("delete file");
    assert!(!os.exists("/tmp/a2.txt").await.expect("deleted"));

    // Recursive delete of a populated directory.
    os.delete("/tmp/d1", DeleteOptions { recursive: true })
        .await
        .expect("delete -r");
    assert!(!os.exists("/tmp/d1").await.expect("dir removed"));

    // Path-guard contract: a relative path errors with PathNotAbsolute (matches TS "Path must be
    // absolute" data), and the call errors rather than touching the VFS.
    let guard_err = os
        .read_file("relative")
        .await
        .expect_err("relative path must error");
    assert!(
        matches!(
            guard_err.downcast_ref::<ClientError>(),
            Some(ClientError::PathNotAbsolute(_))
        ),
        "expected PathNotAbsolute, got {guard_err:?}"
    );

    os.shutdown().await.expect("shutdown");
}
