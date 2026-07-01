//! Agent OS actor plugin (`cdylib`) — the **plugin side** of
//! `rivet-actor-plugin-abi`, the inverse of the RivetKit host loader.
//!
//! RivetKit `dlopen`s this library, verifies the ABI, and drives the agent-os
//! actor through the exported symbols. This crate owns the plugin tokio runtime
//! and (in the run-loop port that builds on this foundation) imports the
//! **unmodified** `agentos-client` to spawn + drive the sidecar, calling back
//! into the host `HostVtable` for durable storage and events.
//!
//! This file is the export/ABI/runtime skeleton (spec phase 4 foundation). The
//! actor run loop + the plugin-side host-vtable bridge are layered on top next.

#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use rivet_actor_plugin_abi as abi;
use tokio_util::sync::CancellationToken;

mod actions;
mod config;
mod host_ctx;
mod http;
mod persistence;
mod vm;

#[cfg(test)]
mod persistence_e2e;

use std::sync::Arc;

/// Process-global plugin state created once per `dlopen` (spec §5.2): the
/// plugin's own tokio runtime (`enable_all` — the time driver is required by
/// agentos-client hot paths).
struct Plugin {
    runtime: tokio::runtime::Runtime,
    /// Unique sidecar pool for this plugin runtime (spec §7): one sidecar
    /// process per dlopen, shared across the actors it hosts, never the global
    /// `"default"` pool.
    pool: String,
}

fn write_err(out: *mut abi::OwnedBuf, msg: &str) {
    if !out.is_null() {
        unsafe {
            *out = abi::OwnedBuf::from_vec(msg.as_bytes().to_vec());
        }
    }
}

#[no_mangle]
pub extern "C" fn rivet_actor_abi_magic() -> u64 {
    abi::RIVET_ACTOR_ABI_MAGIC
}

#[no_mangle]
pub extern "C" fn rivet_actor_abi_version() -> u64 {
    abi::RIVET_ACTOR_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn rivet_actor_plugin_init(out_err: *mut abi::OwnedBuf) -> *mut c_void {
    // Debug-only tracing to stderr, gated on AGENTOS_PLUGIN_LOG (RUST_LOG
    // syntax). Without a subscriber every tracing::warn! from the embedded
    // agentos-client (e.g. a failed fire-and-forget shell write) is silently
    // dropped, which makes plugin-side failures undiagnosable.
    if let Ok(filter) = std::env::var("AGENTOS_PLUGIN_LOG") {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
            .with_writer(std::io::stderr)
            .try_init();
    }
    let built = std::panic::catch_unwind(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
    });
    match built {
        Ok(Ok(runtime)) => {
            let pool = format!("agentos-plugin-{}", uuid::Uuid::new_v4());
            Box::into_raw(Box::new(Plugin { runtime, pool })) as *mut c_void
        }
        Ok(Err(e)) => {
            write_err(out_err, &format!("build plugin runtime: {e}"));
            std::ptr::null_mut()
        }
        Err(_) => {
            write_err(out_err, "panic building plugin runtime");
            std::ptr::null_mut()
        }
    }
}

/// Opaque factory handle: the plugin runtime handle the actor loop spawns on +
/// the resolved sidecar binary path. (`config_json` parsing into the full
/// `AgentOsActorConfig` is layered on next; the sidecar path is what the VM
/// bring-up needs immediately.)
struct Factory {
    runtime: tokio::runtime::Handle,
    sidecar_path: String,
    /// Parsed client `config_json` (spec §7); rebuilt into a fresh
    /// `AgentOsConfig` per VM bring-up because `AgentOsConfig` is non-`Clone`.
    config: Arc<config::AgentOsConfigJson>,
    /// Per-plugin-runtime sidecar pool, copied from the owning `Plugin`.
    pool: String,
}

