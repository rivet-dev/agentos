//! The agentOS host imports: Preview1 plus the `host_*` POSIX extensions.
//!
//! Engines bind every permitted ABI binding to [`dispatch`], and validate a
//! module's imports with [`validate_imports`] before instantiating it.

use crate::abi::{
    binding, core_signature, AbiBinding, CoreValueType, ImportId, PermissionTier, Restartability,
    ABI_BINDINGS, ALIAS_BINDINGS,
};
use crate::backend::{HostCallReply, HostServiceError};
use crate::host::{ExecImportTiers, HostOperation, SignalMaskHow, SignalOperation, SignalSetValue};
use crate::memory;
use crate::{Val, WasmGuest, WasmHostClient};
use agentos_executor_wasm_abi::WasmPermissionTier;
use serde_json::Value;

mod filesystem;
mod network;
mod preview1;
mod process;
mod terminal;
mod user;

const WASI_ERRNO_SUCCESS: i32 = 0;
const WASI_ERRNO_FAULT: i32 = 21;
const WASI_ERRNO_NOSYS: i32 = 52;
const WASI_ERRNO_INTR: i32 = 27;
const SA_RESTART: u32 = 0x1000_0000;
const MAX_SIGNALS_PER_SAFE_POINT: usize = 64;

/// One import a module declares, described without an engine.
#[derive(Clone, Debug)]
pub struct ImportDesc<'a> {
    pub module: &'a str,
    pub name: &'a str,
    pub kind: ImportKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportKind {
    Func {
        params: Vec<CoreValueType>,
        results: Vec<CoreValueType>,
    },
    Memory {
        shared: bool,
        memory64: bool,
    },
    /// A table, global, or tag import. None is part of the agentOS ABI.
    Other,
}

/// Every ABI binding permitted at `tier`, with the import module name it is
/// linked under (aliases link the same binding under a second module name).
pub fn permitted_bindings(
    tier: WasmPermissionTier,
) -> impl Iterator<Item = (&'static str, AbiBinding)> {
    ABI_BINDINGS
        .iter()
        .copied()
        .filter(move |abi| permitted(*abi, tier))
        .map(|abi| (abi.module, abi))
        .chain(ALIAS_BINDINGS.iter().filter_map(move |alias| {
            let abi = *binding(alias.import);
            (permitted(abi, tier) && alias.permission_tiers.contains(permission_tier(tier)))
                .then_some((alias.alias_module, abi))
        }))
}

/// Reject unsupported or permission-filtered imports before engine linker
/// diagnostics are involved. This keeps the public outcome independent of an
/// engine's error-string format while the generated ABI registry remains the
/// sole import allowlist.
pub fn validate_imports<'a>(
    imports: impl IntoIterator<Item = ImportDesc<'a>>,
    tier: WasmPermissionTier,
    threaded: bool,
) -> Result<(), HostServiceError> {
    for import in imports {
        if import_permitted(import.module, import.name, tier)
            || (threaded && is_thread_runtime_import(import.module, import.name))
        {
            if import_type_supported(&import) {
                continue;
            }
            return Err(HostServiceError::new(
                "ERR_AGENTOS_WASM_IMPORT_TYPE",
                format!(
                    "incompatible WebAssembly host import type {}.{}",
                    import.module, import.name
                ),
            ));
        }
        return Err(HostServiceError::new(
            "ERR_AGENTOS_WASM_UNSUPPORTED_IMPORT",
            format!(
                "unsupported WebAssembly host import {}.{}",
                import.module, import.name
            ),
        )
        .with_details(serde_json::json!({
            "module": import.module,
            "name": import.name,
        })));
    }
    Ok(())
}

