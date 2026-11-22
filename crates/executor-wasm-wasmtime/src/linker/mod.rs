//! Wasmtime binding of the engine-neutral agentOS host imports.
//!
//! The import handlers live in `agentos-executor-wasm-host` and are shared
//! with the V8 executor. This module links them into a Wasmtime `Linker` and
//! supplies what they need from Wasmtime: guest memory, the signal
//! trampoline, fuel, and exec preflight compilation.

use super::memory;
use super::module;
use super::store::{HostState, WasmtimeStoreState};
use crate::backend::HostServiceError;
use crate::host::ExecImportTiers;
use agentos_executor_wasm_abi::WasmPermissionTier;
use agentos_executor_wasm_host::abi::{core_signature, AbiBinding, CoreValueType};
use agentos_executor_wasm_host::imports::{self, ImportDesc, ImportKind};
use agentos_executor_wasm_host::memory::GuestMemoryError;
use agentos_executor_wasm_host::{BoxFuture, Val, WasmGuest};
use std::ops::Range;
use std::sync::Arc;
use wasmtime::{Caller, Engine, Extern, FuncType, Linker, Module, ValType};

pub fn build_linker(
    engine: &Engine,
    tier: WasmPermissionTier,
    threaded: bool,
) -> wasmtime::Result<Linker<WasmtimeStoreState>> {
    let mut linker = Linker::new(engine);
    for (module, abi) in imports::permitted_bindings(tier) {
        link_binding(&mut linker, engine, abi, module)?;
    }
    if threaded {
        linker.func_wrap(
            "wasi",
            "thread-spawn",
            |caller: Caller<'_, WasmtimeStoreState>, start_arg: i32| -> i32 {
                caller
                    .data()
                    .thread_group
                    .as_ref()
                    .map_or(-1, |group| group.spawn(start_arg))
            },
        )?;
    }
    Ok(linker)
}

/// Reject unsupported or permission-filtered imports before Wasmtime linker
/// diagnostics are involved, using the shared generated-registry check.
pub fn validate_module_imports(
    module: &Module,
    tier: WasmPermissionTier,
    threaded: bool,
) -> Result<(), HostServiceError> {
    imports::validate_imports(import_descs(module), tier, threaded)
}

/// Compile-time compatibility of one replacement image at each ABI tier.
fn exec_import_tiers(module: &Module, threaded: bool) -> Result<ExecImportTiers, HostServiceError> {
    imports::exec_import_tiers(&import_descs(module), threaded)
}

fn import_descs(module: &Module) -> Vec<ImportDesc<'_>> {
    module
        .imports()
        .map(|import| ImportDesc {
            module: import.module(),
            name: import.name(),
            kind: match import.ty() {
                wasmtime::ExternType::Func(ty) => {
                    let params = ty.params().map(core_type).collect::<Option<Vec<_>>>();
                    let results = ty.results().map(core_type).collect::<Option<Vec<_>>>();
                    match (params, results) {
                        (Some(params), Some(results)) => ImportKind::Func { params, results },
                        _ => ImportKind::Other,
                    }
                }
                wasmtime::ExternType::Memory(memory) => ImportKind::Memory {
                    shared: memory.is_shared(),
                    memory64: memory.is_64(),
                },
                _ => ImportKind::Other,
            },
        })
        .collect()
}

fn core_type(value: ValType) -> Option<CoreValueType> {
    match value {
        ValType::I32 => Some(CoreValueType::I32),
        ValType::I64 => Some(CoreValueType::I64),
        _ => None,
    }
}

fn value_type(value: CoreValueType) -> ValType {
    match value {
        CoreValueType::I32 => ValType::I32,
        CoreValueType::I64 => ValType::I64,
    }
}

