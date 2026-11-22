//! The V8 side of the shared host imports.
//!
//! Each permitted import is a `v8::Function` whose callback converts its
//! arguments, runs the shared handler to completion on this thread, and
//! converts the result. A handler that ends the image (exit, exec, a host
//! failure) records why in [`GuestState::unwind`] and terminates execution,
//! which no guest code can catch.

use super::block_on::block_on;
use super::error::classify_trap;
use super::module::{admit_image, NativeImage};
use agentos_executor_contract::backend::HostServiceError;
use agentos_executor_contract::host::ExecImportTiers;
use agentos_executor_wasm_abi::abi::{core_signature, AbiBinding, CoreValueType};
use agentos_executor_wasm_abi::{StartWasmExecutionRequest, WasmPermissionTier};
use agentos_executor_wasm_host::imports;
use agentos_executor_wasm_host::memory::GuestMemoryError;
use agentos_executor_wasm_host::{BoxFuture, HostState, Val, WasmGuest};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ops::Range;
use std::sync::Arc;

/// Why an import ended the running image.
#[derive(Debug)]
pub(super) enum Unwind {
    /// `proc_exit`; the code is in the host state.
    Exit,
    /// The kernel committed an exec; the replacement is in the host state.
    ExecReplaced,
    /// A host operation failed in a way that ends the process.
    Failed(HostServiceError),
}

/// Everything the import callbacks of one image share. Lives on the guest
/// thread's stack for the image's lifetime; callbacks reach it through the
/// raw pointer in their function data.
pub(super) struct GuestState {
    pub(super) common: HostState<Arc<NativeImage>>,
    pub(super) request: StartWasmExecutionRequest,
    pub(super) memory: Option<v8::Global<v8::WasmMemoryObject>>,
    pub(super) signal_trampoline: Option<v8::Global<v8::Function>>,
    pub(super) unwind: Option<Unwind>,
}

struct BindingData {
    state: *mut GuestState,
    abi: AbiBinding,
}

/// Function data for one image's imports. Must outlive every call into the
/// image, so the lifecycle drops it only after the image's last call returns.
// Each `BindingData` address is handed to V8 as function data, so the boxes
// keep it stable when the vector grows.
#[allow(clippy::vec_box)]
#[derive(Default)]
pub(super) struct Bindings(Vec<Box<BindingData>>);

/// Build the import object for `image` at `tier`. Every import was validated
/// against the tier before this point; an unknown one here is a bug.
pub(super) fn import_object<'s>(
    scope: &v8::PinScope<'s, '_>,
    image: &NativeImage,
    tier: WasmPermissionTier,
    state: *mut GuestState,
    bindings: &mut Bindings,
) -> Result<v8::Local<'s, v8::Object>, HostServiceError> {
    let permitted = imports::permitted_bindings(tier)
        .map(|(module, abi)| ((module, abi.name), abi))
        .collect::<HashMap<_, _>>();
    let object = v8::Object::new(scope);
    let mut namespaces: HashMap<&str, v8::Local<'s, v8::Object>> = HashMap::new();
    for (module, name) in &image.imports {
        let abi = *permitted
            .get(&(module.as_str(), name.as_str()))
            .ok_or_else(|| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_UNSUPPORTED_IMPORT",
                    format!("import {module}.{name} is not available at this permission tier"),
                )
            })?;
        let namespace = match namespaces.get(module.as_str()) {
            Some(namespace) => *namespace,
            None => {
                let namespace = v8::Object::new(scope);
                let key = v8_string(scope, module)?;
                object.set(scope, key.into(), namespace.into());
                namespaces.insert(module.as_str(), namespace);
                namespace
            }
        };
        let data = Box::new(BindingData { state, abi });
        let external = v8::External::new(scope, &*data as *const BindingData as *mut c_void);
        bindings.0.push(data);
        let function = v8::Function::builder(import_callback)
            .data(external.into())
            .build(scope)
            .ok_or_else(|| {
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_LINK",
                    "failed to create a WebAssembly host import",
                )
            })?;
        let key = v8_string(scope, name)?;
        namespace.set(scope, key.into(), function.into());
    }
    Ok(object)
}

pub(super) fn v8_string<'s>(
    scope: &v8::PinScope<'s, '_>,
    value: &str,
) -> Result<v8::Local<'s, v8::String>, HostServiceError> {
    v8::String::new(scope, value).ok_or_else(|| {
        HostServiceError::new("ERR_AGENTOS_WASM_LINK", "failed to allocate a V8 string")
    })
}

