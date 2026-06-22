//! VM lifecycle + durable-storage bridge — ported from `rivetkit-agent-os`'s
//! `run.rs`/`persistence.rs`, with `HostCtx` substituted for rivetkit's `Ctx`.
//!
//! `ensure_vm` brings up the `AgentOs` VM lazily with a `js_bridge` root whose
//! filesystem callback routes guest fs ops to actor durable storage via
//! `HostCtx.db_*` (spec §6.3 hot path). `agent-os-client` is imported unmodified.

use std::sync::Arc;

use agentos_client::{
    AgentOs, AgentOsConfig, MountPlugin, RootFilesystemConfig, RootFilesystemKind,
    SidecarJsBridgeCall, SidecarJsBridgeCallback,
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
    *vm = Some(handle);
    let _ = host.broadcast(b"vmBooted".to_vec(), b"{}".to_vec());
    Ok(())
}

/// Tear down the VM if running; broadcast `vmShutdown` afterward.
pub(crate) async fn shutdown_vm(host: &HostCtx, vm: &mut Option<AgentOs>, reason: &str) {
    let Some(handle) = vm.take() else {
        return;
    };
    if let Err(error) = handle.shutdown().await {
        host.log_warn(&format!("agent-os vm shutdown error ({reason}): {error}"));
    }
    let payload = format!("{{\"reason\":\"{reason}\"}}");
    let _ = host.broadcast(b"vmShutdown".to_vec(), payload.into_bytes());
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
