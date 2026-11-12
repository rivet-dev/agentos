//! Filesystem e2e against a real `agentos-native-sidecar`. Filesystem ops go straight through the kernel
//! VFS (no V8/WASM), so this is a clean, client-focused surface.
//!
//! One VM, many assertions (quality over quantity): text + binary round-trips, batch (never-rejects),
//! recursive mkdir, readdir(_recursive), stat, exists, move (file + dir), delete (recursive), and the
//! path-guard error contract.

mod common;

use agentos_client::fs::{BatchWriteEntry, DirEntryType, FileContent, MkdirOptions, RemoveOptions};
use agentos_client::ClientError;

#[tokio::test]
async fn base_layer_exposes_default_files() {
    if !common::sidecar_available() {
        eprintln!("skipping base_layer_exposes_default_files: sidecar binary not built");
        return;
    }
    let os = common::new_vm_with_sidecar_pool("fs-base-layer").await;

    // A known base-layer file reads (sanity that the bundled base is applied).
    let hostname = os
        .read_file("/etc/hostname")
        .await
        .expect("read /etc/hostname");
    assert!(!hostname.is_empty());

    os.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn filesystem_surface_round_trips() {
    if !common::require_sidecar("filesystem_surface_round_trips") {
        return;
    }
    let os = common::new_vm_with_sidecar_pool("fs-round-trips").await;

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

    let snapshot = os
        .export_root_filesystem(64 * 1024 * 1024)
        .await
        .expect("snapshot root filesystem");
    assert_eq!(snapshot.source.format, "agentos-filesystem-snapshot-v1");
    assert!(
        snapshot
            .source
            .filesystem
            .entries
            .iter()
            .any(|entry| entry.path == "/tmp/blob.bin" && entry.entry_type == DirEntryType::File),
        "snapshot must include the binary file written through the filesystem surface"
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
    os.remove("/tmp/a2.txt", RemoveOptions { recursive: false })
        .await
        .expect("delete file");
    assert!(!os.exists("/tmp/a2.txt").await.expect("deleted"));

    // Recursive delete of a populated directory.
    os.remove("/tmp/d1", RemoveOptions { recursive: true })
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

/// Parity with TS `filesystem-write-files.test.ts`: a failed entry reports its own error, every other
/// entry is written, results keep input order, and a batch larger than one frame still writes all.
#[tokio::test]
async fn write_files_isolates_failed_entries_and_spans_frames() {
    if !common::require_sidecar("write_files_isolates_failed_entries_and_spans_frames") {
        return;
    }
    let os = common::new_vm_with_sidecar_pool("fs-write-files").await;
    let entry = |path: &str, content: FileContent| BatchWriteEntry {
        path: path.to_string(),
        content,
    };
    let bytes = vec![0u8, 1, 2, 255];

    let results = os
        .write_files(vec![
            entry("/tmp/wf/file.txt", FileContent::Text("first".into())),
            // The parent is the regular file written by the entry above.
            entry(
                "/tmp/wf/file.txt/child.txt",
                FileContent::Text("nope".into()),
            ),
            entry(
                "/tmp/wf/deep/nested/data.bin",
                FileContent::Bytes(bytes.clone()),
            ),
            entry("/proc/not-writable", FileContent::Text("nope".into())),
            entry("/tmp/wf/last.txt", FileContent::Text("last".into())),
        ])
        .await;
    let outcome: Vec<(&str, bool)> = results
        .iter()
        .map(|result| (result.path.as_str(), result.success))
        .collect();
    assert_eq!(
        outcome,
        vec![
            ("/tmp/wf/file.txt", true),
            ("/tmp/wf/file.txt/child.txt", false),
            ("/tmp/wf/deep/nested/data.bin", true),
            ("/proc/not-writable", false),
            ("/tmp/wf/last.txt", true),
        ]
    );
    let child_error = results[1].error.as_deref().expect("child error");
    assert!(
        child_error.contains("ENOTDIR") || child_error.contains("EEXIST"),
        "unexpected error: {child_error}"
    );
    assert_eq!(
        os.read_file("/tmp/wf/file.txt").await.expect("read"),
        b"first"
    );
    assert_eq!(
        os.read_file("/tmp/wf/deep/nested/data.bin")
            .await
            .expect("read"),
        bytes
    );
    assert_eq!(
        os.read_file("/tmp/wf/last.txt").await.expect("read"),
        b"last"
    );

    // Five 4 MiB files: together they exceed one 16 MiB frame.
    let big: Vec<Vec<u8>> = (0..5u32)
        .map(|seed| {
            (0..4 * 1024 * 1024u32)
                .map(|index| ((index + seed) % 251) as u8)
                .collect()
        })
        .collect();
    let results = os
        .write_files(
            big.iter()
                .enumerate()
                .map(|(seed, content)| {
                    entry(
                        &format!("/tmp/big/{seed}.bin"),
                        FileContent::Bytes(content.clone()),
                    )
                })
                .collect(),
        )
        .await;
    assert!(
        results.iter().all(|result| result.success),
        "every big file should be written: {results:?}"
    );
    for (seed, content) in big.iter().enumerate() {
        let read = os
            .read_file(&format!("/tmp/big/{seed}.bin"))
            .await
            .expect("read big file");
        assert!(read == *content, "big file {seed} must round-trip");
    }

    os.shutdown().await.expect("shutdown");
}
