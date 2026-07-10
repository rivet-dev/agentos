//! V8 import-object adapter for the shared AgentOS Linux/POSIX provider.
//!
//! The object is closure-private runtime state. Each generated function closes
//! over a fixed [`SyscallId`]; guest code cannot submit an arbitrary syscall
//! number or string operation. See POSIX.1-2024 and the per-row references in
//! `docs-internal/node-runtime-wasm-abi/agentos-posix-contract.json`.

use agentos_wasm_posix_host::{
    KernelDispatcher, PosixProvider, ProviderError, SharedLinearMemory, SyscallId, WasmType,
    WasmValue, SYSCALLS,
};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

struct BindingContext {
    provider: Arc<PosixProvider>,
    kernel: Arc<Mutex<Box<dyn KernelDispatcher>>>,
    memory: v8::Global<v8::WasmMemoryObject>,
}

struct CallbackData {
    context: *const BindingContext,
    id: SyscallId,
}

/// Owns every native callback payload and the closure-private import object.
/// It must outlive all V8 instances created with `local()`.
pub struct V8PosixImportObject {
    object: v8::Global<v8::Object>,
    _context: Box<BindingContext>,
    _callbacks: Box<[CallbackData]>,
}

impl V8PosixImportObject {
    pub fn new<'s>(
        scope: &mut v8::HandleScope<'s>,
        memory: v8::Local<'s, v8::WasmMemoryObject>,
        provider: Arc<PosixProvider>,
        kernel: Arc<Mutex<Box<dyn KernelDispatcher>>>,
    ) -> Result<Self, String> {
        let context = Box::new(BindingContext {
            provider,
            kernel,
            memory: v8::Global::new(scope, memory),
        });
        let context_pointer = &*context as *const BindingContext;
        let object = v8::Object::new(scope);
        let callbacks = SYSCALLS
            .iter()
            .map(|descriptor| CallbackData {
                context: context_pointer,
                id: descriptor.id,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        for (descriptor, data) in SYSCALLS.iter().zip(callbacks.iter()) {
            let pointer = data as *const CallbackData as *mut c_void;
            let external = v8::External::new(scope, pointer);
            let function = v8::FunctionTemplate::builder(posix_callback)
                .data(external.into())
                .build(scope)
                .get_function(scope)
                .ok_or_else(|| format!("failed to create POSIX import {}", descriptor.name))?;
            let name = v8::String::new(scope, descriptor.name).ok_or_else(|| {
                format!("POSIX import name is not a V8 string: {}", descriptor.name)
            })?;
            if object.set(scope, name.into(), function.into()) != Some(true) {
                return Err(format!(
                    "failed to install POSIX import {}",
                    descriptor.name
                ));
            }
        }
        Ok(Self {
            object: v8::Global::new(scope, object),
            _context: context,
            _callbacks: callbacks,
        })
    }

    pub fn local<'s>(&self, scope: &mut v8::HandleScope<'s>) -> v8::Local<'s, v8::Object> {
        v8::Local::new(scope, &self.object)
    }
}

fn posix_callback(
    scope: &mut v8::HandleScope,
    arguments: v8::FunctionCallbackArguments,
    mut return_value: v8::ReturnValue,
) {
    let result = (|| -> Result<Option<WasmValue>, String> {
        let external = v8::Local::<v8::External>::try_from(arguments.data())
            .map_err(|_| "POSIX callback data is not External".to_owned())?;
        // SAFETY: V8PosixImportObject owns the boxed payload for longer than
        // every function and instance that can invoke this callback.
        let data = unsafe { &*(external.value() as *const CallbackData) };
        // SAFETY: CallbackData points to the boxed context owned by the same
        // V8PosixImportObject and neither box moves after construction.
        let context = unsafe { &*data.context };
        let descriptor = data.id.descriptor();
        let mut values = Vec::with_capacity(descriptor.params.len());
        for (index, kind) in descriptor.params.iter().copied().enumerate() {
            values.push(v8_to_wasm_value(scope, arguments.get(index as i32), kind)?);
        }

        let memory = v8::Local::new(scope, &context.memory);
        let buffer = memory.buffer();
        let backing_store = buffer.get_backing_store();
        let pointer = backing_store
            .data()
            .ok_or_else(|| "Node WASM memory has no backing-store data".to_owned())?
            .cast::<u8>();
        // SAFETY: `backing_store` remains owned in this stack frame through
        // dispatch, and V8 shared memory permits atomic byte access. A fresh
        // view is constructed on every call so memory growth cannot stale it.
        let mut memory =
            unsafe { SharedLinearMemory::from_raw_parts(pointer, backing_store.byte_length()) };
        let mut kernel = context
            .kernel
            .lock()
            .map_err(|_| "POSIX kernel dispatcher lock poisoned".to_owned())?;
        context
            .provider
            .invoke(data.id, &values, &mut memory, kernel.as_mut())
            .map_err(|error| provider_error(descriptor.name, error))
    })();

    match result {
        Ok(None) => return_value.set_undefined(),
        Ok(Some(WasmValue::I32(value))) => return_value.set_int32(value),
        Ok(Some(WasmValue::I64(value))) => {
            return_value.set(v8::BigInt::new_from_i64(scope, value).into())
        }
        Ok(Some(WasmValue::F32(value))) => {
            return_value.set(v8::Number::new(scope, value.into()).into())
        }
        Ok(Some(WasmValue::F64(value))) => return_value.set(v8::Number::new(scope, value).into()),
        Err(error) => {
            let message = v8::String::new(scope, &error).unwrap();
            let exception = v8::Exception::error(scope, message);
            scope.throw_exception(exception);
        }
    }
}

fn v8_to_wasm_value(
    scope: &mut v8::HandleScope,
    value: v8::Local<v8::Value>,
    expected: WasmType,
) -> Result<WasmValue, String> {
    match expected {
        WasmType::I32 => value
            .int32_value(scope)
            .map(WasmValue::I32)
            .ok_or_else(|| "POSIX i32 argument conversion failed".to_owned()),
        WasmType::I64 => v8::Local::<v8::BigInt>::try_from(value)
            .map(|value| WasmValue::I64(value.i64_value().0))
            .map_err(|_| "POSIX i64 argument is not a BigInt".to_owned()),
        WasmType::F32 => value
            .number_value(scope)
            .map(|value| WasmValue::F32(value as f32))
            .ok_or_else(|| "POSIX f32 argument conversion failed".to_owned()),
        WasmType::F64 => value
            .number_value(scope)
            .map(WasmValue::F64)
            .ok_or_else(|| "POSIX f64 argument conversion failed".to_owned()),
    }
}

fn provider_error(syscall: &'static str, error: ProviderError) -> String {
    format!("agentos_posix_v1.{syscall} failed: {error}")
}