/// Compile-time compatibility of one replacement image at each ABI tier. Exec
/// may narrow authority, and the sidecar chooses the tier at the commit point.
pub fn exec_import_tiers<'a>(
    imports: &[ImportDesc<'a>],
    threaded: bool,
) -> Result<ExecImportTiers, HostServiceError> {
    let valid = |tier| validate_imports(imports.iter().cloned(), tier, threaded).is_ok();
    let tiers = ExecImportTiers {
        isolated: valid(WasmPermissionTier::Isolated),
        read_only: valid(WasmPermissionTier::ReadOnly),
        read_write: valid(WasmPermissionTier::ReadWrite),
        full: valid(WasmPermissionTier::Full),
    };
    if !(tiers.isolated || tiers.read_only || tiers.read_write || tiers.full) {
        return Err(HostServiceError::new(
            "ENOEXEC",
            "replacement has unsupported imports",
        ));
    }
    Ok(tiers)
}

// Name allowlisting does not establish linkability. Validate the exact ABI
// signature before exec can commit destructive changes to the old image.
fn import_type_supported(import: &ImportDesc<'_>) -> bool {
    if (import.module, import.name) == ("env", "memory") {
        return matches!(
            import.kind,
            ImportKind::Memory {
                shared: true,
                memory64: false
            }
        );
    }
    let ImportKind::Func {
        params: actual_params,
        results: actual_results,
    } = &import.kind
    else {
        return false;
    };
    let abi = ABI_BINDINGS
        .iter()
        .copied()
        .find(|abi| abi.module == import.module && abi.name == import.name)
        .or_else(|| {
            ALIAS_BINDINGS.iter().find_map(|alias| {
                let abi = *binding(alias.import);
                (alias.alias_module == import.module && abi.name == import.name).then_some(abi)
            })
        });
    let (params, results): (&[CoreValueType], &[CoreValueType]) = if let Some(abi) = abi {
        let signature = core_signature(abi.signature);
        (signature.params, signature.results)
    } else if (import.module, import.name) == ("wasi", "thread-spawn") {
        (&[CoreValueType::I32], &[CoreValueType::I32])
    } else {
        return false;
    };
    actual_params.as_slice() == params && actual_results.as_slice() == results
}

fn is_thread_runtime_import(module: &str, name: &str) -> bool {
    matches!((module, name), ("env", "memory") | ("wasi", "thread-spawn"))
}

fn import_permitted(module: &str, name: &str, tier: WasmPermissionTier) -> bool {
    ABI_BINDINGS
        .iter()
        .any(|abi| abi.module == module && abi.name == name && permitted(*abi, tier))
        || ALIAS_BINDINGS.iter().any(|alias| {
            let abi = *binding(alias.import);
            alias.alias_module == module
                && abi.name == name
                && permitted(abi, tier)
                && alias.permission_tiers.contains(permission_tier(tier))
        })
}

pub fn permitted(abi: AbiBinding, tier: WasmPermissionTier) -> bool {
    abi.permission_tiers.contains(permission_tier(tier))
}

pub fn permission_tier(tier: WasmPermissionTier) -> PermissionTier {
    match tier {
        WasmPermissionTier::Isolated => PermissionTier::Isolated,
        WasmPermissionTier::ReadOnly => PermissionTier::ReadOnly,
        WasmPermissionTier::ReadWrite => PermissionTier::ReadWrite,
        WasmPermissionTier::Full => PermissionTier::Full,
    }
}

/// The result slots for one binding, sized and typed from its ABI signature.
pub fn result_slots(abi: AbiBinding) -> Vec<Val> {
    core_signature(abi.signature)
        .results
        .iter()
        .copied()
        .map(Val::zero)
        .collect()
}

/// Run one host import: deliver pending signals, run the handler, and
/// restart it when a restartable signal interrupted it.
pub async fn dispatch<C: WasmGuest>(
    caller: &mut C,
    abi: AbiBinding,
    params: &[Val],
    results: &mut [Val],
) -> crate::Result<()> {
    if caller.data().canceled() {
        return Err(crate::format_err!(
            "ERR_AGENTOS_WASMTIME_CANCELED: execution canceled"
        ));
    }
    drain_signal_checkpoints(caller, false).await?;
    loop {
        dispatch_once(caller, abi, params, results).await?;
        let interrupted = matches!(results, [Val::I32(value)] if *value == WASI_ERRNO_INTR);
        // EINTR is itself an authoritative signal wake. In the threaded worker
        // topology its reply can become runnable just before the coalesced
        // SignalWake frame updates the local fast-path counter, so probe the
        // parent-owned checkpoint once unconditionally.
        let signals = drain_signal_checkpoints(caller, interrupted).await?;
        if interrupted
            && abi.restartability == Restartability::SignalRestartable
            && signals.delivered
            && signals.all_restart
        {
            continue;
        }
        return Ok(());
    }
}

