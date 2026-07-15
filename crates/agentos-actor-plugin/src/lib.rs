//! Agent OS actor plugin (`cdylib`) — the **plugin side** of
//! `rivet-actor-plugin-abi`, the inverse of the RivetKit host loader.
//!
//! RivetKit `dlopen`s this library, verifies the ABI, and drives the agent-os
//! actor through the exported symbols. This crate owns the plugin tokio runtime
//! and (in the run-loop port that builds on this foundation) imports the
//! **unmodified** `agentos-client` to spawn + drive the sidecar, calling back
//! into the host `HostVtable` for durable storage and events.
//!
//! The actor loop is implemented against RivetKit's portable context so the
//! dylib owns AgentOS behavior while the host owns lifecycle dispatch.

#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use rivet_actor_plugin_abi as abi;
use tokio_util::sync::CancellationToken;

pub mod actions;
mod config;
mod host_ctx;
mod http;
mod persistence;
mod vm;

#[cfg(test)]
mod persistence_e2e;

use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::thread;

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

extern "C" fn plugin_init(out_err: *mut abi::OwnedBuf) -> *mut c_void {
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

extern "C" fn factory_new(
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

struct SendUserData(*mut c_void);
unsafe impl Send for SendUserData {}

struct CompletionTarget {
    done: abi::CompletionFn,
    user_data: SendUserData,
}

impl CompletionTarget {
    fn finish(self, result: abi::AbiResult) {
        (self.done)(self.user_data.0, result);
    }

    fn finish_outcome(self, outcome: &Result<(), String>) {
        match outcome {
            Ok(()) => self.finish(abi::AbiResult::ok(abi::OwnedBuf::empty())),
            Err(message) => self.finish(abi::AbiResult::err(abi::OwnedBuf::from_vec(
                message.clone().into_bytes(),
            ))),
        }
    }
}

struct Instance {
    inner: Arc<InstanceInner>,
}

struct InstanceInner {
    bridge: abi::DylibEventBridge,
    join: Mutex<Option<thread::JoinHandle<()>>>,
    shutdown: Mutex<ShutdownState>,
    cancel: CancellationToken,
}

#[derive(Default)]
struct ShutdownState {
    started: bool,
    outcome: Option<Result<(), String>>,
    waiters: Vec<CompletionTarget>,
}

extern "C" fn instance_new(
    factory: *mut c_void,
    host: *const abi::HostVtable,
    start: abi::BorrowedBuf,
    out_err: *mut abi::OwnedBuf,
    terminal_done: abi::CompletionFn,
    user_data: *mut c_void,
) -> *mut c_void {
    if factory.is_null() || host.is_null() {
        write_err(out_err, "null factory or host handle");
        return std::ptr::null_mut();
    }
    let _: abi::InstanceStart =
        match ciborium::from_reader(Cursor::new(unsafe { start.as_slice() })) {
            Ok(start) => start,
            Err(error) => {
                write_err(out_err, &format!("decode instance start: {error}"));
                return std::ptr::null_mut();
            }
        };
    let factory = unsafe { &*(factory as *const Factory) };
    let (backend, bridge) = unsafe { abi::DylibBackend::from_host_vtable(&*host) };
    let host_ctx = host_ctx::HostCtx::from_backend(backend);
    let runtime = factory.runtime.clone();
    let sidecar_path = factory.sidecar_path.clone();
    let config = factory.config.clone();
    let pool = factory.pool.clone();
    let cancel = CancellationToken::new();
    let run_cancel = cancel.clone();
    let terminal = CompletionTarget {
        done: terminal_done,
        user_data: SendUserData(user_data),
    };
    let join = thread::spawn(move || {
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            runtime.block_on(actor_loop(host_ctx, sidecar_path, config, pool, run_cancel));
        }));
        match outcome {
            Ok(()) => terminal.finish(abi::AbiResult::ok(abi::OwnedBuf::empty())),
            Err(_) => terminal.finish(abi::AbiResult::status_only(abi::AbiStatus::Panic)),
        }
    });
    Box::into_raw(Box::new(Instance {
        inner: Arc::new(InstanceInner {
            bridge,
            join: Mutex::new(Some(join)),
            shutdown: Mutex::new(ShutdownState::default()),
            cancel,
        }),
    })) as *mut c_void
}