fn link_binding(
    linker: &mut Linker<WasmtimeStoreState>,
    engine: &Engine,
    abi: AbiBinding,
    module: &'static str,
) -> wasmtime::Result<()> {
    let signature = core_signature(abi.signature);
    let ty = FuncType::new(
        engine,
        signature.params.iter().copied().map(value_type),
        signature.results.iter().copied().map(value_type),
    );
    linker.func_new_async(module, abi.name, ty, move |mut caller, params, results| {
        Box::new(async move {
            let params = params
                .iter()
                .enumerate()
                .map(|(index, value)| match value {
                    wasmtime::Val::I32(value) => Ok(Val::I32(*value)),
                    wasmtime::Val::I64(value) => Ok(Val::I64(*value)),
                    _ => Err(wasmtime::format_err!(
                        "invalid AgentOS ABI argument at index {index}"
                    )),
                })
                .collect::<wasmtime::Result<Vec<_>>>()?;
            let mut host_results = imports::result_slots(abi);
            let mut guest = WasmtimeGuest {
                caller: &mut caller,
            };
            imports::dispatch(&mut guest, abi, &params, &mut host_results)
                .await
                .map_err(into_wasmtime_error)?;
            if host_results.len() != results.len() {
                return Err(wasmtime::format_err!("unsupported AgentOS ABI result shape"));
            }
            for (slot, value) in results.iter_mut().zip(host_results) {
                *slot = match value {
                    Val::I32(value) => wasmtime::Val::I32(value),
                    Val::I64(value) => wasmtime::Val::I64(value),
                };
            }
            Ok(())
        })
    })?;
    Ok(())
}

/// Map a shared-handler error back to the Wasmtime error the lifecycle and
/// `error::normalize` expect. Unwinding sentinels keep their exact text.
fn into_wasmtime_error(error: agentos_executor_wasm_host::Error) -> wasmtime::Error {
    use agentos_executor_wasm_host::Error;
    match error {
        Error::Exit(code) => wasmtime::format_err!("agentos:wasi-exit:{code}"),
        Error::ExecReplaced => wasmtime::format_err!("agentos:exec-replaced"),
        Error::Host(error) => wasmtime::Error::new(error),
        Error::Message(message) => wasmtime::format_err!("{message}"),
        Error::Engine(error) => match error.downcast::<wasmtime::Error>() {
            Ok(error) => *error,
            Err(_) => wasmtime::format_err!("guest call failed"),
        },
    }
}

/// The calling Wasmtime guest, as the shared handlers see it.
struct WasmtimeGuest<'a, 'b> {
    caller: &'a mut Caller<'b, WasmtimeStoreState>,
}

impl WasmGuest for WasmtimeGuest<'_, '_> {
    type Module = Arc<Module>;

    fn data(&self) -> &HostState {
        self.caller.data()
    }

    fn data_mut(&mut self) -> &mut HostState {
        self.caller.data_mut()
    }

    fn memory_size(&mut self) -> Result<usize, GuestMemoryError> {
        let memory = memory::exported_memory(self.caller)?;
        Ok(memory.data_size(self.caller))
    }

    fn memory_read(&mut self, range: Range<usize>) -> Result<Vec<u8>, GuestMemoryError> {
        let memory = memory::exported_memory(self.caller)?;
        Ok(memory.read(self.caller, range))
    }

    fn memory_write(&mut self, range: Range<usize>, bytes: &[u8]) -> Result<(), GuestMemoryError> {
        let memory = memory::exported_memory(self.caller)?;
        memory.write(self.caller, range, bytes);
        Ok(())
    }

    fn has_signal_trampoline(&mut self) -> bool {
        let Some(Extern::Func(function)) = self.caller.get_export("__wasi_signal_trampoline")
        else {
            return false;
        };
        let ty = function.ty(&mut *self.caller);
        let mut params = ty.params();
        matches!(params.next(), Some(ValType::I32))
            && params.next().is_none()
            && ty.results().next().is_none()
    }

