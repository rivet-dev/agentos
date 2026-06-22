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
    let mut vm: Option<agentos_client::AgentOs> = None;
    let mut vars = actions::Vars::default();
    // Ensure the agent-os schema exists before handling events (best-effort;
    // mirrors rivetkit-agent-os run.rs).
    if host.sql_is_enabled() {
        let _ = persistence::migrate(&host).await;
    }
    // Signal readiness: the native-plugin factory uses manual startup-ready, so
    // the host's `start()` caller blocks until we report this. The VM is brought
    // up lazily on the first action, so the actor is ready once the schema is in
    // place and the event loop is about to run.
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
                        actions::dispatch(&host, vm_ref, &mut vars, &name, &action_args, token)
                            .await;
                    }
                    Err(_) => {
                        let _ = host.reply_err(token, "malformed action event payload");
                    }
                }
            }
            Some(abi::AbiEventTag::Http) => {
                // Preview proxy: do NOT bring the VM up for HTTP (matches r6's
                // run loop, which passes `vm.as_ref()`); no VM → 404.
                let response = http::proxy_preview(&host, vm.as_ref(), &payload).await;
                let _ = host.reply_ok(token, response);
            }
            Some(abi::AbiEventTag::ConnPreflight) => {
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::ConnOpen) => {
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::Sleep) => {
                vars.clear();
                vm::shutdown_vm(&host, &mut vm, "sleep").await;
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(abi::AbiEventTag::Destroy) => {
                vars.clear();
                vm::shutdown_vm(&host, &mut vm, "destroy").await;
                let _ = host.reply_ok(token, Vec::new());
            }
            Some(t) if t.needs_reply() => {
                let _ = host.reply_err(token, "event not supported by agent-os actor");
            }
            _ => {}
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