fn import_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    rv: v8::ReturnValue,
) {
    let Ok(external) = v8::Local::<v8::External>::try_from(args.data()) else {
        eprintln!("ERR_AGENTOS_WASM_LINK: host import is missing its binding data");
        scope.terminate_execution();
        return;
    };
    // SAFETY: the pointer is a live `BindingData` owned by the image's
    // `Bindings`, which outlive every call into the image.
    let binding = unsafe { &*(external.value() as *const BindingData) };
    let state = binding.state;
    // SAFETY: `state` is the image's live `GuestState`; no other reference to
    // it is held across this point.
    if unsafe { (*state).unwind.is_some() } {
        scope.terminate_execution();
        return;
    }
    // V8 calls this through an `extern "C"` boundary, where a panic would
    // abort the whole sidecar. Contain it to this guest, as Wasmtime does.
    // Each nested import call has its own guard, so a panic never unwinds
    // through V8 frames.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_import(scope, args, rv, binding, state)
    }));
    if outcome.is_err() {
        record(
            state,
            agentos_executor_wasm_host::Error::Host(HostServiceError::new(
                "ERR_AGENTOS_WASM_HOST_PANIC",
                "a WebAssembly host import failed unexpectedly; the guest was stopped",
            )),
        );
        scope.terminate_execution();
    }
}

fn run_import<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
    binding: &BindingData,
    state: *mut GuestState,
) {
    let signature = core_signature(binding.abi.signature);
    let mut params = Vec::with_capacity(signature.params.len());
    for (index, ty) in signature.params.iter().enumerate() {
        let value = args.get(index as i32);
        let converted = match ty {
            CoreValueType::I32 => value.int32_value(scope).map(Val::I32),
            CoreValueType::I64 => v8::Local::<v8::BigInt>::try_from(value)
                .ok()
                .map(|value| Val::I64(value.i64_value().0)),
        };
        let Some(converted) = converted else {
            record(
                state,
                agentos_executor_wasm_host::Error::Message(format!(
                    "invalid AgentOS ABI argument at index {index}"
                )),
            );
            scope.terminate_execution();
            return;
        };
        params.push(converted);
    }
    let mut results = imports::result_slots(binding.abi);
    let outcome = {
        let mut guest = V8Guest {
            scope: &mut *scope,
            state,
        };
        block_on(imports::dispatch(
            &mut guest,
            binding.abi,
            &params,
            &mut results,
        ))
    };
    match outcome {
        Ok(()) => match results.as_slice() {
            [] => rv.set_undefined(),
            [Val::I32(value)] => rv.set_int32(*value),
            [Val::I64(value)] => rv.set(v8::BigInt::new_from_i64(scope, *value).into()),
            _ => {
                record(
                    state,
                    agentos_executor_wasm_host::Error::Message(String::from(
                        "unsupported AgentOS ABI result shape",
                    )),
                );
                scope.terminate_execution();
            }
        },
        Err(error) => {
            record(state, error);
            scope.terminate_execution();
        }
    }
}

/// Keep the first reason an image ended; a nested import may already have
/// recorded it before the outer one unwinds.
fn record(state: *mut GuestState, error: agentos_executor_wasm_host::Error) {
    use agentos_executor_wasm_host::Error;
    // SAFETY: see `import_callback`.
    let state = unsafe { &mut *state };
    if state.unwind.is_some() {
        return;
    }
    state.unwind = Some(match error {
        Error::Exit(_) => Unwind::Exit,
        Error::ExecReplaced => Unwind::ExecReplaced,
        Error::Host(error) => Unwind::Failed(error),
        Error::Message(message) => {
            eprintln!("ERR_AGENTOS_WASM_HOST_IMPORT: private import diagnostic: {message}");
            Unwind::Failed(
                HostServiceError::new(
                    "ERR_AGENTOS_WASM_TRAP",
                    "WebAssembly validation or execution failed",
                )
                .with_details(serde_json::json!({ "engine": "v8" })),
            )
        }
        Error::Engine(error) => match error.downcast::<HostServiceError>() {
            Ok(error) => Unwind::Failed(*error),
            Err(_) => Unwind::Failed(HostServiceError::new(
                "ERR_AGENTOS_WASM_TRAP",
                "WebAssembly validation or execution failed",
            )),
        },
    });
}

/// The calling V8 guest, as the shared handlers see it.
struct V8Guest<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    state: *mut GuestState,
}