#[no_mangle]
pub extern "C" fn rivet_actor_factory_new(
    plugin: *mut c_void,
    config_json: abi::BorrowedBuf,
    sidecar_path: abi::BorrowedBuf,
    out_err: *mut abi::OwnedBuf,
) -> *mut c_void {
    if plugin.is_null() {
        write_err(out_err, "null plugin handle");
        return std::ptr::null_mut();
    }
    let plugin_ref = unsafe { &*(plugin as *const Plugin) };
    let runtime = plugin_ref.runtime.handle().clone();
    let pool = plugin_ref.pool.clone();
    let sidecar_path = unsafe { String::from_utf8_lossy(sidecar_path.as_slice()).into_owned() };
    let config_str = unsafe { String::from_utf8_lossy(config_json.as_slice()).into_owned() };
    let config = match config::AgentOsConfigJson::parse(&config_str) {
        Ok(config) => Arc::new(config),
        Err(error) => {
            write_err(out_err, &format!("parse config_json: {error}"));
            return std::ptr::null_mut();
        }
    };
    Box::into_raw(Box::new(Factory {
        runtime,
        sidecar_path,
        config,
        pool,
    })) as *mut c_void
}

/// Send wrapper for the host completion `user_data` so it can move into the
/// spawned actor task.
struct SendUserData(*mut c_void);
unsafe impl Send for SendUserData {}

struct RunGuard {
    done: abi::CompletionFn,
    ud: SendUserData,
    fired: bool,
}

impl RunGuard {
    fn finish(&mut self, result: abi::AbiResult) {
        if !self.fired {
            self.fired = true;
            (self.done)(self.ud.0, result);
        }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.finish(abi::AbiResult::status_only(abi::AbiStatus::Cancelled));
    }
}

/// Run one actor instance: build the `HostCtx` bridge over the host vtable,
/// spawn the actor loop on the plugin runtime, and signal completion when the
/// event stream closes (host cancel). The VM-dispatch layer (decode actions +
/// drive the sidecar via `agentos-client`) slots into `actor_loop` next.
#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn rivet_actor_run(
    factory: *mut c_void,
    host: *const abi::HostVtable,
    done: abi::CompletionFn,
    user_data: *mut c_void,
) -> *mut c_void {
    let instance = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let factory = &*(factory as *const Factory);
        let host_ctx = host_ctx::HostCtx::from_vtable(*host);
        let sidecar_path = factory.sidecar_path.clone();
        let config = factory.config.clone();
        let pool = factory.pool.clone();
        let ud = SendUserData(user_data);
        let cancel = CancellationToken::new();
        let run_cancel = cancel.clone();
        let handle = factory.runtime.spawn(async move {
            let mut guard = RunGuard {
                done,
                ud,
                fired: false,
            };
            actor_loop(host_ctx, sidecar_path, config, pool, run_cancel).await;
            guard.finish(abi::AbiResult::ok(abi::OwnedBuf::empty()));
        });
        Instance {
            abort: Some(handle.abort_handle()),
            cancel,
        }
    }))
    .unwrap_or_else(|_| {
        done(
            user_data,
            abi::AbiResult::status_only(abi::AbiStatus::Panic),
        );
        Instance {
            abort: None,
            cancel: CancellationToken::new(),
        }
    });
    Box::into_raw(Box::new(instance)) as *mut c_void
}

