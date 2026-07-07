//! Action dispatcher — the plugin-side port of `rivetkit-agent-os::actions`.
//!
//! Each arm decodes its positional args via `abi::codec::decode_positional`
//! (TS sends args as a CBOR array) and replies via [`reply_ok`] / [`reply_err`]
//! over the host vtable. `reply_ok` runs the value through
//! `encode_json_compat_to_vec` — byte-exact with rivetkit's `ActionCall::ok`,
//! so the `["$Uint8Array", base64]` byte-wrapping round-trips identically.
//!
//! The pure-`AgentOs` helper modules (filesystem/process/network/cron) are
//! verbatim copies of the rivetkit-agent-os helpers; `session`/`preview` swap
//! rivetkit's `Ctx` for [`HostCtx`] (durable storage via `db_*`).

mod contract_surface;
pub(crate) mod cron;
pub(crate) mod filesystem;
pub(crate) mod network;
pub(crate) mod preview;
pub(crate) mod process;
pub(crate) mod session;
pub(crate) mod shell;

use std::collections::HashMap;

use agentos_client::AgentOs;
use anyhow::{Context as _, Result};
use rivet_actor_plugin_abi as abi;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::task::JoinHandle;

use crate::host_ctx::HostCtx;
use filesystem::{WriteFileContent, WriteFilesEntryArg};

/// Ephemeral per-VM-lifetime actor state (session-resume, spec §3/§5/§8),
/// ported from `rivetkit-agent-os::actor::Vars`. Reconstructed on each wake from
/// the durable SQLite tables + the freshly created VM; intentionally NOT
/// persisted.
#[derive(Default)]
pub struct Vars {
    /// `external_session_id -> live_session_id`.
    pub live_sessions: HashMap<String, String>,
    /// `live_session_id -> capture pump task`.
    pub capture_tasks: HashMap<String, JoinHandle<()>>,
    /// `live_session_id -> permission-request pump task`.
    pub permission_tasks: HashMap<String, JoinHandle<()>>,
    /// Shell data/stderr/exit broadcast pump tasks (one triple per `openShell`).
    /// The pumps end on their own when the shell exits (stream close); this
    /// list exists so VM teardown aborts any still-live pumps. Bounded by the
    /// client's shell registries, not here.
    pub shell_tasks: Vec<JoinHandle<()>>,
    /// One cron event pump per VM lifetime. It fans `AgentOs::cron_events()` to
    /// actor clients as `cronEvent` broadcasts.
    pub cron_task: Option<JoinHandle<()>>,
}

impl Vars {
    /// Resolve a client-facing `external_session_id` to the live ACP session id,
    /// falling back to the external id itself (native / not-yet-resumed case).
    pub fn live_id<'a>(&'a self, external_session_id: &'a str) -> &'a str {
        self.live_sessions
            .get(external_session_id)
            .map(String::as_str)
            .unwrap_or(external_session_id)
    }

    /// Abort and clear all in-flight pump tasks (event capture + permission
    /// requests). Called on VM teardown (sleep / destroy / run-loop exit).
    pub fn clear(&mut self) {
        for (_, task) in self.capture_tasks.drain() {
            task.abort();
        }
        for (_, task) in self.permission_tasks.drain() {
            task.abort();
        }
        for task in self.shell_tasks.drain(..) {
            task.abort();
        }
        if let Some(task) = self.cron_task.take() {
            task.abort();
        }
        self.live_sessions.clear();
    }
}

/// Decode positional CBOR args into `T`.
///
/// The rivetkit client wire wraps values CBOR can't carry in JSON-compat
/// envelopes (`["$Undefined", 0]`, `["$Uint8Array", base64]`, ... — see
/// rivetkit `common/encoding.ts`). JS `undefined` is the one that reaches
/// arbitrary action args (`handle.exec(cmd, undefined)`, options objects
/// with explicitly-undefined fields), so revive it to null before serde;
/// other envelopes keep their existing per-DTO handling.
///
/// Failures carry a bounded hex prefix of the raw payload: action decode
/// errors surface to clients as opaque 500s, so the server-side log must be
/// enough to diagnose an encoding mismatch without a wire capture.
fn decode_as<T: DeserializeOwned>(args: &[u8]) -> Result<T> {
    decode_args_impl(args).map_err(|error| {
        let prefix_len = args.len().min(96);
        let mut hex = String::with_capacity(prefix_len * 2);
        for byte in &args[..prefix_len] {
            use std::fmt::Write;
            let _ = write!(hex, "{byte:02x}");
        }
        let suffix = if args.len() > prefix_len { "…" } else { "" };
        error.context(format!(
            "action args cbor ({} bytes): {hex}{suffix}",
            args.len()
        ))
    })
}

const JSON_COMPAT_UNDEFINED: &str = "$Undefined";

