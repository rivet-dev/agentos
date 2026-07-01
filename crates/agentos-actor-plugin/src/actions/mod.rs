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

pub mod cron;
pub mod filesystem;
pub mod network;
pub mod preview;
pub mod process;
pub mod session;
pub mod shell;

use std::collections::HashMap;

use agentos_client::AgentOs;
use anyhow::Result;
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
        self.live_sessions.clear();
    }
}

/// Decode positional CBOR args into `T`.
fn decode_as<T: DeserializeOwned>(args: &[u8]) -> Result<T> {
    abi::codec::decode_positional(args)
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

/// Reply failure with the error message (matches `ActionCall::err`).
fn reply_err(host: &HostCtx, token: u64, error: anyhow::Error) {
    let message = error.to_string();
    host.log_warn(&format!("agent-os action failed: {message}"));
    host.reply_err(token, &message);
}

/// Dispatch one decoded action against a live VM. `host` provides the actor's
/// SQLite database (via `db_*`) for the persistence-backed arms (signed preview
/// URLs + session metadata); `vm` is the live `AgentOs`; `vars` is the
/// ephemeral session-resume state.
///
/// ⚠️ SOURCE OF TRUTH / KEEP IN SYNC ⚠️
/// This match statement is mirrored one-to-one by the TypeScript
/// `AgentOsActions` interface in `packages/agentos/src/actor-actions.ts`, which
/// types the `createClient<typeof registry>()` handle. Every `"name" =>` arm
/// below must have a corresponding method there with matching positional args
/// and serialized return type. Update both in the same change.
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
        "exec" => match decode_as::<(String,)>(args) {
            Ok((command,)) => match process::exec(vm, &command).await {
                Ok(result) => reply_ok(host, token, &result),
                Err(error) => reply_err(host, token, error),
            },
            Err(error) => reply_err(host, token, error),
        },
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
            Ok((options,)) => match cron::schedule_cron(vm, options) {
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
                        Ok(id) => reply_ok(host, token, &id),
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
        "listPersistedSessions" => match session::list_persisted_sessions(host).await {
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
        "openShell" => match decode_as::<(Option<shell::OpenShellActionOptions>,)>(args) {
            Ok((options,)) => {
                match shell::open_shell(host, vm, vars, options.unwrap_or_default()) {
                    Ok(dto) => reply_ok(host, token, &dto),
                    Err(error) => reply_err(host, token, error),
                }
            }
            Err(error) => reply_err(host, token, error),
        },
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
            let commands: HashMap<String, Vec<String>> = vm.provided_commands().into_iter().collect();
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
