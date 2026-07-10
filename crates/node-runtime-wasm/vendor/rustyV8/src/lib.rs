// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

//! # Example
//!
//! ```rust
//! let platform = v8::new_default_platform(0, false).make_shared();
//! v8::V8::initialize_platform(platform);
//! v8::V8::initialize();
//!
//! let isolate = &mut v8::Isolate::new(Default::default());
//!
//! let scope = &mut v8::HandleScope::new(isolate);
//! let context = v8::Context::new(scope, Default::default());
//! let scope = &mut v8::ContextScope::new(scope, context);
//!
//! let code = v8::String::new(scope, "'Hello' + ' World!'").unwrap();
//! println!("javascript code: {}", code.to_rust_string_lossy(scope));
//!
//! let script = v8::Script::compile(scope, code, None).unwrap();
//! let result = script.run(scope).unwrap();
//! let result = result.to_string(scope).unwrap();
//! println!("result: {}", result.to_rust_string_lossy(scope));
//! ```

#![allow(clippy::missing_safety_doc)]

#[macro_use]
extern crate bitflags;

mod array_buffer;
mod array_buffer_view;
mod bigint;
mod binding;
mod context;
pub use context::ContextOptions;
pub mod cppgc;
mod data;
mod date;
mod exception;
mod external;
mod external_references;
pub mod fast_api;
mod fixed_array;
mod function;
mod gc;
mod get_property_names_args_builder;
mod handle;
pub mod icu;
mod isolate;
mod isolate_create_params;
mod microtask;
mod module;
mod name;
mod number;
mod object;
mod platform;
mod primitive_array;
mod primitives;
mod private;
mod promise;
mod property_attribute;
mod property_descriptor;
mod property_filter;
mod property_handler_flags;
mod proxy;
mod regexp;
mod scope;
mod script;
mod script_or_module;
mod shared_array_buffer;
mod snapshot;
mod string;
mod support;
mod symbol;
mod template;
mod typed_array;
mod unbound_module_script;
mod unbound_script;
mod value;
mod value_deserializer;
mod value_serializer;
mod wasm;

pub mod inspector;
pub mod json;
pub mod script_compiler;
// This module is intentionally named "V8" rather than "v8" to match the
// C++ namespace "v8::V8".
#[allow(non_snake_case)]
pub mod V8;

pub use array_buffer::*;
pub use data::*;
pub use exception::*;
pub use external_references::ExternalReference;
pub use external_references::ExternalReferences;
pub use function::*;
pub use gc::*;
pub use get_property_names_args_builder::*;
pub use handle::Eternal;
pub use handle::Global;
pub use handle::Handle;
pub use handle::Local;
pub use handle::TracedReference;
pub use handle::Weak;
pub use isolate::GarbageCollectionType;
pub use isolate::HeapSpaceStatistics;
pub use isolate::HeapStatistics;
pub use isolate::HostCreateShadowRealmContextCallback;
pub use isolate::HostImportModuleDynamicallyCallback;
pub use isolate::HostImportModuleWithPhaseDynamicallyCallback;
pub use isolate::HostInitializeImportMetaObjectCallback;
pub use isolate::Isolate;
pub use isolate::IsolateHandle;
pub use isolate::MemoryPressureLevel;
pub use isolate::MessageCallback;
pub use isolate::MessageErrorLevel;
pub use isolate::MicrotasksPolicy;
pub use isolate::ModuleImportPhase;
pub use isolate::NearHeapLimitCallback;
pub use isolate::OomDetails;
pub use isolate::OomErrorCallback;
pub use isolate::OwnedIsolate;
pub use isolate::PromiseHook;
pub use isolate::PromiseHookType;
pub use isolate::PromiseRejectCallback;
pub use isolate::TimeZoneDetection;
pub use isolate::UseCounterCallback;
pub use isolate::UseCounterFeature;
pub use isolate::WasmAsyncSuccess;
pub use isolate_create_params::CreateParams;
pub use microtask::MicrotaskQueue;
pub use module::*;
pub use object::*;
pub use platform::Platform;
pub use platform::new_default_platform;
pub use platform::new_single_threaded_default_platform;
pub use platform::new_unprotected_default_platform;
pub use primitives::*;
pub use promise::{PromiseRejectEvent, PromiseRejectMessage, PromiseState};
pub use property_attribute::*;
pub use property_descriptor::*;
pub use property_filter::*;
pub use property_handler_flags::*;
pub use regexp::RegExpCreationFlags;
pub use scope::AllowJavascriptExecutionScope;
pub use scope::CallbackScope;
pub use scope::ContextScope;
pub use scope::DisallowJavascriptExecutionScope;
pub use scope::EscapableHandleScope;
pub use scope::HandleScope;
pub use scope::OnFailure;
pub use scope::TryCatch;
pub use script::ScriptOrigin;
pub use script_compiler::CachedData;
pub use snapshot::FunctionCodeHandling;
pub use snapshot::StartupData;
pub use string::Encoding;
pub use string::NewStringType;
pub use string::OneByteConst;
pub use string::ValueView;
pub use string::ValueViewData;
pub use string::WriteFlags;
pub use string::WriteOptions;
pub use support::SharedPtr;
pub use support::SharedRef;
pub use support::UniquePtr;
pub use support::UniqueRef;
pub use template::*;
pub use value_deserializer::ValueDeserializer;
pub use value_deserializer::ValueDeserializerHelper;
pub use value_deserializer::ValueDeserializerImpl;
pub use value_serializer::ValueSerializer;
pub use value_serializer::ValueSerializerHelper;
pub use value_serializer::ValueSerializerImpl;
pub use wasm::CompiledWasmModule;
pub use wasm::WasmStreaming;

/// https://v8.dev/docs/version-numbers
pub const MAJOR_VERSION: u32 = binding::v8__MAJOR_VERSION;
/// https://v8.dev/docs/version-numbers
pub const MINOR_VERSION: u32 = binding::v8__MINOR_VERSION;
/// https://v8.dev/docs/version-numbers
pub const BUILD_NUMBER: u32 = binding::v8__BUILD_NUMBER;
/// https://v8.dev/docs/version-numbers
pub const PATCH_LEVEL: u32 = binding::v8__PATCH_LEVEL;
/// https://v8.dev/docs/version-numbers
pub const VERSION_STRING: &str =
  // TODO: cleanup when Result::unwrap is const stable.
  match binding::v8__VERSION_STRING.to_str() {
    Ok(v) => v,
    Err(_) => panic!("Unable to convert CStr to &str??"),
  };

// TODO(piscisaureus): Ideally this trait would not be exported.
pub use support::MapFnTo;

pub const TYPED_ARRAY_MAX_SIZE_IN_HEAP: usize =
  binding::v8__TYPED_ARRAY_MAX_SIZE_IN_HEAP as _;

#[cfg(test)]
pub(crate) fn initialize_v8() {
  use std::sync::Once;

  static INIT: Once = Once::new();
  INIT.call_once(|| {
    V8::initialize_platform(new_default_platform(0, false).make_shared());
    V8::initialize();
  });
}