fn decode_args_impl<T: DeserializeOwned>(args: &[u8]) -> Result<T> {
    // Cheap pre-scan: the envelope always contains the literal sentinel text.
    let needle = JSON_COMPAT_UNDEFINED.as_bytes();
    let has_sentinel = args.windows(needle.len()).any(|w| w == needle);
    if !has_sentinel {
        return abi::codec::decode_positional(args);
    }
    let value: ciborium::Value = ciborium::from_reader(std::io::Cursor::new(args))
        .context("decode action args from cbor")?;
    let value = revive_undefined_envelopes(value);
    let mut normalized = Vec::new();
    ciborium::into_writer(&value, &mut normalized).context("re-encode revived action args")?;
    abi::codec::decode_positional(&normalized)
}

/// Revive rivetkit `["$Undefined", 0]` envelopes recursively, matching JS
/// semantics: positional/array occurrences become null, and object fields
/// that are explicitly `undefined` are treated as absent (dropped) so
/// `#[serde(default)]` fields on options DTOs apply their defaults.
fn revive_undefined_envelopes(value: ciborium::Value) -> ciborium::Value {
    use ciborium::Value;
    match value {
        Value::Array(items) => {
            if is_undefined_envelope_items(&items) {
                return Value::Null;
            }
            Value::Array(items.into_iter().map(revive_undefined_envelopes).collect())
        }
        Value::Map(entries) => Value::Map(
            entries
                .into_iter()
                .filter(|(_, v)| !is_undefined_envelope(v))
                .map(|(k, v)| (k, revive_undefined_envelopes(v)))
                .collect(),
        ),
        Value::Tag(tag, inner) => Value::Tag(tag, Box::new(revive_undefined_envelopes(*inner))),
        other => other,
    }
}

fn is_undefined_envelope(value: &ciborium::Value) -> bool {
    matches!(value, ciborium::Value::Array(items) if is_undefined_envelope_items(items))
}

fn is_undefined_envelope_items(items: &[ciborium::Value]) -> bool {
    items.len() == 2
        && matches!(&items[0], ciborium::Value::Text(tag) if tag == JSON_COMPAT_UNDEFINED)
}

/// Reply success: encode `value` with the JSON-compat byte wrapping (byte-exact
/// with rivetkit's `ActionCall::ok`) and send it over the host vtable.
fn reply_ok<T: Serialize>(host: &HostCtx, token: u64, value: &T) {
    match abi::codec::encode_json_compat_to_vec(value) {
        Ok(bytes) => {
            host.reply_ok(token, bytes);
        }
        Err(error) => {
            host.reply_err(token, &format!("encode action response: {error}"));
        }
    }
}

fn reply_ok_encoded(host: &HostCtx, token: u64, encoded: Result<Vec<u8>>) {
    match encoded {
        Ok(bytes) => {
            host.reply_ok(token, bytes);
        }
        Err(error) => {
            host.reply_err(token, &format!("encode action response: {error}"));
        }
    }
}

pub(crate) fn encode_event_arg<T: Serialize>(payload: &T) -> Result<Vec<u8>> {
    abi::codec::encode_json_compat_to_vec(&(payload,))
}

/// Reply failure with the error message (matches `ActionCall::err`).
fn reply_err(host: &HostCtx, token: u64, error: anyhow::Error) {
    // `{:#}` prints the full anyhow context chain — a bare top-level context
    // like "decode positional action args" is undiagnosable from client logs.
    let message = format!("{error:#}");
    host.log_warn(&format!("agent-os action failed: {message}"));
    host.reply_err(token, &message);
}

pub mod contract {
    use std::collections::BTreeMap;

    use agentos_client::{
        AgentExitEvent, CronEvent, CronOverlap, DirEntry, DirEntryType, JsonRpcResponse,
        ProcessInfo, ProcessStatus, ProcessTreeNode, SpawnHandle, SpawnedProcessInfo, VirtualStat,
    };
    use anyhow::{anyhow, Result};
    use ciborium::Value as CborValue;
    use rivet_actor_plugin_abi as abi;
    use serde_json::json;

    use super::{cron, filesystem, network, preview, process, session, shell};

    pub use super::contract_surface::{
        render_actor_actions_ts, ActionContract, EventContract, ReplyShape, ACTION_CONTRACTS,
        EVENT_CONTRACTS, GENERATED_ACTOR_ACTIONS_PATH,
    };

