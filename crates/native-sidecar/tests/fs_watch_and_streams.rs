mod support;

use agentos_native_sidecar::wire::{
    CreateVmRequest, GuestRuntimeKind, RequestPayload, ResponsePayload, RootFilesystemDescriptor,
    RootFilesystemEntry, RootFilesystemEntryEncoding, RootFilesystemEntryKind, RootFilesystemMode,
};
use std::collections::HashMap;
use std::time::Duration;
use support::{
    assert_node_available, authenticate_wire, collect_process_output_wire_with_timeout,
    configure_host_workspace_mount, execute_wire, new_sidecar, open_session_wire, temp_dir,
    wire_permissions_allow_all, wire_request, wire_session, write_fixture,
};

fn root_dir(path: &str, mode: u32) -> RootFilesystemEntry {
    RootFilesystemEntry {
        path: path.to_owned(),
        kind: RootFilesystemEntryKind::Directory,
        mode: Some(mode),
        uid: None,
        gid: None,
        content: None,
        encoding: None,
        target: None,
        executable: false,
    }
}

fn root_file(path: &str, content: &str) -> RootFilesystemEntry {
    RootFilesystemEntry {
        path: path.to_owned(),
        kind: RootFilesystemEntryKind::File,
        mode: None,
        uid: None,
        gid: None,
        content: Some(content.to_owned()),
        encoding: Some(RootFilesystemEntryEncoding::Utf8),
        target: None,
        executable: false,
    }
}

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

    let connection_id = authenticate_wire(&mut sidecar, "conn-fs-watch-and-streams");
    let session_id = open_session_wire(&mut sidecar, 2, &connection_id);
    let create = sidecar
        .dispatch_wire_blocking(wire_request(
            3,
            wire_session(&connection_id, &session_id),
            RequestPayload::CreateVmRequest(CreateVmRequest::legacy_test_config(
                GuestRuntimeKind::JavaScript,
                HashMap::from([(String::from("cwd"), String::from("/workspace"))]),
                RootFilesystemDescriptor {
                    mode: RootFilesystemMode::Ephemeral,
                    disable_default_base_layer: false,
                    lowers: Vec::new(),
                    bootstrap_entries: vec![
                        root_dir("/rpc", 0o755),
                        root_file("/rpc/input.txt", "abcdefg"),
                        root_file("/rpc/output.txt", "hello"),
                        root_file("/rpc/watch.txt", "before"),
                    ],
                },
                Some(wire_permissions_allow_all()),
            )),
        ))
        .expect("create sidecar vm");
    let vm_id = match create.response.payload {
        ResponsePayload::VmCreatedResponse(response) => response.vm_id,
        other => panic!("unexpected create vm response: {other:?}"),
    };
    configure_host_workspace_mount(&mut sidecar, 3, &connection_id, &session_id, &vm_id, &cwd);

    execute_wire(
        &mut sidecar,
        4,
        &connection_id,
        &session_id,
        &vm_id,
        "fs-watch-and-streams",
        GuestRuntimeKind::JavaScript,
        "/workspace/fs-watch-and-streams.mjs",
        Vec::new(),
    );

    let (stdout, stderr, exit_code) = collect_process_output_wire_with_timeout(
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