/// Plugin-side actor run loop (ported from `rivetkit-agent-os::run`): drains
/// host lifecycle events via the `HostCtx` bridge, brings the VM up lazily on
/// the first action, tears it down on Sleep/Destroy, and ends when the host
/// closes the stream. Action dispatch (decode + drive the VM) is the remaining
/// layer; until it lands, actions reply with a clear not-yet-ported error.
async fn actor_loop(
    host: host_ctx::HostCtx,
    sidecar_path: String,
    config: Arc<config::AgentOsConfigJson>,
    pool: String,
    cancel: CancellationToken,
) {
    // Ensure the agent-os schema exists before handling events (best-effort;
    // mirrors rivetkit-agent-os run.rs).
    if host.sql_is_enabled() {
        let _ = persistence::migrate(&host).await;
    }
    // The VM handle + actor vars live on a dedicated worker task that drains
    // stateful jobs (Action/Http/Sleep/Destroy) serially in submission order.
    // The event loop below only forwards those jobs and answers
    // connection-lifecycle events (ConnPreflight/ConnOpen/SerializeState) inline.
    // This is what keeps the loop responsive: a cold VM bring-up (>5s) on the
    // first action no longer blocks the loop, so a queued ConnOpen is still
    // answered within RivetKit's 5s websocket-setup deadline. Previously
    // `ensure_vm().await` ran inline in this loop, so the first action starved
    // ConnOpen, the actor websocket connection setup timed out at 5000ms, and
    // live `sessionEvent` streaming silently delivered zero events.
    let (job_tx, job_rx) = tokio::sync::mpsc::unbounded_channel::<ActorJob>();
    let worker = tokio::spawn(actor_worker(
        host.clone(),
        sidecar_path,
        config,
        pool,
        job_rx,
        cancel.clone(),
    ));
    // Signal readiness: the native-plugin factory uses manual startup-ready, so
    // the host's `start()` caller blocks until we report this. The VM is brought
    // up lazily on the first action (on the worker), so the actor is ready once
    // the schema is in place and the event loop is about to run.
    host.startup_ready(true, "");
    loop {
        let event = tokio::select! {
            _ = cancel.cancelled() => break,
            event = host.next_event() => event,
        };
        let Some((tag, token, payload)) = event else {
            break;
        };
        match abi::AbiEventTag::from_u32(tag) {
            Some(abi::AbiEventTag::Action) => {
                if job_tx.send(ActorJob::Action { token, payload }).is_err() {
                    let _ = host.reply_err(token, "agent-os actor worker unavailable");
                }
            }
            Some(abi::AbiEventTag::Http) => {
                if job_tx.send(ActorJob::Http { token, payload }).is_err() {
                    let _ = host.reply_err(token, "agent-os actor worker unavailable");
                }
            }
            Some(abi::AbiEventTag::ConnPreflight) => {
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::ConnOpen) => {
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::Subscribe) => {
                // Accept the connection's event subscription (e.g. `sessionEvent`)
                // so RivetKit registers it and routes matching broadcasts to this
                // connection. Subscription state is tracked by RivetKit core; the
                // plugin only needs to accept. Previously this fell through to the
                // generic "event not supported" reply, which REJECTED every
                // subscription — so the connection was never registered and every
                // broadcast (sessionEvent, vmBooted, ...) was silently dropped,
                // making live `sessionEvent` streaming deliver nothing.
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::SerializeState) => {
                // Agent OS persists durable data through SQLite and rebuilds VM/process
                // state after wake, so there is no opaque actor state payload to return.
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::Sleep) => {
                if job_tx.send(ActorJob::Sleep { token }).is_err() {
                    let _ = host.reply_ok(token, Vec::new());
                }
            }
            Some(abi::AbiEventTag::Destroy) => {
                if job_tx.send(ActorJob::Destroy { token }).is_err() {
                    let _ = host.reply_ok(token, Vec::new());
                }
            }
            Some(t) if t.needs_reply() => {
                let _ = host.reply_err(token, "event not supported by agent-os actor");
            }
            _ => {}
        }
    }
    // Stream closed (cancel/teardown): stop accepting new jobs and let the
    // worker drain in-flight work + shut the VM down before we return.
    drop(job_tx);
    let _ = worker.await;
}

/// Stateful actor jobs serviced by the worker task in submission order. Keeping
/// VM-touching work (bring-up, action dispatch, HTTP proxy, shutdown) off the
/// event loop is what lets the loop answer connection-lifecycle events promptly.
enum ActorJob {
    Action { token: u64, payload: Vec<u8> },
    Http { token: u64, payload: Vec<u8> },
    Sleep { token: u64 },
    Destroy { token: u64 },
}