    fn call_signal_trampoline(
        &mut self,
        signal: i32,
    ) -> BoxFuture<'_, agentos_executor_wasm_host::Result<()>> {
        Box::pin(async move {
            let trampoline = self
                .caller
                .get_export("__wasi_signal_trampoline")
                .and_then(|export| export.into_func())
                .ok_or_else(|| {
                    agentos_executor_wasm_host::format_err!(
                        "ERR_AGENTOS_WASMTIME_SIGNAL_TRAMPOLINE: caught signal has no trampoline"
                    )
                })?
                .typed::<i32, ()>(&mut *self.caller)
                .map_err(|error| {
                    agentos_executor_wasm_host::format_err!(
                        "ERR_AGENTOS_WASMTIME_SIGNAL_TRAMPOLINE: invalid trampoline type: {error}"
                    )
                })?;
            trampoline
                .call_async(&mut *self.caller, signal)
                .await
                .map_err(|error| agentos_executor_wasm_host::Error::Engine(Box::new(error)))
        })
    }

    fn fuel(&mut self) -> Result<u64, HostServiceError> {
        self.caller.get_fuel().map_err(|error| {
            eprintln!("ERR_AGENTOS_WASMTIME_FUEL_CONFIG: private Store diagnostic: {error:#}");
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_FUEL_CONFIG",
                "failed to preserve limits.wasm.deterministicFuel",
            )
        })
    }

    fn prepare_exec_replacement(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Self::Module, ExecImportTiers), HostServiceError> {
        let engine = self.caller.data().engine.clone();
        let compiled = module::compile_module(&engine, bytes)?.module;
        let threaded = matches!(
            engine.profile().feature_profile,
            crate::WasmtimeFeatureProfile::AgentOsOwnedWasiV1Threads
        );
        let import_tiers = exec_import_tiers(&compiled, threaded)?;
        if !matches!(compiled.get_export("_start"), Some(wasmtime::ExternType::Func(ty))
            if ty.params().next().is_none() && ty.results().next().is_none())
        {
            return Err(HostServiceError::new(
                "ENOEXEC",
                "replacement has no valid _start",
            ));
        }
        Ok((compiled, import_tiers))
    }
}

pub async fn initialize_inherited_signal_mask(
    store: &mut wasmtime::Store<WasmtimeStoreState>,
    instance: &wasmtime::Instance,
) -> Result<(), HostServiceError> {
    let mask = imports::inherited_signal_mask(
        &store.data().host,
        store.data().thread_id,
        store.data().thread_group.is_some(),
    )
    .await?;
    let Some((low, high)) = mask else {
        return Ok(());
    };
    let setter = instance
        .get_typed_func::<(i32, i32), i32>(&mut *store, "__agentos_set_initial_sigmask")
        .map_err(|error| {
            eprintln!(
                "ERR_AGENTOS_WASMTIME_INITIAL_SIGNAL_MASK: private export diagnostic: {error:#}"
            );
            HostServiceError::new(
                "ERR_AGENTOS_WASMTIME_INITIAL_SIGNAL_MASK",
                "WebAssembly module cannot initialize its inherited signal mask",
            )
        })?;
    let status = setter
        .call_async(&mut *store, (low, high))
        .await
        .map_err(initial_signal_mask_error)?;
    store.data().check_active_cpu_budget()?;
    if status != 0 {
        return Err(HostServiceError::new(
            "ERR_AGENTOS_WASMTIME_INITIAL_SIGNAL_MASK",
            format!("inherited signal-mask initialization failed with errno {status}"),
        ));
    }
    Ok(())
}