    pub fn decode_action_args(name: &str, args: &[u8]) -> Result<()> {
        match name {
            "readFile" => super::decode_as::<(String,)>(args).map(|_| ()),
            "writeFile" => {
                super::decode_as::<(String, filesystem::WriteFileContent)>(args).map(|_| ())
            }
            "stat" => super::decode_as::<(String,)>(args).map(|_| ()),
            "mkdir" => super::decode_as::<(String,)>(args).map(|_| ()),
            "readdir" => super::decode_as::<(String,)>(args).map(|_| ()),
            "readdirEntries" => super::decode_as::<(String,)>(args).map(|_| ()),
            "exists" => super::decode_as::<(String,)>(args).map(|_| ()),
            "move" => super::decode_as::<(String, String)>(args).map(|_| ()),
            "deleteFile" => {
                super::decode_as::<(String, Option<filesystem::DeleteOptionsArg>)>(args)
                    .map(|_| ())
                    .or_else(|_| super::decode_as::<(String,)>(args).map(|_| ()))
            }
            "writeFiles" => {
                super::decode_as::<(Vec<filesystem::WriteFilesEntryArg>,)>(args).map(|_| ())
            }
            "readFiles" => super::decode_as::<(Vec<String>,)>(args).map(|_| ()),
            "readdirRecursive" => super::decode_as::<(String,)>(args).map(|_| ()),
            "exec" => super::decode_as::<(String, Option<process::ExecActionOptions>)>(args)
                .map(|_| ())
                .or_else(|_| super::decode_as::<(String,)>(args).map(|_| ())),
            "spawn" => {
                super::decode_as::<(String, Vec<String>, Option<process::SpawnActionOptions>)>(args)
                    .map(|_| ())
                    .or_else(|_| super::decode_as::<(String, Vec<String>)>(args).map(|_| ()))
            }
            "waitProcess" | "killProcess" | "stopProcess" | "getProcess" | "closeProcessStdin" => {
                super::decode_as::<(u32,)>(args).map(|_| ())
            }
            "listProcesses"
            | "allProcesses"
            | "processTree"
            | "listCronJobs"
            | "listPersistedSessions"
            | "listMounts"
            | "listSoftware" => super::decode_as::<()>(args).map(|_| ()),
            "writeProcessStdin" => {
                super::decode_as::<(u32, filesystem::WriteFileContent)>(args).map(|_| ())
            }
            "openShell" => super::decode_as::<(Option<shell::OpenShellActionOptions>,)>(args)
                .map(|_| ())
                .or_else(|_| super::decode_as::<()>(args).map(|_| ())),
            "writeShell" => {
                super::decode_as::<(String, filesystem::WriteFileContent)>(args).map(|_| ())
            }
            "resizeShell" => super::decode_as::<(String, u16, u16)>(args).map(|_| ()),
            "closeShell"
            | "waitShell"
            | "cancelCronJob"
            | "closeSession"
            | "getSessionEvents"
            | "expireSignedPreviewUrl" => super::decode_as::<(String,)>(args).map(|_| ()),
            "vmFetch" => super::decode_as::<(u16, String, Option<network::FetchOptions>)>(args)
                .map(|_| ())
                .or_else(|_| super::decode_as::<(u16, String)>(args).map(|_| ())),
            "scheduleCron" => super::decode_as::<(cron::CronJobOptionsDto,)>(args).map(|_| ()),
            "createSession" => {
                super::decode_as::<(String, Option<session::CreateSessionOptionsDto>)>(args)
                    .map(|_| ())
                    .or_else(|_| super::decode_as::<(String,)>(args).map(|_| ()))
            }
            "sendPrompt" => super::decode_as::<(String, String)>(args).map(|_| ()),
            "respondPermission" => super::decode_as::<(String, String, String)>(args).map(|_| ()),
            "createSignedPreviewUrl" => super::decode_as::<(u16, u64)>(args).map(|_| ()),
            other => Err(anyhow!("unknown action {other}")),
        }
    }

    pub fn encoded_client_arg_variants(name: &str) -> Result<Vec<Vec<u8>>> {
        let variants = match name {
            "readFile"
            | "stat"
            | "mkdir"
            | "readdir"
            | "readdirEntries"
            | "exists"
            | "readdirRecursive"
            | "closeShell"
            | "waitShell"
            | "cancelCronJob"
            | "closeSession"
            | "getSessionEvents"
            | "expireSignedPreviewUrl" => {
                vec![json!(["/workspace/file.txt"])]
            }
            "writeFile" | "writeShell" => vec![
                json!(["/workspace/file.txt", "hello"]),
                json!(["/workspace/file.txt", ["$Uint8Array", "aGVsbG8="]]),
            ],
            "move" => vec![json!(["/workspace/a.txt", "/workspace/b.txt"])],
            "deleteFile" => vec![
                json!(["/workspace/file.txt"]),
                json!(["/workspace/file.txt", { "recursive": true }]),
            ],
            "writeFiles" => vec![json!([[{ "path": "/workspace/a.txt", "content": "a" }]])],
            "readFiles" => vec![json!([["/workspace/a.txt", "/workspace/b.txt"]])],
            "exec" => vec![
                json!(["echo hello"]),
                json!(["echo hello", { "cwd": "/workspace", "env": { "A": "B" } }]),
            ],
            "spawn" => vec![
                json!(["node", ["server.js"]]),
                json!(["node", ["server.js"], { "cwd": "/workspace", "streamStdin": true }]),
            ],
            "waitProcess" | "killProcess" | "stopProcess" | "getProcess" | "closeProcessStdin" => {
                vec![json!([42])]
            }
            "listProcesses"
            | "allProcesses"
            | "processTree"
            | "listCronJobs"
            | "listPersistedSessions"
            | "listMounts"
            | "listSoftware" => vec![json!([])],
            "writeProcessStdin" => vec![json!([42, ["$Uint8Array", "aGVsbG8="]])],
            "openShell" => vec![
                json!([]),
                json!([{ "command": "sh", "args": ["-l"], "cols": 80, "rows": 24 }]),
            ],
            "resizeShell" => vec![json!(["shell-1", 80, 24])],
            "vmFetch" => vec![
                json!([3000, "http://127.0.0.1/"]),
                json!([3000, "http://127.0.0.1/", { "method": "POST", "headers": { "x-test": "1" } }]),
            ],
            "scheduleCron" => vec![
                json!([{ "id": "job-1", "schedule": "* * * * *", "action": { "type": "exec", "command": "echo", "args": ["hi"] }, "overlap": "skip" }]),
            ],
            "createSession" => vec![
                json!(["default"]),
                json!(["default", { "cwd": "/workspace", "env": { "A": "B" }, "skipOsInstructions": true, "additionalInstructions": "test" }]),
            ],
            "sendPrompt" => vec![json!(["session-1", "hello"])],
            "respondPermission" => vec![json!(["session-1", "permission-1", "once"])],
            "createSignedPreviewUrl" => vec![json!([3000, 60])],
            other => return Err(anyhow!("unknown action {other}")),
        };
        variants
            .into_iter()
            .map(|value| abi::codec::encode_positional(&value))
            .collect()
    }

