//! Wire-level coverage for the coalesced `filesystem.changed` structured event:
//! guest filesystem mutations mark the per-VM tracker, and draining after the
//! flush window emits one VM-scoped event frame with the changed parent
//! directories (or an overflow collapse past the dirty-dir bound).

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use agentos_native_sidecar::wire::{
    EventPayload, GuestFilesystemCallRequest, GuestFilesystemOperation, GuestRuntimeKind,
    OwnershipScope, RequestPayload, ResponsePayload, RootFilesystemEntryEncoding,
};

use crate::support::{authenticate_wire, create_vm_wire, open_session_wire, wire_request, wire_vm};

/// Comfortably past `FS_CHANGED_FLUSH_INTERVAL` (300ms) without sleeping —
/// `take_due_fs_change_event_frames` takes `now` as a parameter.
const PAST_FLUSH_WINDOW: Duration = Duration::from_secs(1);

fn base_fs_call(operation: GuestFilesystemOperation, path: &str) -> GuestFilesystemCallRequest {
    GuestFilesystemCallRequest {
        operation,
        path: String::from(path),
        destination_path: None,
        target: None,
        content: None,
        encoding: None,
        recursive: false,
        max_depth: None,
        mode: None,
        uid: None,
        gid: None,
        atime_ms: None,
        mtime_ms: None,
        len: None,
        offset: None,
    }
}

fn dispatch_fs_call(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    scope: &(String, String, String),
    request_id: i64,
    payload: GuestFilesystemCallRequest,
) {
    let (connection_id, session_id, vm_id) = scope;
    let response = sidecar
        .dispatch_wire_blocking(wire_request(
            request_id,
            wire_vm(connection_id, session_id, vm_id),
            RequestPayload::GuestFilesystemCallRequest(payload),
        ))
        .expect("dispatch guest filesystem call");
    match response.response.payload {
        ResponsePayload::GuestFilesystemResultResponse(_) => {}
        other => panic!("expected guest_filesystem_result response, got {other:?}"),
    }
}

fn drain_fs_change_events(
    sidecar: &mut agentos_native_sidecar::NativeSidecar<support::RecordingBridge>,
    now: Instant,
) -> Vec<(OwnershipScope, Vec<String>, bool)> {
    sidecar
        .take_due_fs_change_event_frames(now)
        .expect("drain fs change event frames")
        .into_iter()
        .map(|frame| match frame.payload {
            EventPayload::StructuredEvent(event) => {
                assert_eq!(event.name, "filesystem.changed");
                let dirs: Vec<String> = serde_json::from_str(
                    event.detail.get("dirs").expect("dirs detail present"),
                )
                .expect("dirs detail is a JSON string array");
                let overflow = event
                    .detail
                    .get("overflow")
                    .expect("overflow detail present")
                    == "true";
                (frame.ownership, dirs, overflow)
            }
            other => panic!("expected structured event payload, got {other:?}"),
        })
        .collect()
}

#[test]
fn fs_change_events_suite() {
    support::acquire_sidecar_runtime_test_lock();
    let mut sidecar = support::new_sidecar("fs-change-events");
    let connection_id = authenticate_wire(&mut sidecar, "conn-1");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let cwd = support::temp_dir("fs-change-events-cwd");
    let (vm_id, _) = create_vm_wire(
        &mut sidecar,
        3,
        &connection_id,
        &session_id,
        GuestRuntimeKind::JavaScript,
        Path::new(&cwd),
    );
    let scope = (connection_id.clone(), session_id.clone(), vm_id.clone());

    // VM creation itself projects files; drain whatever window that opened so
    // the assertions below observe only this test's mutations.
    let _ = drain_fs_change_events(&mut sidecar, Instant::now() + PAST_FLUSH_WINDOW);

    // Two writes into the same directory plus a mkdir: parents dedupe, and the
    // window holds until the flush interval elapses.
    dispatch_fs_call(&mut sidecar, &scope, 10, {
        let mut call = base_fs_call(GuestFilesystemOperation::WriteFile, "/tmp/a.txt");
        call.content = Some(String::from("a"));
        call.encoding = Some(RootFilesystemEntryEncoding::Utf8);
        call
    });
    dispatch_fs_call(&mut sidecar, &scope, 11, {
        let mut call = base_fs_call(GuestFilesystemOperation::WriteFile, "/tmp/b.txt");
        call.content = Some(String::from("b"));
        call.encoding = Some(RootFilesystemEntryEncoding::Utf8);
        call
    });
    dispatch_fs_call(
        &mut sidecar,
        &scope,
        12,
        base_fs_call(GuestFilesystemOperation::Mkdir, "/tmp/newdir"),
    );

    assert!(
        drain_fs_change_events(&mut sidecar, Instant::now()).is_empty(),
        "no frame before the flush window closes"
    );

    let events = drain_fs_change_events(&mut sidecar, Instant::now() + PAST_FLUSH_WINDOW);
    assert_eq!(events.len(), 1, "one coalesced frame per VM per window");
    let (ownership, dirs, overflow) = &events[0];
    assert!(!overflow);
    assert_eq!(dirs, &vec![String::from("/tmp")], "parents dedupe");
    match ownership {
        OwnershipScope::VmOwnership(vm_scope) => assert_eq!(vm_scope.vm_id, vm_id),
        other => panic!("expected vm-scoped ownership, got {other:?}"),
    }
    assert!(
        drain_fs_change_events(&mut sidecar, Instant::now() + PAST_FLUSH_WINDOW).is_empty(),
        "drain resets the window"
    );

    // A rename marks source and destination parents plus the moved entry
    // itself (its own listing goes stale if it was a directory).
    dispatch_fs_call(&mut sidecar, &scope, 20, {
        let mut call = base_fs_call(GuestFilesystemOperation::Rename, "/tmp/a.txt");
        call.destination_path = Some(String::from("/tmp/newdir/a.txt"));
        call
    });
    let events = drain_fs_change_events(&mut sidecar, Instant::now() + PAST_FLUSH_WINDOW);
    assert_eq!(events.len(), 1);
    let (_, dirs, overflow) = &events[0];
    assert!(!overflow);
    for expected in ["/tmp", "/tmp/a.txt", "/tmp/newdir", "/tmp/newdir/a.txt"] {
        assert!(
            dirs.contains(&String::from(expected)),
            "rename should mark {expected}; got {dirs:?}"
        );
    }

    // Past the dirty-dir bound the window collapses to overflow with no dirs.
    for index in 0..70 {
        dispatch_fs_call(
            &mut sidecar,
            &scope,
            100 + index,
            {
                let mut call = base_fs_call(
                    GuestFilesystemOperation::Mkdir,
                    &format!("/tmp/overflow-{index}/leaf"),
                );
                call.recursive = true;
                call
            },
        );
    }
    let events = drain_fs_change_events(&mut sidecar, Instant::now() + PAST_FLUSH_WINDOW);
    assert_eq!(events.len(), 1);
    let (_, dirs, overflow) = &events[0];
    assert!(overflow, "past the bound the event collapses to overflow");
    assert!(dirs.is_empty());
}