fn initial_signal_mask_error(error: wasmtime::Error) -> HostServiceError {
    if let Some(host_error) = error.downcast_ref::<HostServiceError>() {
        if matches!(
            host_error.code.as_str(),
            "ERR_AGENTOS_WASMTIME_CPU_CLOCK" | "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT"
        ) {
            return host_error.clone();
        }
    }
    eprintln!(
        "ERR_AGENTOS_WASMTIME_INITIAL_SIGNAL_MASK: private initialization diagnostic: {error:#}"
    );
    HostServiceError::new(
        "ERR_AGENTOS_WASMTIME_INITIAL_SIGNAL_MASK",
        "inherited signal-mask initialization trapped",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_executor_wasm_host::abi::ABI_BINDINGS;
    use agentos_executor_wasm_host::imports::permitted;

    #[test]
    fn module_import_validation_uses_generated_registry_not_engine_strings() {
        let engine = Engine::default();
        let allowed = ABI_BINDINGS
            .iter()
            .find(|abi| permitted(**abi, WasmPermissionTier::Isolated))
            .expect("isolated import");
        let signature = core_signature(allowed.signature);
        let wat_type = |ty: &CoreValueType| match ty {
            CoreValueType::I32 => "i32",
            CoreValueType::I64 => "i64",
        };
        let params = signature
            .params
            .iter()
            .map(|ty| format!("(param {})", wat_type(ty)))
            .collect::<String>();
        let results = signature
            .results
            .iter()
            .map(|ty| format!("(result {})", wat_type(ty)))
            .collect::<String>();
        let module = Module::new(
            &engine,
            wat::parse_str(format!(
                "(module (import {:?} {:?} (func {params} {results})))",
                allowed.module, allowed.name
            ))
            .expect("allowed import module"),
        )
        .expect("compile allowed import module");
        validate_module_imports(&module, WasmPermissionTier::Isolated, false)
            .expect("generated registry permits import");

        let tiers = exec_import_tiers(&module, false).expect("preflight generated import");
        assert_eq!(
            tiers.isolated,
            permitted(*allowed, WasmPermissionTier::Isolated)
        );
        assert_eq!(
            tiers.read_only,
            permitted(*allowed, WasmPermissionTier::ReadOnly)
        );
        assert_eq!(
            tiers.read_write,
            permitted(*allowed, WasmPermissionTier::ReadWrite)
        );
        assert_eq!(tiers.full, permitted(*allowed, WasmPermissionTier::Full));

        let hostile = Module::new(
            &engine,
            wat::parse_str("(module (import \"ambient_host\" \"escape\" (func)))")
                .expect("hostile import module"),
        )
        .expect("compile hostile import module");
        let error = validate_module_imports(&hostile, WasmPermissionTier::Full, false)
            .expect_err("unknown import must fail before linker diagnostics");
        assert_eq!(error.code, "ERR_AGENTOS_WASM_UNSUPPORTED_IMPORT");
        assert_eq!(
            error.details.expect("typed import details")["module"],
            "ambient_host"
        );
    }

    #[test]
    fn process_exit_preserves_cpu_error_metadata() {
        for code in [
            "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
            "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT",
        ] {
            for default in [
                "ERR_AGENTOS_WASMTIME_INSTANTIATE",
                "ERR_AGENTOS_WASMTIME_TRAP",
            ] {
                let original = HostServiceError::new(code, "stable CPU error")
                    .with_details(serde_json::json!({"limitName": "limits.wasm.activeCpuTimeLimitMs", "limit": 10, "observed": 12}));
                let wrapped =
                    into_wasmtime_error(agentos_executor_wasm_host::Error::Host(original.clone()));
                let normalized = crate::error::normalize(default, &wrapped, false);
                assert_eq!(normalized.code, original.code);
                assert_eq!(normalized.details, original.details);
            }
        }
    }

    #[test]
    fn unwinding_sentinels_keep_their_text() {
        assert_eq!(
            into_wasmtime_error(agentos_executor_wasm_host::Error::Exit(7)).to_string(),
            "agentos:wasi-exit:7"
        );
        assert_eq!(
            into_wasmtime_error(agentos_executor_wasm_host::Error::ExecReplaced).to_string(),
            "agentos:exec-replaced"
        );
    }

    #[test]
    fn initial_signal_mask_preserves_cpu_error_metadata() {
        for code in [
            "ERR_AGENTOS_WASMTIME_CPU_CLOCK",
            "ERR_AGENTOS_WASMTIME_ACTIVE_CPU_LIMIT",
        ] {
            let original = HostServiceError::new(code, "stable CPU error")
                .with_details(serde_json::json!({"limitName": "limits.wasm.activeCpuTimeLimitMs"}));
            let actual = initial_signal_mask_error(wasmtime::Error::new(original.clone()));
            assert_eq!(actual.code, original.code);
            assert_eq!(actual.details, original.details);
        }
    }
}
