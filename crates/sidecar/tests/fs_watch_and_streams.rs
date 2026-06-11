mod support;

use agent_os_sidecar::protocol::{
    CreateVmRequest, EventPayload, GuestRuntimeKind, OwnershipScope, PermissionsPolicy,
    ProcessOutputEvent, RequestPayload, ResponsePayload, RootFilesystemDescriptor,
    RootFilesystemEntry, RootFilesystemEntryEncoding, RootFilesystemEntryKind, StreamChannel,
};
use std::time::Duration;
use support::{
    assert_node_available, authenticate, execute, new_sidecar, open_session, request, temp_dir,
    write_fixture,
};

const PROCESS_OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;

#[test]
fn javascript_fs_watch_and_streams_work_against_the_vm_kernel_filesystem() {
    assert_node_available();

    let mut sidecar = new_sidecar("fs-watch-and-streams");
    let cwd = temp_dir("fs-watch-and-streams-cwd");
    let entry = cwd.join("fs-watch-and-streams.mjs");

    write_fixture(
        &entry,
        r#"
import fs from "node:fs";
import { once } from "node:events";

const readChunks = [];
const reader = fs.createReadStream("/rpc/input.txt", {
  encoding: "utf8",
  start: 1,
  end: 5,
  highWaterMark: 2,
});
reader.on("data", (chunk) => readChunks.push(chunk));
await once(reader, "close");

const writer = fs.createWriteStream("/rpc/output.txt", {
  start: 2,
  highWaterMark: 2,
});
writer.write("XY");
writer.end("Z");
await once(writer, "close");

const watchEvents = [];
const watchFileEvents = [];
const watcher = fs.watch("/rpc/watch.txt", (eventType, filename) => {
  watchEvents.push({
    eventType,
    filename: Buffer.isBuffer(filename) ? filename.toString("utf8") : filename,
  });
});
fs.watchFile("/rpc/watch.txt", { interval: 20 }, (curr, prev) => {
  watchFileEvents.push({
    currSize: curr.size,
    prevSize: prev.size,
  });
});

setTimeout(() => {
  fs.writeFileSync("/rpc/watch.txt", "after!!");
}, 60);

const deadline = Date.now() + 3000;
while (watchEvents.length === 0 || watchFileEvents.length === 0) {
  if (Date.now() > deadline) {
    watcher.close();
    fs.unwatchFile("/rpc/watch.txt");
    throw new Error(
      `timed out waiting for watch events: ${JSON.stringify({
        watchEvents,
        watchFileEvents,
      })}`,
    );
  }
  await new Promise((resolve) => setTimeout(resolve, 20));
}

watcher.close();
fs.unwatchFile("/rpc/watch.txt");

console.log(
  JSON.stringify({
    readChunks,
    output: fs.readFileSync("/rpc/output.txt", "utf8"),
    watchEvents,
    watchFileEvents,
  }),
);
"#,
    );

    let connection_id = authenticate(&mut sidecar, "conn-fs-watch-and-streams");
    let session_id = open_session(&mut sidecar, 2, &connection_id);
    let create = sidecar
        .dispatch_blocking(request(
            3,
            OwnershipScope::session(&connection_id, &session_id),
            RequestPayload::CreateVm(CreateVmRequest {
                runtime: GuestRuntimeKind::JavaScript,
                metadata: std::collections::BTreeMap::from([(
                    String::from("cwd"),
                    cwd.to_string_lossy().into_owned(),
                )]),
                root_filesystem: RootFilesystemDescriptor {
                    bootstrap_entries: vec![
                        RootFilesystemEntry {
                            path: String::from("/rpc"),
                            kind: RootFilesystemEntryKind::Directory,
                            mode: Some(0o755),
                            ..RootFilesystemEntry::default()
                        },
                        RootFilesystemEntry {
                            path: String::from("/rpc/input.txt"),
                            content: Some(String::from("abcdefg")),
                            encoding: Some(RootFilesystemEntryEncoding::Utf8),
                            ..RootFilesystemEntry::default()
                        },
                        RootFilesystemEntry {
                            path: String::from("/rpc/output.txt"),
                            content: Some(String::from("hello")),
                            encoding: Some(RootFilesystemEntryEncoding::Utf8),
                            ..RootFilesystemEntry::default()
                        },
                        RootFilesystemEntry {
                            path: String::from("/rpc/watch.txt"),
                            content: Some(String::from("before")),
                            encoding: Some(RootFilesystemEntryEncoding::Utf8),
                            ..RootFilesystemEntry::default()
                        },
                    ],
                    ..RootFilesystemDescriptor::default()
                },
                permissions: Some(PermissionsPolicy::allow_all()),
            }),
        ))
        .expect("create sidecar vm");
    let vm_id = match create.response.payload {
        ResponsePayload::VmCreated(response) => response.vm_id,
        other => panic!("unexpected create vm response: {other:?}"),
    };

    execute(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "fs-watch-and-streams",
        GuestRuntimeKind::JavaScript,
        &entry,
        Vec::new(),
    );

    let (stdout, stderr, exit_code) = collect_fs_process_output(
        &mut sidecar,
        &connection_id,
        &session_id,
        &vm_id,
        "fs-watch-and-streams",
        Duration::from_secs(10),
    );

    assert_eq!(exit_code, 0, "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(stderr.trim().is_empty(), "unexpected stderr:\n{stderr}");

    let json_line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .expect("stdout json line");
    let payload: serde_json::Value =
        serde_json::from_str(json_line).expect("parse fs watch and streams result");

    assert_eq!(payload["readChunks"], serde_json::json!(["bc", "de", "f"]));
    assert_eq!(payload["output"], "\u{0}\u{0}XYZ");
    assert_eq!(payload["watchEvents"][0]["eventType"], "change");
    assert_eq!(payload["watchEvents"][0]["filename"], "watch.txt");
    assert_eq!(payload["watchFileEvents"][0]["prevSize"], 6);
    assert_eq!(payload["watchFileEvents"][0]["currSize"], 7);
}

fn collect_fs_process_output(
    sidecar: &mut agent_os_sidecar::NativeSidecar<support::RecordingBridge>,
    connection_id: &str,
    session_id: &str,
    vm_id: &str,
    process_id: &str,
    timeout: Duration,
) -> (String, String, i32) {
    let ownership = OwnershipScope::session(connection_id, session_id);
    let deadline = std::time::Instant::now() + timeout;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit = None;

    loop {
        let event = sidecar
            .poll_event_blocking(&ownership, Duration::from_millis(100))
            .expect("poll fs watch process event");
        if let Some(event) = event {
            assert_eq!(
                event.ownership,
                OwnershipScope::vm(connection_id, session_id, vm_id)
            );

            match event.payload {
                EventPayload::ProcessOutput(ProcessOutputEvent {
                    process_id: event_process_id,
                    channel,
                    chunk,
                }) if event_process_id == process_id => match channel {
                    StreamChannel::Stdout => {
                        append_process_output(&mut stdout, &chunk, &event_process_id, "stdout")
                    }
                    StreamChannel::Stderr => {
                        append_process_output(&mut stderr, &chunk, &event_process_id, "stderr")
                    }
                },
                EventPayload::ProcessExited(exited) if exited.process_id == process_id => {
                    exit = Some((exited.exit_code, std::time::Instant::now()));
                }
                EventPayload::ProcessOutput(_)
                | EventPayload::ProcessExited(_)
                | EventPayload::VmLifecycle(_)
                | EventPayload::Structured(_) => {}
            }
        }

        if let Some((exit_code, seen_at)) = exit {
            if std::time::Instant::now().duration_since(seen_at) >= Duration::from_millis(200) {
                return (stdout, stderr, exit_code);
            }
        }

        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for fs watch process {process_id}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

fn append_process_output(buffer: &mut String, chunk: &[u8], process_id: &str, channel: &str) {
    let text = String::from_utf8_lossy(chunk);
    assert!(
        buffer.len().saturating_add(text.len()) <= PROCESS_OUTPUT_BYTE_LIMIT,
        "fs watch process {process_id} exceeded {PROCESS_OUTPUT_BYTE_LIMIT} bytes on {channel}"
    );
    buffer.push_str(&text);
}
