//! VM lifecycle + durable-storage bridge — ported from `rivetkit-agent-os`'s
//! `run.rs`/`persistence.rs`, with `HostCtx` substituted for rivetkit's `Ctx`.
//!
//! `ensure_vm` brings up the `AgentOs` VM lazily with a `js_bridge` root whose
//! filesystem callback routes guest fs ops to actor durable storage via
//! `HostCtx.db_*` (spec §6.3 hot path). `agentos-client` is imported unmodified.

use std::sync::Arc;

use agentos_client::{
    AgentOs, AgentOsConfig, CronAlarmHandler, MountPlugin, RootFilesystemConfig,
    RootFilesystemKind, SidecarJsBridgeCall, SidecarJsBridgeCallback,
};
use serde_json::{json, Value};

use crate::config::AgentOsConfigJson;
use crate::host_ctx::HostCtx;

/// Build the `AgentOsConfig` from the client-supplied `config` and overlay the
/// actor-DB `js_bridge` root + the durable-storage callback bound to `host`.
///
/// Mirrors r6's `run::configure_actor_db_root`: the actor-DB root and the
/// storage callback are overlaid **only** when the client did not configure
/// them, so an explicit client root/callback is honored. `pool` is the
/// per-plugin-runtime sidecar pool (spec §7).
pub(crate) fn build_config(
    sidecar_path: &str,
    host: HostCtx,
    config: &AgentOsConfigJson,
    pool: &str,
) -> AgentOsConfig {
    let mut options = config.to_agent_os_config(pool);

    // Overlay the actor-DB callback root only when the client left it default.
    if options.root_filesystem == RootFilesystemConfig::default() {
        options.root_filesystem = RootFilesystemConfig {
            kind: RootFilesystemKind::Native,
            native_plugin: Some(MountPlugin {
                id: "js_bridge".to_owned(),
                config: Some(json!({
                    "mountId": "agentos-actor-root",
                })),
            }),
            ..RootFilesystemConfig::default()
        };
    }

    // Overlay the durable-storage callback only when none was configured.
    if options.sidecar_js_bridge_callback.is_none() {
        let storage_host = host;
        let callback: SidecarJsBridgeCallback = Arc::new(move |call: SidecarJsBridgeCall| {
            let host = storage_host.clone();
            Box::pin(async move { handle_storage(&host, call).await })
        });
        options.sidecar_js_bridge_callback = Some(callback);
    }

    options.sidecar_binary_path = Some(sidecar_path.to_owned());
    options
}

/// Bring up the VM if not already running; broadcast `vmBooted` on success.
pub(crate) async fn ensure_vm(
    host: &HostCtx,
    sidecar_path: &str,
    config: &AgentOsConfigJson,
    pool: &str,
    vm: &mut Option<AgentOs>,
) -> Result<(), String> {
    if vm.is_some() {
        return Ok(());
    }
    let config = build_config(sidecar_path, host.clone(), config, pool);
    let handle = AgentOs::create(config)
        .await
        .map_err(|error| format!("agent-os vm bring-up failed: {error}"))?;
    let alarm_host = host.clone();
    let alarm_handler: CronAlarmHandler = Arc::new(move |alarm| {
        let host = alarm_host.clone();
        Box::pin(async move {
            let Some(timestamp_ms) = alarm.next_alarm_ms else {
                return Ok(());
            };
            let timestamp_ms = i64::try_from(timestamp_ms)
                .map_err(|_| "cron alarm exceeds the actor timestamp range".to_string())?;
            let args =
                rivet_actor_plugin_abi::codec::encode_json_compat_to_vec(&(alarm.generation,))
                    .map_err(|error| format!("encode cron wake action: {error}"))?;
            host.schedule_at(
                timestamp_ms,
                crate::actions::cron::INTERNAL_CRON_WAKE_ACTION.to_string(),
                args,
            )
            .await
        })
    });
    handle.set_cron_alarm_handler(alarm_handler);
    if host.sql_is_enabled() {
        let stored_state = match crate::persistence::load_cron_state(host).await {
            Ok(state) => state,
            Err(error) => {
                if let Err(shutdown_error) = handle.shutdown().await {
                    host.log_warn(&format!(
                        "agent-os vm shutdown after cron-state load failure: {shutdown_error}"
                    ));
                }
                return Err(format!("load persisted cron state: {error}"));
            }
        };
        if let Some(state) = stored_state {
            if let Err(error) = handle.import_cron_state(state).await {
                if let Err(shutdown_error) = handle.shutdown().await {
                    host.log_warn(&format!(
                        "agent-os vm shutdown after cron restore failure: {shutdown_error}"
                    ));
                }
                return Err(format!("restore persisted cron state: {error}"));
            }
        }
    }
    *vm = Some(handle);
    // CBOR array of handler args (`handler(...body)`): the listener takes one
    // object argument, so the body is `[{}]`. JSON bytes / a bare object trip the
    // client's CBOR decoder ("length over 4294967295" / "Spread requires iterable").
    match encode_vm_booted_event() {
        Ok(cbor) => {
            let _ = host.broadcast(b"vmBooted".to_vec(), cbor);
        }
        Err(error) => {
            tracing::warn!(?error, "failed to encode vmBooted broadcast");
        }
    }
    Ok(())
}

/// Capture the scheduler in its sidecar-owned opaque format. The actor stores
/// the bytes but never interprets schedule or run state.
pub(crate) async fn persist_cron_state(host: &HostCtx, vm: &AgentOs) -> Result<(), String> {
    if !host.sql_is_enabled() {
        return Ok(());
    }
    let state = vm
        .export_cron_state()
        .await
        .map_err(|error| format!("export sidecar cron state: {error}"))?;
    crate::persistence::save_cron_state(host, &state)
        .await
        .map_err(|error| format!("persist sidecar cron state: {error}"))
}

pub(crate) async fn delete_cron_state(host: &HostCtx) -> Result<(), String> {
    if !host.sql_is_enabled() {
        return Ok(());
    }
    crate::persistence::delete_cron_state(host)
        .await
        .map_err(|error| format!("delete persisted cron state: {error}"))
}

/// Tear down the VM if running; broadcast `vmShutdown` afterward.
pub(crate) async fn shutdown_vm(host: &HostCtx, vm: &mut Option<AgentOs>, reason: &str) {
    let Some(handle) = vm.take() else {
        return;
    };
    if let Err(error) = handle.shutdown().await {
        host.log_warn(&format!("agent-os vm shutdown error ({reason}): {error}"));
    }
    match encode_vm_shutdown_event(reason) {
        Ok(cbor) => {
            let _ = host.broadcast(b"vmShutdown".to_vec(), cbor);
        }
        Err(error) => {
            tracing::warn!(?error, "failed to encode vmShutdown broadcast");
        }
    }
}

pub(crate) fn encode_vm_booted_event() -> anyhow::Result<Vec<u8>> {
    crate::actions::encode_event_arg(&json!({}))
}

pub(crate) fn encode_vm_shutdown_event(reason: &str) -> anyhow::Result<Vec<u8>> {
    crate::actions::encode_event_arg(&json!({ "reason": reason }))
}

/// Durable-storage callback: routes a sidecar fs op to actor SQLite via
/// `HostCtx.db_*`. This exercises the storage bridge end-to-end; the full
/// op set (~24 ops in `persistence.rs`) is ported on top of this.
async fn handle_storage(
    host: &HostCtx,
    call: SidecarJsBridgeCall,
) -> Result<Option<Value>, String> {
    crate::persistence::handle_fs_call(host, &call.operation, &call.args)
        .await
        .map_err(|error| error.to_string())
}
