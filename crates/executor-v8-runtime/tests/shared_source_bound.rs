//! Snapshot sources shared across isolates are bounded: sources no snapshot or
//! isolate uses are freed for new ones, and a source that a live isolate still
//! uses is never freed.

use agentos_executor_v8_runtime::shared_source::{
    MAX_SHARED_SOURCE_BYTES, SHARED_SOURCE_LIMIT_ERROR_CODE,
};
use agentos_executor_v8_runtime::snapshot::{create_isolate_from_cached_snapshot, SnapshotCache};

const BRIDGE: &str = "(function() { globalThis.__bridge = true; })();";

/// A userland source of 40% of the bound whose function text spans it, so
/// `toString` reads the whole shared source. Two fit; a third does not.
fn userland(marker: u32) -> String {
    let padding = "x".repeat(MAX_SHARED_SOURCE_BYTES / 10 * 4);
    format!("globalThis.probe = function marker{marker}() {{ /* {padding} */ return {marker}; }};")
}

fn probe_text(isolate: &mut v8::OwnedIsolate) -> String {
    let scope = &mut v8::HandleScope::new(isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let code = v8::String::new(scope, "probe.toString()").unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    script.run(scope).unwrap().to_rust_string_lossy(scope)
}

/// Build a snapshot in its own cache, which is dropped with the result unless
/// the caller keeps the blob.
fn snapshot(marker: u32) -> Result<agentos_executor_v8_runtime::snapshot::SnapshotBlob, String> {
    SnapshotCache::new(1).get_or_create_with_userland(BRIDGE, Some(&userland(marker)))
}

// One test per process: V8 and the shared source interner are process-wide.
#[test]
fn shared_sources_are_freed_only_when_unused() {
    let first = userland(1);
    let mut isolate = create_isolate_from_cached_snapshot(&snapshot(1).expect("first"), None);

    // The second snapshot is dropped at once, so its source is unused. The
    // third needs more than the bound and fits only if that source is freed.
    snapshot(2).expect("second snapshot");
    snapshot(3).expect("an unused source is freed for a new one");
    // The first source is the oldest, but the live isolate still uses it.
    snapshot(4).expect("the next unused source is freed instead");

    let text = probe_text(&mut isolate);
    assert!(
        text.starts_with("function marker1() { /* xxx"),
        "{}",
        &text[..40]
    );
    assert!(text.ends_with("*/ return 1; }"));
    assert_eq!(text.len(), first.len() - "globalThis.probe = ".len() - 1);

    // With the isolate and a kept blob holding the bound, nothing can be freed.
    let kept = snapshot(5).expect("fifth snapshot");
    let Err(error) = snapshot(6) else {
        panic!("in-use sources are never freed, so a sixth source must not fit");
    };
    assert!(error.contains(SHARED_SOURCE_LIMIT_ERROR_CODE), "{error}");

    drop(isolate);
    snapshot(6).expect("the source is freed once its isolate exits");
    drop(kept);
}