    pub fn encoded_invalid_client_arg_variants(name: &str) -> Result<Vec<(&'static str, Vec<u8>)>> {
        let variants = match name {
            "readFile"
            | "stat"
            | "mkdir"
            | "readdir"
            | "readdirEntries"
            | "exists"
            | "readdirRecursive"
            | "closeShell"
            | "waitShell"
            | "cancelCronJob"
            | "closeSession"
            | "getSessionEvents"
            | "expireSignedPreviewUrl" => {
                vec![("path/id must be a string", json!([42]))]
            }
            "writeFile" | "writeShell" => {
                vec![(
                    "content must be string or Uint8Array",
                    json!(["/workspace/file.txt", { "bad": true }]),
                )]
            }
            "move" => vec![(
                "destination must be a string",
                json!(["/workspace/a.txt", 42]),
            )],
            "deleteFile" => vec![(
                "recursive option must be boolean",
                json!(["/workspace/file.txt", { "recursive": "yes" }]),
            )],
            "writeFiles" => vec![(
                "entry path must be a string",
                json!([[{ "path": 42, "content": "a" }]]),
            )],
            "readFiles" => vec![("paths must be strings", json!([[42]]))],
            "exec" => vec![(
                "env option values must be strings",
                json!(["echo hello", { "env": { "A": 42 } }]),
            )],
            "spawn" => vec![("args must be strings", json!(["node", [42]]))],
            "waitProcess" | "killProcess" | "stopProcess" | "getProcess" | "closeProcessStdin" => {
                vec![("pid must be a number", json!(["42"]))]
            }
            "listProcesses"
            | "allProcesses"
            | "processTree"
            | "listCronJobs"
            | "listPersistedSessions"
            | "listMounts"
            | "listSoftware" => vec![(
                "zero-arg action must not accept extras",
                json!(["unexpected"]),
            )],
            "writeProcessStdin" => {
                vec![(
                    "stdin content must be string or Uint8Array",
                    json!([42, { "bad": true }]),
                )]
            }
            "openShell" => vec![("cols option must be numeric", json!([{ "cols": "wide" }]))],
            "resizeShell" => vec![("cols must be numeric", json!(["shell-1", "80", 24]))],
            "vmFetch" => vec![("port must be numeric", json!(["3000", "http://127.0.0.1/"]))],
            "scheduleCron" => vec![(
                "cron job requires a schedule/action shape",
                json!([{ "id": "job-1" }]),
            )],
            "createSession" => vec![("agent type must be a string", json!([42]))],
            "sendPrompt" => vec![(
                "prompt must be a string",
                json!(["session-1", { "text": "hello" }]),
            )],
            "respondPermission" => vec![(
                "permission response must be a string",
                json!(["session-1", "permission-1", 42]),
            )],
            "createSignedPreviewUrl" => vec![("ttl must be numeric", json!([3000, "60"]))],
            other => return Err(anyhow!("unknown action {other}")),
        };
        variants
            .into_iter()
            .map(|(case, value)| {
                abi::codec::encode_positional(&value).map(|encoded| (case, encoded))
            })
            .collect()
    }