/// Plugin-side actor run loop: drains host lifecycle events through the
/// portable bridge, brings the VM up lazily on the first action, tears it down
/// on Sleep/Destroy, and ends when the host closes the stream.
async fn actor_loop(
    host: host_ctx::HostCtx,
    sidecar_path: String,
    config: Arc<config::AgentOsConfigJson>,
    pool: String,
    cancel: CancellationToken,
) {
    // Ensure the durable schema exists before accepting work. Starting with a
    // half-migrated actor would turn every later filesystem/session failure
    // into a misleading action error.
    if host.sql_is_enabled() {
        if let Err(error) = persistence::migrate(&host).await {
            let message = format!("agent-os schema migration failed: {error:#}");
            host.log_warn(&message);
            host.startup_ready(false, &message);
            return;
        }
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
    let (job_tx, job_rx) = tokio::sync::mpsc::channel::<ActorJob>(MAX_PENDING_ACTOR_JOBS);
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
        let Some(event) = event else {
            break;
        };
        match event {
            abi::Event::Action {
                name, args, reply, ..
            } => {
                enqueue_job(
                    &host,
                    &job_tx,
                    ActorJob::Action {
                        token: reply.0,
                        name,
                        args,
                    },
                );
            }
            abi::Event::Http { request, reply } => {
                enqueue_job(
                    &host,
                    &job_tx,
                    ActorJob::Http {
                        token: reply.0,
                        request,
                    },
                );
            }
            abi::Event::ConnPreflight { reply, .. } => {
                let _ = host.reply_ok(reply.0, Vec::new());
            }
            abi::Event::ConnOpen { reply, .. } => {
                let _ = host.reply_ok(reply.0, Vec::new());
            }
            abi::Event::Subscribe { reply, .. } => {
                // Accept the connection's event subscription (e.g. `sessionEvent`)
                // so RivetKit registers it and routes matching broadcasts to this
                // connection. Subscription state is tracked by RivetKit core; the
                // plugin only needs to accept. Previously this fell through to the
                // generic "event not supported" reply, which REJECTED every
                // subscription — so the connection was never registered and every
                // broadcast (sessionEvent, vmBooted, ...) was silently dropped,
                // making live `sessionEvent` streaming deliver nothing.
                let _ = host.reply_ok(reply.0, Vec::new());
            }
            abi::Event::SerializeState { reply } => {
                // Agent OS persists durable data through SQLite and rebuilds VM/process
                // state after wake, so there is no opaque actor state payload to return.
                let _ = host.reply_ok(reply.0, Vec::new());
            }
            abi::Event::Sleep { reply } => {
                enqueue_job(&host, &job_tx, ActorJob::Sleep { token: reply.0 });
            }
            abi::Event::Destroy { reply } => {
                enqueue_job(&host, &job_tx, ActorJob::Destroy { token: reply.0 });
            }
            abi::Event::QueueSend { reply, .. } | abi::Event::WebSocketOpen { reply, .. } => {
                let _ = host.reply_err(reply.0, "event not supported by agent-os actor");
            }
            abi::Event::ConnClosed { .. } => {}
        }
    }
    // Stream closed (cancel/teardown): stop accepting new jobs and let the
    // worker drain in-flight work + shut the VM down before we return.
    drop(job_tx);
    if let Err(error) = worker.await {
        host.log_warn(&format!("agent-os actor worker task failed: {error}"));
    }
}

/// Stateful actor jobs serviced by the worker task in submission order. Keeping
/// VM-touching work (bring-up, action dispatch, HTTP proxy, shutdown) off the
/// event loop is what lets the loop answer connection-lifecycle events promptly.
enum ActorJob {
    Action {
        token: u64,
        name: String,
        args: Vec<u8>,
    },
    Http {
        token: u64,
        request: Vec<u8>,
    },
    Sleep {
        token: u64,
    },
    Destroy {
        token: u64,
    },
}

const MAX_PENDING_ACTOR_JOBS: usize = 64;
const PENDING_ACTOR_JOBS_WARN_REMAINING: usize = 13;

impl ActorJob {
    fn token(&self) -> u64 {
        match self {
            Self::Action { token, .. }
            | Self::Http { token, .. }
            | Self::Sleep { token }
            | Self::Destroy { token } => *token,
        }
    }
}

fn enqueue_job(
    host: &host_ctx::HostCtx,
    sender: &tokio::sync::mpsc::Sender<ActorJob>,
    job: ActorJob,
) {
    let token = job.token();
    if sender.capacity() <= PENDING_ACTOR_JOBS_WARN_REMAINING {
        host.log_warn(&format!(
            "agent-os actor job queue is near its limit: remaining={} limit={MAX_PENDING_ACTOR_JOBS}",
            sender.capacity()
        ));
    }
    match sender.try_send(job) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            let _ = host.reply_err(
                token,
                &format!(
                    "agent-os actor pending-job limit {MAX_PENDING_ACTOR_JOBS} exceeded; raise MAX_PENDING_ACTOR_JOBS in the plugin build"
                ),
            );
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            let _ = host.reply_err(token, "agent-os actor worker unavailable");
        }
    }
}