impl V8Guest<'_, '_, '_> {
    /// Re-resolve the guest's memory. Growth replaces the backing buffer, so
    /// no pointer is kept between accesses.
    fn memory(&mut self) -> Result<(*mut u8, usize), GuestMemoryError> {
        // SAFETY: see `import_callback`.
        let state = unsafe { &*self.state };
        let memory = state
            .memory
            .as_ref()
            .ok_or(GuestMemoryError::MissingMemory)?;
        let memory = v8::Local::new(&*self.scope, memory);
        let buffer = memory.buffer();
        let length = buffer.byte_length();
        let data = buffer
            .data()
            .map_or(std::ptr::null_mut(), |data| data.as_ptr() as *mut u8);
        if data.is_null() && length != 0 {
            return Err(GuestMemoryError::MissingMemory);
        }
        Ok((data, length))
    }
}

impl WasmGuest for V8Guest<'_, '_, '_> {
    type Module = Arc<NativeImage>;

    fn data(&self) -> &HostState<Self::Module> {
        // SAFETY: see `import_callback`.
        unsafe { &(*self.state).common }
    }

    fn data_mut(&mut self) -> &mut HostState<Self::Module> {
        // SAFETY: see `import_callback`.
        unsafe { &mut (*self.state).common }
    }

    fn memory_size(&mut self) -> Result<usize, GuestMemoryError> {
        Ok(self.memory()?.1)
    }

    fn memory_read(&mut self, range: Range<usize>) -> Result<Vec<u8>, GuestMemoryError> {
        let (data, length) = self.memory()?;
        if range.start > range.end || range.end > length {
            return Err(GuestMemoryError::OutOfBounds);
        }
        // SAFETY: the range is inside the live buffer, and no guest code runs
        // while this copy is made.
        Ok(unsafe { std::slice::from_raw_parts(data.add(range.start), range.len()) }.to_vec())
    }

    fn memory_write(&mut self, range: Range<usize>, bytes: &[u8]) -> Result<(), GuestMemoryError> {
        let (data, length) = self.memory()?;
        if range.start > range.end || range.end > length || range.len() != bytes.len() {
            return Err(GuestMemoryError::OutOfBounds);
        }
        // SAFETY: as for `memory_read`; the destination cannot overlap `bytes`,
        // which the handlers own.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.add(range.start), bytes.len())
        };
        Ok(())
    }

    fn has_signal_trampoline(&mut self) -> bool {
        // SAFETY: see `import_callback`.
        unsafe { (*self.state).signal_trampoline.is_some() }
    }

    fn call_signal_trampoline(
        &mut self,
        signal: i32,
    ) -> BoxFuture<'_, agentos_executor_wasm_host::Result<()>> {
        // SAFETY: see `import_callback`.
        let trampoline = unsafe { (*self.state).signal_trampoline.clone() };
        let Some(trampoline) = trampoline else {
            return Box::pin(std::future::ready(Err(
                agentos_executor_wasm_host::format_err!(
                    "ERR_AGENTOS_WASM_SIGNAL_TRAMPOLINE: caught signal has no trampoline"
                ),
            )));
        };
        let scope = &mut *self.scope;
        v8::tc_scope!(let tc, scope);
        let function = v8::Local::new(&*tc, &trampoline);
        let receiver = v8::undefined(&*tc).into();
        let argument = v8::Integer::new(&*tc, signal).into();
        let result = function.call(&*tc, receiver, &[argument]);
        Box::pin(std::future::ready(match result {
            Some(_) => Ok(()),
            // A nested import already recorded why the image is ending.
            None if tc.has_terminated() => Err(agentos_executor_wasm_host::Error::Engine(
                Box::new(HostServiceError::new(
                    "ERR_AGENTOS_WASM_UNWINDING",
                    "the guest image is ending",
                )),
            )),
            None => {
                let diagnostic = tc
                    .exception()
                    .map(|exception| exception.to_rust_string_lossy(&*tc))
                    .unwrap_or_default();
                Err(agentos_executor_wasm_host::Error::Engine(Box::new(
                    classify_trap(&diagnostic),
                )))
            }
        }))
    }

    fn fuel(&mut self) -> Result<u64, HostServiceError> {
        Err(HostServiceError::new(
            "ERR_AGENTOS_WASM_FUEL_UNSUPPORTED",
            "the V8 engine does not meter deterministic fuel",
        ))
    }

    fn prepare_exec_replacement(
        &mut self,
        bytes: &[u8],
    ) -> Result<(Self::Module, ExecImportTiers), HostServiceError> {
        // SAFETY: see `import_callback`.
        let request = unsafe { &(*self.state).request };
        let admitted = admit_image(&mut *self.scope, bytes, None, request, None)?;
        if !admitted.has_valid_start {
            return Err(HostServiceError::new(
                "ENOEXEC",
                "replacement has no valid _start",
            ));
        }
        Ok((admitted.image, admitted.tiers))
    }
}