    pub fn encode_sample_reply(name: &str) -> Result<Vec<u8>> {
        match name {
            "readFile" => encode(&serde_bytes::ByteBuf::from(vec![1, 2, 3])),
            "writeFile"
            | "mkdir"
            | "move"
            | "deleteFile"
            | "killProcess"
            | "stopProcess"
            | "writeProcessStdin"
            | "closeProcessStdin"
            | "writeShell"
            | "resizeShell"
            | "closeShell"
            | "cancelCronJob"
            | "closeSession"
            | "respondPermission"
            | "expireSignedPreviewUrl" => encode(&()),
            "stat" => encode(&VirtualStat {
                mode: 0o100644,
                size: 3,
                blocks: 1,
                dev: 1,
                rdev: 0,
                is_directory: false,
                is_symbolic_link: false,
                atime_ms: 1.0,
                mtime_ms: 2.0,
                ctime_ms: 3.0,
                birthtime_ms: 4.0,
                ino: 5,
                nlink: 1,
                uid: 1000,
                gid: 1000,
            }),
            "readdir" => encode(&vec!["a.txt".to_owned()]),
            "readdirEntries" => encode(&Some(vec![filesystem::ReaddirEntryDto {
                name: "a.txt".to_owned(),
                is_directory: false,
                is_symbolic_link: false,
            }])),
            "exists" => encode(&true),
            "writeFiles" => encode(&vec![filesystem::BatchWriteResultDto {
                path: "/workspace/a.txt".to_owned(),
                success: true,
                error: None,
            }]),
            "readFiles" => encode(&vec![filesystem::BatchReadResultDto {
                path: "/workspace/a.txt".to_owned(),
                content: Some(serde_bytes::ByteBuf::from(vec![1, 2, 3])),
                error: None,
            }]),
            "readdirRecursive" => encode(&vec![DirEntry {
                path: "/workspace/a.txt".to_owned(),
                entry_type: DirEntryType::File,
                size: 3,
            }]),
            "exec" => encode(&process::ExecResultDto {
                exit_code: 0,
                stdout: "ok\n".to_owned(),
                stderr: String::new(),
            }),
            "spawn" => encode(&SpawnHandle { pid: 42 }),
            "waitProcess" | "waitShell" => encode(&0i32),
            "listProcesses" => encode(&vec![spawned_process_info()]),
            "allProcesses" => encode(&vec![process_info()]),
            "processTree" => encode(&vec![ProcessTreeNode {
                info: process_info(),
                children: Vec::new(),
            }]),
            "getProcess" => encode(&spawned_process_info()),
            "openShell" => encode(&shell::OpenShellDto {
                shell_id: "shell-1".to_owned(),
            }),
            "vmFetch" => encode(&network::FetchResponseDto {
                status: 200,
                status_text: "OK".to_owned(),
                headers: BTreeMap::from([("content-type".to_owned(), "text/plain".to_owned())]),
                body: serde_bytes::ByteBuf::from(b"ok".to_vec()),
            }),
            "scheduleCron" => encode(&cron::ScheduledCronDto {
                id: "job-1".to_owned(),
            }),
            "listCronJobs" => encode(&vec![cron::CronJobInfoDto {
                id: "job-1".to_owned(),
                schedule: "* * * * *".to_owned(),
                overlap: CronOverlap::Skip,
                last_run: Some(1.0),
                next_run: Some(2.0),
            }]),
            "createSession" => encode_create_session_reply("session-1"),
            "sendPrompt" => encode(&session::PromptResultDto {
                response: JsonRpcResponse {
                    jsonrpc: "2.0".to_owned(),
                    id: None,
                    result: Some(json!({ "ok": true })),
                    error: None,
                },
                text: "hello".to_owned(),
            }),
            "listPersistedSessions" => encode(&vec![session::PersistedSessionDto {
                session_id: "session-1".to_owned(),
                agent_type: "default".to_owned(),
                created_at: 1.0,
                status: "running",
            }]),
            "getSessionEvents" => encode(&vec![session::PersistedSessionEventDto {
                session_id: "session-1".to_owned(),
                seq: 1,
                event: json!({ "jsonrpc": "2.0", "method": "session/update", "params": {} }),
                created_at: 1.0,
            }]),
            "createSignedPreviewUrl" => encode(&preview::SignedPreviewUrlDto {
                path: "/request/preview/token-1".to_owned(),
                token: "token-1".to_owned(),
                port: 3000,
                expires_at: 1.0,
            }),
            "listMounts" => encode(&vec![crate::config::MountInfoDto {
                path: "/data".to_owned(),
                kind: "host_dir".to_owned(),
                config: None,
                read_only: false,
            }]),
            "listSoftware" => encode(&vec![crate::config::SoftwareInfoDto {
                package: "@agentos-software/common".to_owned(),
                kind: "wasm-commands".to_owned(),
                version: Some("0.0.1".to_owned()),
                commands: vec!["ls".to_owned()],
            }]),
            other => Err(anyhow!("unknown action {other}")),
        }
    }

    pub fn encode_sample_event(name: &str) -> Result<Vec<u8>> {
        match name {
            "sessionEvent" => session::encode_session_event(
                "session-1",
                &json!({ "jsonrpc": "2.0", "method": "session/update", "params": {} }),
            ),
            "permissionRequest" => session::encode_permission_request_event(
                "session-1",
                "permission-1",
                Some("run command"),
                &json!({ "toolCall": { "title": "Bash" } }),
            ),
            "agentCrashed" => session::encode_agent_crashed_event(
                "session-1",
                &AgentExitEvent {
                    session_id: "live-session-1".to_owned(),
                    agent_type: "pi".to_owned(),
                    process_id: "proc-1".to_owned(),
                    exit_code: Some(1),
                    restart: "restarted".to_owned(),
                    restart_count: 1,
                    max_restarts: 3,
                },
            ),
            "vmBooted" => crate::vm::encode_vm_booted_event(),
            "vmShutdown" => crate::vm::encode_vm_shutdown_event("sleep"),
            "processOutput" => shell::encode_process_output_event(42, "stdout", b"hello".to_vec()),
            "processExit" => shell::encode_process_exit_event(42, 0),
            "shellData" => shell::encode_shell_data_event("shell-1", b"hello".to_vec()),
            "shellStderr" => shell::encode_shell_stderr_event("shell-1", b"oops".to_vec()),
            "shellExit" => shell::encode_shell_exit_event("shell-1", 0),
            "cronEvent" => cron::encode_cron_event(&CronEvent::Fire {
                job_id: "job-1".to_owned(),
                time: chrono::Utc::now(),
            }),
            other => Err(anyhow!("unknown event {other}")),
        }
    }