async fn dispatch_once<C: WasmGuest>(
    caller: &mut C,
    abi: AbiBinding,
    params: &[Val],
    results: &mut [Val],
) -> crate::Result<()> {
    if preview1::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    if user::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    if terminal::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    if filesystem::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    if network::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    if process::dispatch(caller, abi, params, results).await? {
        return Ok(());
    }
    match abi.id {
        ImportId::WasiSnapshotPreview1ArgsSizesGet => {
            let count = caller.data().argv.len();
            let bytes = table_bytes(&caller.data().argv)?;
            let count =
                u32::try_from(count).map_err(|_| crate::format_err!("argv count overflow"))?;
            let bytes =
                u32::try_from(bytes).map_err(|_| crate::format_err!("argv bytes overflow"))?;
            let status = if memory::validate_range(caller, i32_arg(params, 0)?, 4).is_err()
                || memory::validate_range(caller, i32_arg(params, 1)?, 4).is_err()
            {
                WASI_ERRNO_FAULT
            } else {
                memory::write_u32(caller, i32_arg(params, 0)?, count).expect("prevalidated argc");
                memory::write_u32(caller, i32_arg(params, 1)?, bytes)
                    .expect("prevalidated argv bytes");
                WASI_ERRNO_SUCCESS
            };
            set_i32_result(results, status)?;
        }
        ImportId::WasiSnapshotPreview1ArgsGet => {
            let values = caller.data().argv.clone();
            let status = if memory::write_string_table(
                caller,
                i32_arg(params, 0)?,
                i32_arg(params, 1)?,
                &values,
            )
            .is_ok()
            {
                WASI_ERRNO_SUCCESS
            } else {
                WASI_ERRNO_FAULT
            };
            set_i32_result(results, status)?;
        }
        ImportId::WasiSnapshotPreview1EnvironSizesGet => {
            let count = caller.data().env.len();
            let bytes = table_bytes(&caller.data().env)?;
            let count =
                u32::try_from(count).map_err(|_| crate::format_err!("env count overflow"))?;
            let bytes =
                u32::try_from(bytes).map_err(|_| crate::format_err!("env bytes overflow"))?;
            let status = if memory::validate_range(caller, i32_arg(params, 0)?, 4).is_err()
                || memory::validate_range(caller, i32_arg(params, 1)?, 4).is_err()
            {
                WASI_ERRNO_FAULT
            } else {
                memory::write_u32(caller, i32_arg(params, 0)?, count)
                    .expect("prevalidated env count");
                memory::write_u32(caller, i32_arg(params, 1)?, bytes)
                    .expect("prevalidated env bytes");
                WASI_ERRNO_SUCCESS
            };
            set_i32_result(results, status)?;
        }
        ImportId::WasiSnapshotPreview1EnvironGet => {
            let values = caller.data().env.clone();
            let status = if memory::write_string_table(
                caller,
                i32_arg(params, 0)?,
                i32_arg(params, 1)?,
                &values,
            )
            .is_ok()
            {
                WASI_ERRNO_SUCCESS
            } else {
                WASI_ERRNO_FAULT
            };
            set_i32_result(results, status)?;
        }
        ImportId::WasiSnapshotPreview1SchedYield => set_i32_result(results, WASI_ERRNO_SUCCESS)?,
        ImportId::WasiSnapshotPreview1ProcExit => {
            let code = i32_arg(params, 0)? as i32;
            if let Some(group) = caller.data().thread_group.as_ref() {
                group
                    .request_process_exit(code)
                    .map_err(wasmtime_host_error)?;
            }
            caller.data_mut().exit_code = Some(code);
            return Err(crate::Error::Exit(code));
        }
        _ => set_default_result(results)?,
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SignalDispatch {
    delivered: bool,
    all_restart: bool,
}

async fn drain_signal_checkpoints<C: WasmGuest>(
    caller: &mut C,
    mut force_probe: bool,
) -> crate::Result<SignalDispatch> {
    let mut outcome = SignalDispatch {
        delivered: false,
        all_restart: true,
    };
    for _ in 0..MAX_SIGNALS_PER_SAFE_POINT {
        let host = caller.data().host.clone();
        if !force_probe && !host.signal_pending() {
            return Ok(outcome);
        }
        force_probe = false;
        let thread_id = u32::try_from(caller.data().thread_id).map_err(|_| {
            crate::format_err!("ERR_AGENTOS_WASMTIME_SIGNAL_THREAD_ID: invalid thread id")
        })?;
        let take = if caller.data().thread_group.is_some() {
            SignalOperation::TakePublishedDeliveryForThread { thread_id }
        } else {
            SignalOperation::TakePublishedDelivery
        };
        let reply = host
            .submit(HostOperation::Signal(take), std::mem::size_of::<u32>())
            .await
            .map_err(wasmtime_host_error)?;
        let value = host_json(reply, "process.take_signal")?;
        if value.is_null() {
            return Ok(outcome);
        }
        let signal = value
            .get("signal")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| crate::format_err!("process.take_signal omitted a valid signal"))?;
        let token = value
            .get("token")
            .and_then(Value::as_u64)
            .ok_or_else(|| crate::format_err!("process.take_signal omitted a delivery token"))?;
        let flags = value
            .get("flags")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| crate::format_err!("process.take_signal omitted valid flags"))?;
        outcome.delivered = true;
        outcome.all_restart &= flags & SA_RESTART != 0;

        let handler_result = caller.call_signal_trampoline(signal).await;
        let end = if caller.data().thread_group.is_some() {
            SignalOperation::EndDeliveryForThread { thread_id, token }
        } else {
            SignalOperation::EndDelivery { token }
        };
        let end_result = caller
            .data()
            .host
            .clone()
            .submit(
                HostOperation::Signal(end),
                std::mem::size_of::<u64>() + std::mem::size_of::<u32>(),
            )
            .await;
        if let Err(error) = handler_result {
            if let Err(end_error) = end_result {
                eprintln!(
                    "ERR_AGENTOS_WASMTIME_SIGNAL_SETTLEMENT: handler failed with {error}; token settlement also failed with {end_error}"
                );
            }
            return Err(error);
        }
        end_result.map_err(wasmtime_host_error)?;
    }
    Err(crate::format_err!(
        "ERR_AGENTOS_WASMTIME_SIGNAL_DRAIN_LIMIT: more than {MAX_SIGNALS_PER_SAFE_POINT} signals were delivered at one safe point"
    ))
}