/// Owns the VM handle + actor vars and processes `ActorJob`s serially. Runs as a
/// sibling task to `actor_loop`; the loop forwards jobs and never blocks on the
/// VM, so a slow cold boot can't starve connection setup.
async fn actor_worker(
    host: host_ctx::HostCtx,
    sidecar_path: String,
    config: Arc<config::AgentOsConfigJson>,
    pool: String,
    mut job_rx: tokio::sync::mpsc::UnboundedReceiver<ActorJob>,
    cancel: CancellationToken,
) {
    let mut vm: Option<agentos_client::AgentOs> = None;
    let mut vars = actions::Vars::default();
    loop {
        let job = tokio::select! {
            _ = cancel.cancelled() => break,
            job = job_rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
        };
        match job {
            ActorJob::Action { token, payload } => {
                if let Err(error) =
                    vm::ensure_vm(&host, &sidecar_path, &config, &pool, &mut vm).await
                {
                    host.log_warn(&format!("agent-os vm bring-up failed: {error}"));
                    let _ = host.reply_err(token, &error);
                    continue;
                }
                let Some(vm_ref) = vm.as_ref() else {
                    let _ = host.reply_err(token, "vm unavailable after bring-up");
                    continue;
                };
                match abi::decode_action_payload(&payload) {
                    Ok((name, action_args)) => {
                        tracing::debug!(action = %name, "agent-os action start");
                        actions::dispatch(&host, vm_ref, &mut vars, &name, &action_args, token)
                            .await;
                        tracing::debug!(action = %name, "agent-os action done");
                    }
                    Err(_) => {
                        let _ = host.reply_err(token, "malformed action event payload");
                    }
                }
            }
            ActorJob::Http { token, payload } => {
                // Preview proxy: do NOT bring the VM up for HTTP (matches r6's
                // run loop, which passes `vm.as_ref()`); no VM → 404.
                let response = http::proxy_preview(&host, vm.as_ref(), &payload).await;
                let _ = host.reply_ok(token, response);
            }
            ActorJob::Sleep { token } => {
                vars.clear();
                vm::shutdown_vm(&host, &mut vm, "sleep").await;
                let _ = host.reply_ok(token, Vec::new());
            }
            ActorJob::Destroy { token } => {
                vars.clear();
                vm::shutdown_vm(&host, &mut vm, "destroy").await;
                let _ = host.reply_ok(token, Vec::new());
            }
        }
    }
    // Stream closed (cancel/teardown): best-effort VM shutdown.
    vars.clear();
    vm::shutdown_vm(&host, &mut vm, "error").await;
}

struct Instance {
    abort: Option<tokio::task::AbortHandle>,
    cancel: CancellationToken,
}

#[no_mangle]
pub extern "C" fn rivet_actor_cancel(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| unsafe {
        let inst = &*(instance as *const Instance);
        inst.cancel.cancel();
    });
}

#[no_mangle]
pub extern "C" fn rivet_actor_grace_deadline(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| unsafe {
        let inst = &*(instance as *const Instance);
        inst.cancel.cancel();
        if let Some(abort) = inst.abort.as_ref() {
            abort.abort();
        }
    });
}

#[no_mangle]
pub extern "C" fn rivet_actor_instance_free(instance: *mut c_void) {
    if !instance.is_null() {
        let _ = std::panic::catch_unwind(|| unsafe {
            drop(Box::from_raw(instance as *mut Instance));
        });
    }
}

#[no_mangle]
pub extern "C" fn rivet_actor_factory_free(factory: *mut c_void) {
    if !factory.is_null() {
        let _ = std::panic::catch_unwind(|| unsafe {
            drop(Box::from_raw(factory as *mut Factory));
        });
    }
}

#[no_mangle]
pub extern "C" fn rivet_actor_plugin_shutdown(plugin: *mut c_void) {
    if !plugin.is_null() {
        let _ = std::panic::catch_unwind(|| unsafe {
            drop(Box::from_raw(plugin as *mut Plugin));
        });
    }
}