/// Owns the VM handle + actor vars and processes `ActorJob`s serially. Runs as a
/// sibling task to `actor_loop`; the loop forwards jobs and never blocks on the
/// VM, so a slow cold boot can't starve connection setup.
async fn actor_worker(
    host: host_ctx::HostCtx,
    sidecar_path: String,
    config: Arc<config::AgentOsConfigJson>,
    pool: String,
    mut job_rx: tokio::sync::mpsc::Receiver<ActorJob>,
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
            ActorJob::Action {
                token,
                name,
                args: action_args,
            } => {
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
                tracing::debug!(action = %name, "agent-os action start");
                actions::dispatch(
                    &host,
                    vm_ref,
                    &config,
                    &mut vars,
                    &name,
                    &action_args,
                    token,
                )
                .await;
                tracing::debug!(action = %name, "agent-os action done");
            }
            ActorJob::Http { token, request } => {
                // Preview proxy: do NOT bring the VM up for HTTP (matches r6's
                // run loop, which passes `vm.as_ref()`); no VM → 404.
                let response = http::proxy_preview(&host, vm.as_ref(), &request).await;
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

extern "C" fn handle_event(
    instance: *mut c_void,
    event_id: u64,
    event: abi::AbiEvent,
    done: abi::CompletionFn,
    user_data: *mut c_void,
) {
    let instance = unsafe { &*(instance as *const Instance) };
    instance
        .inner
        .bridge
        .handle_event(event_id, event, done, user_data);
}

extern "C" fn cancel_event(instance: *mut c_void, event_id: u64) {
    let instance = unsafe { &*(instance as *const Instance) };
    instance.inner.bridge.cancel_event(event_id);
}

extern "C" fn shutdown(
    instance: *mut c_void,
    force: u8,
    done: abi::CompletionFn,
    user_data: *mut c_void,
) {
    let instance = unsafe { &*(instance as *const Instance) };
    let inner = instance.inner.clone();
    let completion = CompletionTarget {
        done,
        user_data: SendUserData(user_data),
    };
    if force != 0 {
        inner.cancel.cancel();
    }
    inner.bridge.close(force != 0);

    let mut shutdown = inner.shutdown.lock().expect("shutdown lock");
    if let Some(outcome) = &shutdown.outcome {
        completion.finish_outcome(outcome);
        return;
    }
    shutdown.waiters.push(completion);
    if shutdown.started {
        return;
    }
    shutdown.started = true;
    drop(shutdown);

    thread::spawn(move || {
        let outcome = inner
            .join
            .lock()
            .expect("instance join lock")
            .take()
            .map(|join| {
                join.join()
                    .map_err(|_| "agent-os actor thread panicked".to_owned())
            })
            .transpose()
            .map(|_| ());
        inner.bridge.finish_shutdown();
        let waiters = {
            let mut shutdown = inner.shutdown.lock().expect("shutdown lock");
            shutdown.outcome = Some(outcome.clone());
            std::mem::take(&mut shutdown.waiters)
        };
        for waiter in waiters {
            waiter.finish_outcome(&outcome);
        }
    });
}

extern "C" fn instance_free(instance: *mut c_void) {
    if !instance.is_null() {
        unsafe { drop(Box::from_raw(instance as *mut Instance)) };
    }
}

extern "C" fn factory_free(factory: *mut c_void) {
    if !factory.is_null() {
        unsafe { drop(Box::from_raw(factory as *mut Factory)) };
    }
}

extern "C" fn plugin_shutdown(plugin: *mut c_void) {
    if !plugin.is_null() {
        unsafe { drop(Box::from_raw(plugin as *mut Plugin)) };
    }
}

static PLUGIN_API: abi::PluginApi = abi::PluginApi::new(
    plugin_init,
    factory_new,
    factory_free,
    instance_new,
    handle_event,
    cancel_event,
    shutdown,
    instance_free,
    plugin_shutdown,
);

/// AgentOS exports one process-lifetime descriptor. RivetKit validates and
/// copies this table while retaining the loaded library for process lifetime.
#[no_mangle]
pub extern "C" fn rivet_actor_plugin_api() -> *const abi::PluginApi {
    &PLUGIN_API
}