/// The signal mask a new image inherits, as the `(low, high)` words the
/// guest's `__agentos_set_initial_sigmask` export takes, or `None` when no
/// signal is blocked. The engine calls that export when this returns `Some`.
pub async fn inherited_signal_mask(
    host: &WasmHostClient,
    thread_id: i32,
    in_thread_group: bool,
) -> Result<Option<(i32, i32)>, HostServiceError> {
    let thread_id = u32::try_from(thread_id).map_err(|_| {
        HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_SIGNAL_THREAD_ID",
            "invalid Wasmtime signal thread id",
        )
    })?;
    let update = if in_thread_group {
        SignalOperation::UpdateMaskForThread {
            thread_id,
            how: SignalMaskHow::Block,
            set: SignalSetValue::default(),
        }
    } else {
        SignalOperation::UpdateMask {
            how: SignalMaskHow::Block,
            set: SignalSetValue::default(),
        }
    };
    let reply = host
        .clone()
        .submit(
            HostOperation::Signal(update),
            std::mem::size_of::<SignalSetValue>(),
        )
        .await?;
    let value = match reply {
        HostCallReply::Json(value) => value,
        _ => {
            return Err(HostServiceError::new(
                "EIO",
                "process.signal_mask returned a non-JSON reply",
            ));
        }
    };
    let signals = value
        .get("signals")
        .and_then(Value::as_array)
        .ok_or_else(|| HostServiceError::new("EIO", "signal-mask query omitted signals"))?;
    let mut low = 0u32;
    let mut high = 0u32;
    for signal in signals {
        let signal = signal
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| (1..=64).contains(value))
            .ok_or_else(|| {
                HostServiceError::new("EIO", "signal-mask query returned an invalid signal")
            })?;
        if signal <= 32 {
            low |= 1 << (signal - 1);
        } else {
            high |= 1 << (signal - 33);
        }
    }
    if low == 0 && high == 0 {
        return Ok(None);
    }
    Ok(Some((low as i32, high as i32)))
}