    pub fn decode_reply_value(bytes: &[u8]) -> Result<CborValue> {
        ciborium::from_reader(std::io::Cursor::new(bytes)).map_err(Into::into)
    }

    fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
        abi::codec::encode_json_compat_to_vec(value)
    }

    pub fn encode_create_session_reply(session_id: &str) -> Result<Vec<u8>> {
        encode(&session_id)
    }

    fn spawned_process_info() -> SpawnedProcessInfo {
        SpawnedProcessInfo {
            pid: 42,
            command: "node".to_owned(),
            args: vec!["server.js".to_owned()],
            running: true,
            exit_code: None,
            started_at: 1,
        }
    }

    fn process_info() -> ProcessInfo {
        ProcessInfo {
            pid: 42,
            ppid: 1,
            pgid: 42,
            sid: 42,
            driver: "native".to_owned(),
            command: "node".to_owned(),
            args: vec!["server.js".to_owned()],
            cwd: "/workspace".to_owned(),
            status: ProcessStatus::Running,
            exit_code: None,
            start_time: 1.0,
            exit_time: None,
        }
    }
}

/// Dispatch one decoded action against a live VM. `host` provides the actor's
/// SQLite database (via `db_*`) for the persistence-backed arms (signed preview
/// URLs + session metadata); `vm` is the live `AgentOs`; `vars` is the
/// ephemeral session-resume state.
///
/// ⚠️ SOURCE OF TRUTH / KEEP IN SYNC ⚠️
/// This match statement is mirrored one-to-one by `contract_surface.rs`, which
/// generates the TypeScript `AgentOsActions` surface used to type
/// `createClient<typeof registry>()`. Every `"name" =>` arm below must have a
/// corresponding contract row with matching positional args and serialized
/// return type. Update both in the same change.
pub(crate) async fn dispatch(
    host: &HostCtx,
    vm: &AgentOs,
    config: &crate::config::AgentOsConfigJson,
    vars: &mut Vars,
    name: &str,
    args: &[u8],
    token: u64,
) {
    match name {
        "readFile" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::read_file(vm, &path).await {
                // Wrap as serde_bytes so it serializes as a byte string, which
                // the JSON-compat encoder re-wraps as `["$Uint8Array", base64]`.
                Ok(bytes) => reply_ok(host, token, &serde_bytes::ByteBuf::from(bytes)),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "writeFile" => match decode_as::<(String, WriteFileContent)>(args) {
            Ok((path, contents)) => {
                match filesystem::write_file(vm, &path, contents.into_bytes()).await {
                    Ok(()) => reply_ok(host, token, &()),
                    Err(error) => reply_err(host, token, error),
                }
            }
            Err(error) => reply_err(host, token, error),
        },
        "stat" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::stat(vm, &path).await {
                Ok(vstat) => reply_ok(host, token, &vstat),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "mkdir" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::mkdir(vm, &path).await {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "readdir" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::readdir(vm, &path).await {
                Ok(entries) => reply_ok(host, token, &entries),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "readdirEntries" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::readdir_entries(vm, &path).await {
                Ok(entries) => reply_ok(host, token, &entries),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "exists" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::exists(vm, &path).await {
                Ok(present) => reply_ok(host, token, &present),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "move" => match decode_as::<(String, String)>(args) {
            Ok((from, to)) => match filesystem::move_path(vm, &from, &to).await {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "deleteFile" => {
            // TS may omit the trailing options object (array length 1 or 2).
            let decoded = decode_as::<(String, Option<filesystem::DeleteOptionsArg>)>(args)
                .map(|(path, options)| (path, options.unwrap_or_default().recursive))
                .or_else(|_| decode_as::<(String,)>(args).map(|(path,)| (path, false)));
            match decoded {
                Ok((path, recursive)) => {
                    match filesystem::delete_file(vm, &path, recursive).await {
                        Ok(()) => reply_ok(host, token, &()),
                        Err(error) => reply_err(host, token, error),
                    }
                }
                Err(error) => reply_err(host, token, error),
            }
        }
        "writeFiles" => match decode_as::<(Vec<WriteFilesEntryArg>,)>(args) {
            Ok((entries,)) => {
                let results = filesystem::write_files(vm, entries).await;
                reply_ok(host, token, &results);
            }
            Err(error) => reply_err(host, token, error),
        },
        "readFiles" => match decode_as::<(Vec<String>,)>(args) {
            Ok((paths,)) => {
                let results = filesystem::read_files(vm, paths).await;
                reply_ok(host, token, &results);
            }
            Err(error) => reply_err(host, token, error),
        },
        "readdirRecursive" => match decode_as::<(String,)>(args) {
            Ok((path,)) => match filesystem::readdir_recursive(vm, &path).await {
                Ok(entries) => reply_ok(host, token, &entries),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "exec" => {
            // The trailing options object is optional on the TS side, so a
            // 1-arg call decodes via the fallback (mirrors `spawn`).
            let decoded = decode_as::<(String, Option<process::ExecActionOptions>)>(args)
                .or_else(|_| decode_as::<(String,)>(args).map(|(command,)| (command, None)));
            match decoded {
                Ok((command, options)) => {
                    match process::exec(vm, &command, options.unwrap_or_default()).await {
                        Ok(result) => reply_ok(host, token, &result),
                        Err(error) => reply_err(host, token, error),
                    }
                }
                Err(error) => reply_err(host, token, error),
            }
        }
        "spawn" => {
            // The trailing options object is optional on the TS side, so a
            // 2-arg call decodes via the fallback.
            let decoded =
                decode_as::<(String, Vec<String>, Option<process::SpawnActionOptions>)>(args)
                    .or_else(|_| {
                        decode_as::<(String, Vec<String>)>(args)
                            .map(|(command, spawn_args)| (command, spawn_args, None))
                    });
            match decoded {
                Ok((command, spawn_args, options)) => match process::spawn(
                    host,
                    vm,
                    vars,
                    &command,
                    spawn_args,
                    options.unwrap_or_default(),
                ) {
                    Ok(handle) => reply_ok(host, token, &handle),
                    Err(error) => reply_err(host, token, error),
                },
                Err(error) => reply_err(host, token, error),
            }
        }
        // Long-running wait: replies from a spawned task so it does not occupy
        // the serial action worker (a waitProcess held for the process lifetime
        // would starve every later action, including the stdin writes the
        // process needs to make progress).
        "waitProcess" => match decode_as::<(u32,)>(args) {
            Ok((pid,)) => {
                let host = host.clone();
                let vm = vm.clone();
                vars.shell_tasks.push(tokio::spawn(async move {
                    match process::wait_process(&vm, pid).await {
                        Ok(code) => reply_ok(&host, token, &code),
                        Err(error) => reply_err(&host, token, error),
                    }
                }));
            }
            Err(error) => reply_err(host, token, error),
        },
        "killProcess" => match decode_as::<(u32,)>(args) {
            Ok((pid,)) => match process::kill_process(vm, pid) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "stopProcess" => match decode_as::<(u32,)>(args) {
            Ok((pid,)) => match process::stop_process(vm, pid) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "listProcesses" => {
            let processes = process::list_processes(vm);
            reply_ok(host, token, &processes);
        }
        "allProcesses" => match process::all_processes(vm).await {
            Ok(processes) => reply_ok(host, token, &processes),
            Err(error) => reply_err(host, token, error),
        },
        "processTree" => match process::process_tree(vm).await {
            Ok(tree) => reply_ok(host, token, &tree),
            Err(error) => reply_err(host, token, error),
        },
        "getProcess" => match decode_as::<(u32,)>(args) {
            Ok((pid,)) => match process::get_process(vm, pid) {
                Ok(info) => reply_ok(host, token, &info),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "writeProcessStdin" => match decode_as::<(u32, WriteFileContent)>(args) {
            Ok((pid, data)) => match process::write_process_stdin(vm, pid, data) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "closeProcessStdin" => match decode_as::<(u32,)>(args) {
            Ok((pid,)) => match process::close_process_stdin(vm, pid) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "vmFetch" => {
            // Trailing options object is optional (length 2 or 3).
            let decoded = decode_as::<(u16, String, Option<network::FetchOptions>)>(args)
                .map(|(port, url, options)| (port, url, options.unwrap_or_default()))
                .or_else(|_| {
                    decode_as::<(u16, String)>(args)
                        .map(|(port, url)| (port, url, network::FetchOptions::default()))
                });
            match decoded {
                Ok((port, url, options)) => match network::fetch(vm, port, &url, options).await {
                    Ok(response) => reply_ok(host, token, &response),
                    Err(error) => reply_err(host, token, error),
                },
                Err(error) => reply_err(host, token, error),
            }
        }
        "scheduleCron" => match decode_as::<(cron::CronJobOptionsDto,)>(args) {
            Ok((options,)) => match cron::schedule_cron(host, vm, vars, options) {
                Ok(handle) => reply_ok(host, token, &handle),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "listCronJobs" => reply_ok(host, token, &cron::list_cron_jobs(vm)),
        "cancelCronJob" => match decode_as::<(String,)>(args) {
            Ok((id,)) => {
                cron::cancel_cron_job(vm, &id);
                reply_ok(host, token, &());
            }
            Err(error) => reply_err(host, token, error),
        },
        "createSession" => {
            // Trailing options object is optional (length 1 or 2).
            let decoded = decode_as::<(String, Option<session::CreateSessionOptionsDto>)>(args)
                .map(|(agent_type, options)| (agent_type, options.unwrap_or_default()))
                .or_else(|_| {
                    decode_as::<(String,)>(args).map(|(agent_type,)| {
                        (agent_type, session::CreateSessionOptionsDto::default())
                    })
                });
            match decoded {
                Ok((agent_type, options)) => {
                    match session::create_session(host, vm, vars, &agent_type, options).await {
                        Ok(id) => reply_ok_encoded(
                            host,
                            token,
                            contract::encode_create_session_reply(&id),
                        ),
                        Err(error) => {
                            tracing::error!(?error, agent_type, "create_session failed");
                            reply_err(host, token, error)
                        }
                    }
                }
                Err(error) => reply_err(host, token, error),
            }
        }
        "sendPrompt" => match decode_as::<(String, String)>(args) {
            Ok((session_id, text)) => {
                match session::send_prompt(host, vm, vars, &session_id, &text).await {
                    Ok(result) => reply_ok(host, token, &result),
                    Err(error) => reply_err(host, token, error),
                }
            }
            Err(error) => reply_err(host, token, error),
        },
        "closeSession" => match decode_as::<(String,)>(args) {
            Ok((session_id,)) => match session::close_session(host, vm, vars, &session_id).await {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "listPersistedSessions" => match session::list_persisted_sessions(host, vm).await {
            Ok(sessions) => reply_ok(host, token, &sessions),
            Err(error) => reply_err(host, token, error),
        },
        "getSessionEvents" => match decode_as::<(String,)>(args) {
            Ok((session_id,)) => match session::get_session_events(host, &session_id).await {
                Ok(events) => reply_ok(host, token, &events),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "respondPermission" => match decode_as::<(String, String, String)>(args) {
            Ok((session_id, permission_id, reply)) => {
                match session::respond_permission(vm, vars, &session_id, &permission_id, &reply)
                    .await
                {
                    Ok(()) => reply_ok(host, token, &()),
                    Err(error) => reply_err(host, token, error),
                }
            }
            Err(error) => reply_err(host, token, error),
        },
        "createSignedPreviewUrl" => match decode_as::<(u16, u64)>(args) {
            Ok((port, ttl_seconds)) => match preview::create(host, port, ttl_seconds).await {
                Ok(dto) => reply_ok(host, token, &dto),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "expireSignedPreviewUrl" => match decode_as::<(String,)>(args) {
            Ok((token_arg,)) => match preview::expire(host, &token_arg).await {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "openShell" => {
            let decoded = decode_as::<(Option<shell::OpenShellActionOptions>,)>(args)
                .or_else(|_| decode_as::<()>(args).map(|()| (None,)));
            match decoded {
                Ok((options,)) => {
                    match shell::open_shell(host, vm, vars, options.unwrap_or_default()) {
                        Ok(dto) => reply_ok(host, token, &dto),
                        Err(error) => reply_err(host, token, error),
                    }
                }
                Err(error) => reply_err(host, token, error),
            }
        }
        "writeShell" => match decode_as::<(String, WriteFileContent)>(args) {
            Ok((shell_id, data)) => match shell::write_shell(vm, &shell_id, data).await {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "resizeShell" => match decode_as::<(String, u16, u16)>(args) {
            Ok((shell_id, cols, rows)) => match shell::resize_shell(vm, &shell_id, cols, rows) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        "closeShell" => match decode_as::<(String,)>(args) {
            Ok((shell_id,)) => match shell::close_shell(vm, &shell_id) {
                Ok(()) => reply_ok(host, token, &()),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
        // Long-running wait: replies from a spawned task so it does not occupy
        // the serial action worker (the shell CLI calls waitShell up front and
        // streams writeShell input afterwards; holding the worker here would
        // deadlock the shell — input can never arrive to end the wait).
        "waitShell" => match decode_as::<(String,)>(args) {
            Ok((shell_id,)) => {
                let host = host.clone();
                let vm = vm.clone();
                vars.shell_tasks.push(tokio::spawn(async move {
                    match shell::wait_shell(&vm, &shell_id).await {
                        Ok(exit_code) => reply_ok(&host, token, &exit_code),
                        Err(error) => reply_err(&host, token, error),
                    }
                }));
            }
            Err(error) => reply_err(host, token, error),
        },
        // Config introspection: echo the actor's declarative mount / software
        // config (no VM round-trip — the kernel has no runtime mount table and
        // software is the requested bundle expanded TS-side in buildConfigJson).
        "listMounts" => reply_ok(host, token, &config.list_mounts()),
        "listSoftware" => {
            // Config carries package/kind/version; the command names each
            // wasm-commands package ships come from the live VM (host package
            // dirs), zipped in here by package name.
            let mut list = config.list_software();
            let commands: HashMap<String, Vec<String>> = match vm.provided_commands().await {
                Ok(commands) => commands.into_iter().collect(),
                Err(error) => {
                    reply_err(host, token, error.into());
                    return;
                }
            };
            for dto in &mut list {
                if let Some(cmds) = commands.get(&dto.package) {
                    dto.commands = cmds.clone();
                }
            }
            reply_ok(host, token, &list);
        }
        other => {
            host.reply_err(
                token,
                &format!("agent-os action not implemented yet: {other}"),
            );
        }
    }
}