fn host_json(reply: HostCallReply, method: &str) -> crate::Result<Value> {
    match reply {
        HostCallReply::Json(value) => Ok(value),
        _ => Err(crate::format_err!("{method} returned a non-JSON reply")),
    }
}

fn wasmtime_host_error(error: HostServiceError) -> crate::Error {
    crate::Error::Host(error)
}

fn table_bytes(values: &[Vec<u8>]) -> crate::Result<usize> {
    values.iter().try_fold(0usize, |total, value| {
        total
            .checked_add(value.len())
            .ok_or_else(|| crate::format_err!("string table byte count overflow"))
    })
}

fn i32_arg(params: &[Val], index: usize) -> crate::Result<u32> {
    match params.get(index) {
        Some(Val::I32(value)) => Ok(*value as u32),
        _ => Err(crate::format_err!(
            "invalid i32 ABI argument at index {index}"
        )),
    }
}

fn set_i32_result(results: &mut [Val], value: i32) -> crate::Result<()> {
    match results {
        [slot] => {
            *slot = Val::I32(value);
            Ok(())
        }
        _ => Err(crate::format_err!("invalid i32 ABI result shape")),
    }
}

fn set_default_result(results: &mut [Val]) -> crate::Result<()> {
    match results {
        [] => Ok(()),
        [slot @ Val::I32(_)] => {
            *slot = Val::I32(WASI_ERRNO_NOSYS);
            Ok(())
        }
        [slot @ Val::I64(_)] => {
            *slot = Val::I64(-1);
            Ok(())
        }
        _ => Err(crate::format_err!("unsupported AgentOS ABI result shape")),
    }
}

async fn check_fixed_request_limit<C: WasmGuest>(
    caller: &mut C,
    limit_name: &'static str,
    observed: usize,
    maximum: usize,
    errno: i32,
) -> i32 {
    let warning_at = maximum.saturating_sub(maximum / 5).max(1);
    let publish =
        observed >= warning_at && caller.data_mut().warned_fixed_limits.insert(limit_name);
    if publish {
        let warning = format!(
            "[agentos] WASM request is near {limit_name} ({observed}/{maximum}); split the request if needed\n"
        );
        let host = caller.data().host.clone();
        if let Err(error) = host.publish_stderr(warning.into_bytes()).await {
            eprintln!(
                "ERR_AGENTOS_WASMTIME_LIMIT_WARNING: failed to publish {limit_name} warning: {error}"
            );
        }
    }
    if observed > maximum {
        errno
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_filter_matches_generated_registry() {
        for tier in [
            WasmPermissionTier::Isolated,
            WasmPermissionTier::ReadOnly,
            WasmPermissionTier::ReadWrite,
            WasmPermissionTier::Full,
        ] {
            assert_eq!(
                ABI_BINDINGS
                    .iter()
                    .filter(|abi| permitted(**abi, tier))
                    .count(),
                ABI_BINDINGS
                    .iter()
                    .filter(|abi| abi.permission_tiers.contains(permission_tier(tier)))
                    .count()
            );
        }
        assert_eq!(
            permission_tier(WasmPermissionTier::Full),
            PermissionTier::Full
        );
    }
}
