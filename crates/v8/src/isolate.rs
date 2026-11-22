// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::Array;
use crate::CallbackScope;
use crate::Context;
use crate::Data;
use crate::FixedArray;
use crate::Function;
use crate::FunctionCodeHandling;
use crate::IsolateGroup;
use crate::Local;
use crate::Message;
use crate::Module;
use crate::Object;
use crate::PinScope;
use crate::Platform;
use crate::Promise;
use crate::PromiseResolver;
use crate::StartupData;
use crate::String;
use crate::V8::get_current_platform;
use crate::Value;
use crate::binding::v8__HeapCodeStatistics;
use crate::binding::v8__HeapSpaceStatistics;
use crate::binding::v8__HeapStatistics;
use crate::binding::v8__Isolate__UseCounterFeature;
pub use crate::binding::v8__ModuleImportPhase as ModuleImportPhase;
use crate::cppgc::Heap;
use crate::external_references::ExternalReference;
use crate::function::FunctionCallbackInfo;
use crate::gc::GCCallbackFlags;
use crate::gc::GCType;
use crate::handle::FinalizerCallback;
use crate::handle::FinalizerMap;
use crate::isolate_create_params::CreateParams;
use crate::isolate_create_params::raw;
use crate::promise::PromiseRejectMessage;
use crate::snapshot::SnapshotCreator;
use crate::support::MapFnFrom;
use crate::support::MapFnTo;
use crate::support::Opaque;
use crate::support::ToCFn;
use crate::support::UnitType;
use crate::support::char;
use crate::support::int;
use crate::support::size_t;
use crate::wasm::WasmStreaming;
use crate::wasm::trampoline;
use std::cell::UnsafeCell;
use std::ffi::CStr;

use std::any::Any;
use std::any::TypeId;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::c_void;
use std::fmt::{self, Debug, Formatter};
use std::hash::BuildHasher;
use std::hash::Hasher;
use std::mem::MaybeUninit;
use std::mem::align_of;
use std::mem::forget;
use std::mem::needs_drop;
use std::mem::size_of;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::pin;
use std::ptr;
use std::ptr::NonNull;
use std::ptr::addr_of_mut;
use std::ptr::drop_in_place;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::AtomicUsize;

/// Policy for running microtasks:
///   - explicit: microtasks are invoked with the
///     Isolate::PerformMicrotaskCheckpoint() method;
///   - auto: microtasks are invoked when the script call depth decrements
///     to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum MicrotasksPolicy {
  Explicit = 0,
  // Scoped = 1 (RAII) is omitted for now, doesn't quite map to idiomatic Rust.
  Auto = 2,
}

/// Signal for dependants of contexts. Useful for
/// [`Isolate::context_disposed_notification`] to implement different
/// strategies.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextDependants {
  /// Context has no dependants. These are usually top-level contexts.
  NoDependants,
  /// Context has some dependants, i.e., it may depend on other contexts. This
  /// is usually the case for inner contexts.
  SomeDependants,
}

/// Memory pressure level for the MemoryPressureNotification.
/// None hints V8 that there is no memory pressure.
/// Moderate hints V8 to speed up incremental garbage collection at the cost
/// of higher latency due to garbage collection pauses.
/// Critical hints V8 to free memory as soon as possible. Garbage collection
/// pauses at this level will be large.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum MemoryPressureLevel {
  None = 0,
  Moderate = 1,
  Critical = 2,
}

/// Time zone redetection indicator for
/// DateTimeConfigurationChangeNotification.
///
/// kSkip indicates V8 that the notification should not trigger redetecting
/// host time zone. kRedetect indicates V8 that host time zone should be
/// redetected, and used to set the default time zone.
///
/// The host time zone detection may require file system access or similar
/// operations unlikely to be available inside a sandbox. If v8 is run inside a
/// sandbox, the host time zone has to be detected outside the sandbox before
/// calling DateTimeConfigurationChangeNotification function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum TimeZoneDetection {
  Skip = 0,
  Redetect = 1,
}

/// PromiseHook with type Init is called when a new promise is
/// created. When a new promise is created as part of the chain in the
/// case of Promise.then or in the intermediate promises created by
/// Promise.{race, all}/AsyncFunctionAwait, we pass the parent promise
/// otherwise we pass undefined.
///
/// PromiseHook with type Resolve is called at the beginning of
/// resolve or reject function defined by CreateResolvingFunctions.
///
/// PromiseHook with type Before is called at the beginning of the
/// PromiseReactionJob.
///
/// PromiseHook with type After is called right at the end of the
/// PromiseReactionJob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum PromiseHookType {
  Init,
  Resolve,
  Before,
  After,
}

/// Types of garbage collections that can be requested via
/// [`Isolate::request_garbage_collection_for_testing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum GarbageCollectionType {
  Full,
  Minor,
}

pub type MessageCallback = unsafe extern "C" fn(Local<Message>, Local<Value>);

bitflags! {
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  #[repr(transparent)]
  pub struct MessageErrorLevel: int {
    const LOG = 1 << 0;
    const DEBUG = 1 << 1;
    const INFO = 1 << 2;
    const ERROR = 1 << 3;
    const WARNING = 1 << 4;
    const ALL = (1 << 5) - 1;
  }
}

pub type PromiseHook =
  unsafe extern "C" fn(PromiseHookType, Local<Promise>, Local<Value>);

pub type PromiseRejectCallback = unsafe extern "C" fn(PromiseRejectMessage);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum WasmAsyncSuccess {
  Success,
  Fail,
}
pub type WasmAsyncResolvePromiseCallback = unsafe extern "C" fn(
  UnsafeRawIsolatePtr,
  Local<Context>,
  Local<PromiseResolver>,
  Local<Value>,
  WasmAsyncSuccess,
);

pub type AllowWasmCodeGenerationCallback =
  unsafe extern "C" fn(Local<Context>, Local<String>) -> bool;

/// HostInitializeImportMetaObjectCallback is called the first time import.meta
/// is accessed for a module. Subsequent access will reuse the same value.
///
/// The method combines two implementation-defined abstract operations into one:
/// HostGetImportMetaProperties and HostFinalizeImportMeta.
///
/// The embedder should use v8::Object::CreateDataProperty to add properties on
/// the meta object.
pub type HostInitializeImportMetaObjectCallback =
  unsafe extern "C" fn(Local<Context>, Local<Module>, Local<Object>);

/// HostImportModuleDynamicallyCallback is called when we require the embedder
/// to load a module. This is used as part of the dynamic import syntax.
///
/// The host_defined_options are metadata provided by the host environment, which may be used
/// to customize or further specify how the module should be imported.
///
/// The resource_name is the identifier or path for the module or script making the import request.
///
/// The specifier is the name of the module that should be imported.
///
/// The import_attributes are import assertions for this request in the form:
/// [key1, value1, key2, value2, ...] where the keys and values are of type
/// v8::String. Note, unlike the FixedArray passed to ResolveModuleCallback and
/// returned from ModuleRequest::GetImportAssertions(), this array does not
/// contain the source Locations of the assertions.
///
/// The embedder must compile, instantiate, evaluate the Module, and obtain its
/// namespace object.
///
/// The Promise returned from this function is forwarded to userland JavaScript.
/// The embedder must resolve this promise with the module namespace object. In
/// case of an exception, the embedder must reject this promise with the
/// exception. If the promise creation itself fails (e.g. due to stack
/// overflow), the embedder must propagate that exception by returning an empty
/// MaybeLocal.
///
/// # Example
///
/// ```
/// fn host_import_module_dynamically_callback_example<'s>(
///   scope: &mut v8::HandleScope<'s>,
///   host_defined_options: v8::Local<'s, v8::Data>,
///   resource_name: v8::Local<'s, v8::Value>,
///   specifier: v8::Local<'s, v8::String>,
///   import_attributes: v8::Local<'s, v8::FixedArray>,
/// ) -> Option<v8::Local<'s, v8::Promise>> {
///   todo!()
/// }
/// ```
pub trait HostImportModuleDynamicallyCallback:
  UnitType
  + for<'s, 'i> FnOnce(
    &mut PinScope<'s, 'i>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    Local<'s, FixedArray>,
  ) -> Option<Local<'s, Promise>>
{
  fn to_c_fn(self) -> RawHostImportModuleDynamicallyCallback;
}

#[cfg(target_family = "unix")]
pub(crate) type RawHostImportModuleDynamicallyCallback =
  for<'s> unsafe extern "C" fn(
    Local<'s, Context>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    Local<'s, FixedArray>,
  ) -> *mut Promise;

#[cfg(all(
  target_family = "windows",
  any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub type RawHostImportModuleDynamicallyCallback =
  for<'s> unsafe extern "C" fn(
    *mut *mut Promise,
    Local<'s, Context>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    Local<'s, FixedArray>,
  ) -> *mut *mut Promise;

impl<F> HostImportModuleDynamicallyCallback for F
where
  F: UnitType
    + for<'s, 'i> FnOnce(
      &mut PinScope<'s, 'i>,
      Local<'s, Data>,
      Local<'s, Value>,
      Local<'s, String>,
      Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>>,
{
  #[inline(always)]
  fn to_c_fn(self) -> RawHostImportModuleDynamicallyCallback {
    #[allow(unused_variables)]
    #[inline(always)]
    fn scope_adapter<'s, 'i: 's, F: HostImportModuleDynamicallyCallback>(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_attributes: Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>> {
      let scope = pin!(unsafe { CallbackScope::new(context) });
      let mut scope = scope.init();
      (F::get())(
        &mut scope,
        host_defined_options,
        resource_name,
        specifier,
        import_attributes,
      )
    }

    #[cfg(target_family = "unix")]
    #[inline(always)]
    unsafe extern "C" fn abi_adapter<
      's,
      F: HostImportModuleDynamicallyCallback,
    >(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_attributes: Local<'s, FixedArray>,
    ) -> *mut Promise {
      scope_adapter::<F>(
        context,
        host_defined_options,
        resource_name,
        specifier,
        import_attributes,
      )
      .map_or_else(null_mut, |return_value| return_value.as_non_null().as_ptr())
    }

    #[cfg(all(
      target_family = "windows",
      any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[inline(always)]
    unsafe extern "C" fn abi_adapter<
      's,
      F: HostImportModuleDynamicallyCallback,
    >(
      return_value: *mut *mut Promise,
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_attributes: Local<'s, FixedArray>,
    ) -> *mut *mut Promise {
      unsafe {
        std::ptr::write(
          return_value,
          scope_adapter::<F>(
            context,
            host_defined_options,
            resource_name,
            specifier,
            import_attributes,
          )
          .map(|return_value| return_value.as_non_null().as_ptr())
          .unwrap_or_else(null_mut),
        );
        return_value
      }
    }

    abi_adapter::<F>
  }
}

/// HostImportModuleWithPhaseDynamicallyCallback is called when we
/// require the embedder to load a module with a specific phase. This is used
/// as part of the dynamic import syntax.
///
/// The referrer contains metadata about the script/module that calls
/// import.
///
/// The specifier is the name of the module that should be imported.
///
/// The phase is the phase of the import requested.
///
/// The import_attributes are import attributes for this request in the form:
/// [key1, value1, key2, value2, ...] where the keys and values are of type
/// v8::String. Note, unlike the FixedArray passed to ResolveModuleCallback and
/// returned from ModuleRequest::GetImportAttributes(), this array does not
/// contain the source Locations of the attributes.
///
/// The Promise returned from this function is forwarded to userland
/// JavaScript. The embedder must resolve this promise according to the phase
/// requested:
/// - For ModuleImportPhase::kSource, the promise must be resolved with a
///   compiled ModuleSource object, or rejected with a SyntaxError if the
///   module does not support source representation.
/// - For ModuleImportPhase::kEvaluation, the promise must be resolved with a
///   ModuleNamespace object of a module that has been compiled, instantiated,
///   and evaluated.
///
/// In case of an exception, the embedder must reject this promise with the
/// exception. If the promise creation itself fails (e.g. due to stack
/// overflow), the embedder must propagate that exception by returning an empty
/// MaybeLocal.
///
/// This callback is still experimental and is only invoked for source phase
/// imports.
pub trait HostImportModuleWithPhaseDynamicallyCallback:
  UnitType
  + for<'s, 'i> FnOnce(
    &mut PinScope<'s, 'i>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    ModuleImportPhase,
    Local<'s, FixedArray>,
  ) -> Option<Local<'s, Promise>>
{
  fn to_c_fn(self) -> RawHostImportModuleWithPhaseDynamicallyCallback;
}

#[cfg(target_family = "unix")]
pub(crate) type RawHostImportModuleWithPhaseDynamicallyCallback =
  for<'s> unsafe extern "C" fn(
    Local<'s, Context>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    ModuleImportPhase,
    Local<'s, FixedArray>,
  ) -> *mut Promise;

#[cfg(all(
  target_family = "windows",
  any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub type RawHostImportModuleWithPhaseDynamicallyCallback =
  for<'s> unsafe extern "C" fn(
    *mut *mut Promise,
    Local<'s, Context>,
    Local<'s, Data>,
    Local<'s, Value>,
    Local<'s, String>,
    ModuleImportPhase,
    Local<'s, FixedArray>,
  ) -> *mut *mut Promise;

impl<F> HostImportModuleWithPhaseDynamicallyCallback for F
where
  F: UnitType
    + for<'s, 'i> FnOnce(
      &mut PinScope<'s, 'i>,
      Local<'s, Data>,
      Local<'s, Value>,
      Local<'s, String>,
      ModuleImportPhase,
      Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>>,
{
  #[inline(always)]
  fn to_c_fn(self) -> RawHostImportModuleWithPhaseDynamicallyCallback {
    #[allow(unused_variables)]
    #[inline(always)]
    fn scope_adapter<'s, F: HostImportModuleWithPhaseDynamicallyCallback>(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_phase: ModuleImportPhase,
      import_attributes: Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>> {
      let scope = pin!(unsafe { CallbackScope::new(context) });
      let mut scope = scope.init();
      (F::get())(
        &mut scope,
        host_defined_options,
        resource_name,
        specifier,
        import_phase,
        import_attributes,
      )
    }

    #[cfg(target_family = "unix")]
    #[inline(always)]
    unsafe extern "C" fn abi_adapter<
      's,
      F: HostImportModuleWithPhaseDynamicallyCallback,
    >(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_phase: ModuleImportPhase,
      import_attributes: Local<'s, FixedArray>,
    ) -> *mut Promise {
      scope_adapter::<F>(
        context,
        host_defined_options,
        resource_name,
        specifier,
        import_phase,
        import_attributes,
      )
      .map_or_else(null_mut, |return_value| return_value.as_non_null().as_ptr())
    }

    #[cfg(all(
      target_family = "windows",
      any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[inline(always)]
    unsafe extern "C" fn abi_adapter<
      's,
      F: HostImportModuleWithPhaseDynamicallyCallback,
    >(
      return_value: *mut *mut Promise,
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_phase: ModuleImportPhase,
      import_attributes: Local<'s, FixedArray>,
    ) -> *mut *mut Promise {
      unsafe {
        std::ptr::write(
          return_value,
          scope_adapter::<F>(
            context,
            host_defined_options,
            resource_name,
            specifier,
            import_phase,
            import_attributes,
          )
          .map(|return_value| return_value.as_non_null().as_ptr())
          .unwrap_or_else(null_mut),
        );
        return_value
      }
    }

    abi_adapter::<F>
  }
}

/// `HostCreateShadowRealmContextCallback` is called each time a `ShadowRealm`
/// is being constructed. You can use [`HandleScope::get_current_context`] to
/// get the [`Context`] in which the constructor is being run.
///
/// The method combines [`Context`] creation and the implementation-defined
/// abstract operation `HostInitializeShadowRealm` into one.
///
/// The embedder should use [`Context::new`] to create a new context. If the
/// creation fails, the embedder must propagate that exception by returning
/// [`None`].
pub type HostCreateShadowRealmContextCallback =
  for<'s, 'i> fn(scope: &mut PinScope<'s, 'i>) -> Option<Local<'s, Context>>;

pub type GcCallbackWithData = unsafe extern "C" fn(
  isolate: UnsafeRawIsolatePtr,
  r#type: GCType,
  flags: GCCallbackFlags,
  data: *mut c_void,
);

pub type InterruptCallback =
  unsafe extern "C" fn(isolate: UnsafeRawIsolatePtr, data: *mut c_void);

pub type NearHeapLimitCallback = unsafe extern "C" fn(
  data: *mut c_void,
  current_heap_limit: usize,
  initial_heap_limit: usize,
) -> usize;

#[repr(C)]
pub struct OomDetails {
  pub is_heap_oom: bool,
  pub detail: *const char,
}

pub type OomErrorCallback =
  unsafe extern "C" fn(location: *const char, details: &OomDetails);

/// The outcome of a [`ModifyCodeGenerationFromStringsCallback`].
///
/// Note that this is not the type V8 sees; it is translated into an internal
/// `#[repr(C)]` equivalent before crossing the FFI boundary, so its layout
/// carries no ABI guarantees.
pub struct ModifyCodeGenerationFromStringsResult<'s> {
  /// If true, proceed with the code generation. Otherwise, block it.
  pub codegen_allowed: bool,
  /// Compile this source instead of the original one, if present. This field
  /// is only considered when `codegen_allowed` is true.
  pub modified_source: Option<Local<'s, String>>,
}

/// Called when JavaScript dynamically generates code, i.e. through `eval` or
/// the `Function` constructor. The callback decides whether the code generation
/// is allowed, and may substitute the source that is about to be compiled.
///
/// V8 skips this callback only when the context allows code generation from
/// strings *and* the source is already a string. So it is consulted either
/// when the context has code generation disabled (see
/// [`Context::set_allow_generation_from_strings`]) or when the source is
/// something other than a string, whichever comes first.
///
/// `source` is the value that was handed to `eval`/`Function`; it is not
/// necessarily a string. When it isn't, returning `codegen_allowed: true`
/// without a `modified_source` leaves V8 with nothing it knows how to compile,
/// and the source object is handed back to the caller unchanged; supply a
/// `modified_source` to have it compiled instead.
///
/// `is_code_like` is true when that value is an object that the host marked as
/// "code like" via `v8::ObjectTemplate::SetCodeLike()`, which rusty_v8 does not
/// currently expose.
pub type ModifyCodeGenerationFromStringsCallback =
  for<'s, 'i> fn(
    scope: &mut PinScope<'s, 'i>,
    source: Local<'s, Value>,
    is_code_like: bool,
  ) -> ModifyCodeGenerationFromStringsResult<'s>;

// The layout that V8 actually sees. It matches
// `v8::ModifyCodeGenerationFromStringsResult`, with the `MaybeLocal<String>`
// spelled as a nullable raw pointer so that the `improper_ctypes` lints — which
// don't recognize the niche in `Option<Local<T>>` — are satisfied.
#[repr(C)]
struct RawModifyCodeGenerationFromStringsResult {
  codegen_allowed: bool,
  modified_source: *mut String,
}

// Unlike `PrepareStackTraceCallback` and the shadow realm callback below, this
// one needs no `#[cfg(target_os = "windows")]` sret shim. Those return a bare
// `MaybeLocal`, which is 8 bytes: MSVC returns it through a hidden pointer
// because it has user-declared constructors, while Rust returns it in a
// register, so the two disagree. `ModifyCodeGenerationFromStringsResult` is 16
// bytes, which is over the register-return threshold on every ABI we target, so
// MSVC and rustc independently agree on the hidden-pointer form. On System V
// and AArch64 it is trivially copyable and fits in two registers, and both
// sides agree there too.
type RawModifyCodeGenerationFromStringsCallback =
  for<'s> unsafe extern "C" fn(
    context: Local<'s, Context>,
    source: Local<'s, Value>,
    is_code_like: bool,
  ) -> RawModifyCodeGenerationFromStringsResult;

// Windows x64 ABI: MaybeLocal<Value> returned on the stack.
#[cfg(target_os = "windows")]
pub type PrepareStackTraceCallback<'s> =
  unsafe extern "C" fn(
    *mut *const Value,
    Local<'s, Context>,
    Local<'s, Value>,
    Local<'s, Array>,
  ) -> *mut *const Value;

#[cfg(not(target_os = "windows"))]
pub type PrepareStackTraceCallback<'s> =
  unsafe extern "C" fn(
    Local<'s, Context>,
    Local<'s, Value>,
    Local<'s, Array>,
  ) -> PrepareStackTraceCallbackRet;

// System V ABI: MaybeLocal<Value> returned in a register.
// System V i386 ABI: Local<Value> returned in hidden pointer (struct).
#[cfg(not(target_os = "windows"))]
#[repr(C)]
pub struct PrepareStackTraceCallbackRet(*const Value);

pub type UseCounterFeature = v8__Isolate__UseCounterFeature;
pub type UseCounterCallback =
  unsafe extern "C" fn(&mut Isolate, UseCounterFeature);

unsafe extern "C" {
  fn v8__Isolate__New(params: *const raw::CreateParams) -> *mut RealIsolate;
  fn v8__Isolate__NewInGroup(
    group: *const IsolateGroup,
    params: *const raw::CreateParams,
  ) -> *mut RealIsolate;
  fn v8__Isolate__Dispose(this: *mut RealIsolate);
  fn v8__Isolate__GetNumberOfDataSlots(this: *const RealIsolate) -> u32;
  fn v8__Isolate__GetData(
    isolate: *const RealIsolate,
    slot: u32,
  ) -> *mut c_void;
  fn v8__Isolate__SetData(
    isolate: *const RealIsolate,
    slot: u32,
    data: *mut c_void,
  );
  fn v8__Isolate__Enter(this: *mut RealIsolate);
  fn v8__Isolate__Exit(this: *mut RealIsolate);
  fn v8__Global__Reset(data: *const Data);
  fn v8__Isolate__GetCurrent() -> *mut RealIsolate;
  fn v8__Isolate__MemoryPressureNotification(this: *mut RealIsolate, level: u8);
  fn v8__Isolate__ClearKeptObjects(isolate: *mut RealIsolate);
  fn v8__Isolate__LowMemoryNotification(isolate: *mut RealIsolate);
  fn v8__Isolate__ContextDisposedNotification(
    isolate: *mut RealIsolate,
    dependants: ContextDependants,
  );
  fn v8__Isolate__SetIdle(isolate: *mut RealIsolate, is_idle: bool);
  fn v8__CpuProfiler__CollectSample(
    isolate: *mut RealIsolate,
    trace_id: *const u64,
  );
  fn v8__CpuProfiler__UseDetailedSourcePositionsForProfiling(
    isolate: *mut RealIsolate,
  );
  fn v8__Isolate__GetHeapStatistics(
    this: *mut RealIsolate,
    s: *mut v8__HeapStatistics,
  );
  fn v8__Isolate__SetCaptureStackTraceForUncaughtExceptions(
    this: *mut RealIsolate,
    capture: bool,
    frame_limit: i32,
  );
  fn v8__Isolate__AddMessageListener(
    isolate: *mut RealIsolate,
    callback: MessageCallback,
  ) -> bool;
  fn v8__Isolate__AddMessageListenerWithErrorLevel(
    isolate: *mut RealIsolate,
    callback: MessageCallback,
    message_levels: MessageErrorLevel,
  ) -> bool;
  fn v8__Isolate__AddGCPrologueCallback(
    isolate: *mut RealIsolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  );
  fn v8__Isolate__RemoveGCPrologueCallback(
    isolate: *mut RealIsolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
  );
  fn v8__Isolate__AddGCEpilogueCallback(
    isolate: *mut RealIsolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  );
  fn v8__Isolate__RemoveGCEpilogueCallback(
    isolate: *mut RealIsolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
  );
  fn v8__Isolate__NumberOfHeapSpaces(isolate: *mut RealIsolate) -> size_t;
  fn v8__Isolate__GetHeapSpaceStatistics(
    isolate: *mut RealIsolate,
    space_statistics: *mut v8__HeapSpaceStatistics,
    index: size_t,
  ) -> bool;
  fn v8__Isolate__GetHeapCodeAndMetadataStatistics(
    isolate: *mut RealIsolate,
    code_statistics: *mut v8__HeapCodeStatistics,
  ) -> bool;
  fn v8__Isolate__AddNearHeapLimitCallback(
    isolate: *mut RealIsolate,
    callback: NearHeapLimitCallback,
    data: *mut c_void,
  );
  fn v8__Isolate__RemoveNearHeapLimitCallback(
    isolate: *mut RealIsolate,
    callback: NearHeapLimitCallback,
    heap_limit: usize,
  );
  fn v8__Isolate__SetOOMErrorHandler(
    isolate: *mut RealIsolate,
    callback: OomErrorCallback,
  );
  fn v8__Isolate__AdjustAmountOfExternalAllocatedMemory(
    isolate: *mut RealIsolate,
    change_in_bytes: i64,
  ) -> i64;
  fn v8__Isolate__GetCppHeap(isolate: *mut RealIsolate) -> *mut Heap;
  fn v8__Isolate__SetPrepareStackTraceCallback(
    isolate: *mut RealIsolate,
    callback: PrepareStackTraceCallback,
  );
  fn v8__Isolate__SetPromiseHook(isolate: *mut RealIsolate, hook: PromiseHook);
  fn v8__Isolate__SetPromiseRejectCallback(
    isolate: *mut RealIsolate,
    callback: PromiseRejectCallback,
  );
  fn v8__Isolate__SetWasmAsyncResolvePromiseCallback(
    isolate: *mut RealIsolate,
    callback: WasmAsyncResolvePromiseCallback,
  );
  fn v8__Isolate__SetAllowWasmCodeGenerationCallback(
    isolate: *mut RealIsolate,
    callback: AllowWasmCodeGenerationCallback,
  );
  fn v8__Isolate__SetHostInitializeImportMetaObjectCallback(
    isolate: *mut RealIsolate,
    callback: HostInitializeImportMetaObjectCallback,
  );
  fn v8__Isolate__SetHostImportModuleDynamicallyCallback(
    isolate: *mut RealIsolate,
    callback: RawHostImportModuleDynamicallyCallback,
  );
  fn v8__Isolate__SetHostImportModuleWithPhaseDynamicallyCallback(
    isolate: *mut RealIsolate,
    callback: RawHostImportModuleWithPhaseDynamicallyCallback,
  );
  #[cfg(not(target_os = "windows"))]
  fn v8__Isolate__SetHostCreateShadowRealmContextCallback(
    isolate: *mut RealIsolate,
    callback: unsafe extern "C" fn(
      initiator_context: Local<Context>,
    ) -> *mut Context,
  );
  #[cfg(target_os = "windows")]
  fn v8__Isolate__SetHostCreateShadowRealmContextCallback(
    isolate: *mut RealIsolate,
    callback: unsafe extern "C" fn(
      rv: *mut *mut Context,
      initiator_context: Local<Context>,
    ) -> *mut *mut Context,
  );
  fn v8__Isolate__SetUseCounterCallback(
    isolate: *mut RealIsolate,
    callback: UseCounterCallback,
  );
  fn v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
    isolate: *mut RealIsolate,
    callback: RawModifyCodeGenerationFromStringsCallback,
  );
  fn v8__Isolate__RequestInterrupt(
    isolate: *const RealIsolate,
    callback: InterruptCallback,
    data: *mut c_void,
  );
  fn v8__Isolate__TerminateExecution(isolate: *const RealIsolate);
  fn v8__Isolate__IsExecutionTerminating(isolate: *const RealIsolate) -> bool;
  fn v8__Isolate__CancelTerminateExecution(isolate: *const RealIsolate);
  fn v8__Isolate__GetMicrotasksPolicy(
    isolate: *const RealIsolate,
  ) -> MicrotasksPolicy;
  fn v8__Isolate__SetMicrotasksPolicy(
    isolate: *mut RealIsolate,
    policy: MicrotasksPolicy,
  );
  fn v8__Isolate__PerformMicrotaskCheckpoint(isolate: *mut RealIsolate);
  fn v8__Isolate__EnqueueMicrotask(
    isolate: *mut RealIsolate,
    function: *const Function,
  );
  fn v8__Isolate__SetAllowAtomicsWait(isolate: *mut RealIsolate, allow: bool);
  fn v8__Isolate__SetWasmStreamingCallback(
    isolate: *mut RealIsolate,
    callback: unsafe extern "C" fn(*const FunctionCallbackInfo),
  );
  fn v8__Isolate__DateTimeConfigurationChangeNotification(
    isolate: *mut RealIsolate,
    time_zone_detection: TimeZoneDetection,
  );
  fn v8__Isolate__HasPendingBackgroundTasks(
    isolate: *const RealIsolate,
  ) -> bool;
  fn v8__Isolate__RequestGarbageCollectionForTesting(
    isolate: *mut RealIsolate,
    r#type: usize,
  );

  fn v8__HeapProfiler__TakeHeapSnapshot(
    isolate: *mut RealIsolate,
    callback: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> bool,
    arg: *mut c_void,
  );
}

/// Isolate represents an isolated instance of the V8 engine.  V8 isolates have
/// completely separate states.  Objects from one isolate must not be used in
/// other isolates.  The embedder can create multiple isolates and use them in
/// parallel in multiple threads.  An isolate can be entered by at most one
/// thread at any given time.  The Locker/Unlocker API must be used to
/// synchronize.
///
/// rusty_v8 note: Unlike in the C++ API, the Isolate is entered when it is
/// constructed and exited when dropped. Because of that v8::OwnedIsolate
/// instances must be dropped in the reverse order of creation
#[repr(transparent)]
#[derive(Debug)]
pub struct Isolate(NonNull<RealIsolate>);

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct UnsafeRawIsolatePtr(*mut RealIsolate);

impl UnsafeRawIsolatePtr {
  #[inline]
  pub(crate) fn from_real_ptr(ptr: *mut RealIsolate) -> Self {
    Self(ptr)
  }

  pub fn null() -> Self {
    Self(std::ptr::null_mut())
  }

  pub fn is_null(&self) -> bool {
    self.0.is_null()
  }

  #[inline]
  pub(crate) fn as_real_ptr(&self) -> *mut RealIsolate {
    self.0
  }
}

#[repr(C)]
pub struct RealIsolate(Opaque);

impl Isolate {
  pub(crate) fn as_real_ptr(&self) -> *mut RealIsolate {
    self.0.as_ptr()
  }

  pub unsafe fn as_raw_isolate_ptr(&self) -> UnsafeRawIsolatePtr {
    UnsafeRawIsolatePtr(self.0.as_ptr())
  }

  #[inline]
  pub unsafe fn from_raw_isolate_ptr(ptr: UnsafeRawIsolatePtr) -> Self {
    Self(NonNull::new(ptr.0).unwrap())
  }

  #[inline]
  pub unsafe fn from_raw_isolate_ptr_unchecked(
    ptr: UnsafeRawIsolatePtr,
  ) -> Self {
    Self(unsafe { NonNull::new_unchecked(ptr.0) })
  }

  pub unsafe fn from_raw_ptr_unchecked(ptr: *mut RealIsolate) -> Self {
    Self(unsafe { NonNull::new_unchecked(ptr) })
  }

  pub unsafe fn from_raw_ptr(ptr: *mut RealIsolate) -> Self {
    Self(NonNull::new(ptr).unwrap())
  }

  #[inline]
  pub unsafe fn ref_from_raw_isolate_ptr(ptr: &UnsafeRawIsolatePtr) -> &Self {
    if ptr.is_null() {
      panic!("UnsafeRawIsolatePtr is null");
    }
    unsafe { &*(ptr as *const UnsafeRawIsolatePtr as *const Isolate) }
  }

  #[inline]
  pub unsafe fn ref_from_raw_isolate_ptr_unchecked(
    ptr: &UnsafeRawIsolatePtr,
  ) -> &Self {
    unsafe { &*(ptr as *const UnsafeRawIsolatePtr as *const Isolate) }
  }

  #[inline]
  pub unsafe fn ref_from_raw_isolate_ptr_mut(
    ptr: &mut UnsafeRawIsolatePtr,
  ) -> &mut Self {
    if ptr.is_null() {
      panic!("UnsafeRawIsolatePtr is null");
    }
    unsafe { &mut *(ptr as *mut UnsafeRawIsolatePtr as *mut Isolate) }
  }

  #[inline]
  pub unsafe fn ref_from_raw_isolate_ptr_mut_unchecked(
    ptr: &mut UnsafeRawIsolatePtr,
  ) -> &mut Self {
    unsafe { &mut *(ptr as *mut UnsafeRawIsolatePtr as *mut Isolate) }
  }

  #[inline]
  pub(crate) unsafe fn from_non_null(ptr: NonNull<RealIsolate>) -> Self {
    Self(ptr)
  }

  #[inline]
  pub(crate) unsafe fn from_raw_ref(ptr: &NonNull<RealIsolate>) -> &Self {
    // SAFETY: Isolate is a repr(transparent) wrapper around NonNull<RealIsolate>
    unsafe { &*(ptr as *const NonNull<RealIsolate> as *const Isolate) }
  }

  #[inline]
  pub(crate) unsafe fn from_raw_ref_mut(
    ptr: &mut NonNull<RealIsolate>,
  ) -> &mut Self {
    // SAFETY: Isolate is a repr(transparent) wrapper around NonNull<RealIsolate>
    unsafe { &mut *(ptr as *mut NonNull<RealIsolate> as *mut Isolate) }
  }

  // Isolate data slots used internally by rusty_v8.
  const ANNEX_SLOT: u32 = 0;
  const INTERNAL_DATA_SLOT_COUNT: u32 = 1;

  #[inline(always)]
  fn assert_embedder_data_slot_count_and_offset_correct(&self) {
    assert!(
      unsafe { v8__Isolate__GetNumberOfDataSlots(self.as_real_ptr()) }
        >= Self::INTERNAL_DATA_SLOT_COUNT
    )
  }

  fn new_impl(
    group: Option<&IsolateGroup>,
    params: CreateParams,
  ) -> *mut RealIsolate {
    crate::V8::assert_initialized();
    let (raw_create_params, create_param_allocations) = params.finalize();
    let has_embedder_cpp_heap = !raw_create_params.cpp_heap.is_null();
    let cxx_isolate = match group {
      // Calls the group-less V8 overload, which is itself defined as passing
      // the default group, so this only saves materializing an `IsolateGroup`
      // handle on the Rust side. Keeping it means `Isolate::new` reaches V8 by
      // exactly the path it always has.
      None => unsafe { v8__Isolate__New(&raw_create_params) },
      Some(group) => unsafe {
        v8__Isolate__NewInGroup(group, &raw_create_params)
      },
    };
    let mut isolate = unsafe { Isolate::from_raw_ptr(cxx_isolate) };
    isolate.initialize(create_param_allocations);
    isolate.get_annex_mut().has_embedder_cpp_heap = has_embedder_cpp_heap;
    cxx_isolate
  }

  pub(crate) fn initialize(&mut self, create_param_allocations: Box<dyn Any>) {
    self.assert_embedder_data_slot_count_and_offset_correct();
    self.create_annex(create_param_allocations);
  }

  /// Creates a new isolate.  Does not change the currently entered
  /// isolate.
  ///
  /// When an isolate is no longer used its resources should be freed
  /// by calling V8::dispose().  Using the delete operator is not allowed.
  ///
  /// V8::initialize() must have run prior to this.
  #[allow(clippy::new_ret_no_self)]
  pub fn new(params: CreateParams) -> OwnedIsolate {
    OwnedIsolate::new(Self::new_impl(None, params))
  }

  /// Creates a new isolate belonging to the given [`IsolateGroup`]. Does not
  /// change the currently entered isolate.
  ///
  /// Isolates in different groups have separate pointer-compression cages, so
  /// this is how an embedder gets past the 4 GB per-group heap ceiling that
  /// pointer compression otherwise imposes. Pass the same group to several
  /// calls to place those isolates together.
  ///
  /// When an isolate is no longer used its resources should be freed
  /// by calling V8::dispose(). Using the delete operator is not allowed.
  ///
  /// V8::initialize() must have run prior to this.
  #[allow(clippy::new_ret_no_self)]
  pub fn new_with_group(
    group: &IsolateGroup,
    params: CreateParams,
  ) -> OwnedIsolate {
    OwnedIsolate::new(Self::new_impl(Some(group), params))
  }

  #[allow(clippy::new_ret_no_self)]
  pub fn snapshot_creator(
    external_references: Option<Cow<'static, [ExternalReference]>>,
    params: Option<CreateParams>,
  ) -> OwnedIsolate {
    SnapshotCreator::new(external_references, params)
  }

  #[allow(clippy::new_ret_no_self)]
  pub fn snapshot_creator_from_existing_snapshot(
    existing_snapshot_blob: StartupData,
    external_references: Option<Cow<'static, [ExternalReference]>>,
    params: Option<CreateParams>,
  ) -> OwnedIsolate {
    SnapshotCreator::from_existing_snapshot(
      existing_snapshot_blob,
      external_references,
      params,
    )
  }

  /// Initial configuration parameters for a new Isolate.
  #[inline(always)]
  pub fn create_params() -> CreateParams {
    CreateParams::default()
  }

  #[inline(always)]
  pub fn thread_safe_handle(&self) -> IsolateHandle {
    self.get_annex().isolate_handle.clone()
  }

  #[inline(always)]
  pub(crate) fn global_liveness(&self) -> NonNull<IsolateLiveness> {
    self.get_annex().global_liveness
  }

  /// See [`IsolateHandle::terminate_execution`]
  #[inline(always)]
  pub fn terminate_execution(&self) -> bool {
    unsafe { v8__Isolate__TerminateExecution(self.as_real_ptr()) };
    true
  }

  /// See [`IsolateHandle::cancel_terminate_execution`]
  #[inline(always)]
  pub fn cancel_terminate_execution(&self) -> bool {
    unsafe { v8__Isolate__CancelTerminateExecution(self.as_real_ptr()) };
    true
  }

  /// See [`IsolateHandle::is_execution_terminating`]
  #[inline(always)]
  pub fn is_execution_terminating(&self) -> bool {
    unsafe { v8__Isolate__IsExecutionTerminating(self.as_real_ptr()) }
  }

  pub(crate) fn create_annex(
    &mut self,
    create_param_allocations: Box<dyn Any>,
  ) {
    let annex_box = Box::new(IsolateAnnex::new(self, create_param_allocations));
    let annex_ptr = Box::into_raw(annex_box);
    assert!(self.get_data_internal(Self::ANNEX_SLOT).is_null());
    self.set_data_internal(Self::ANNEX_SLOT, annex_ptr as *mut _);
  }

  /// Prepare annex teardown while keeping `ANNEX_SLOT` pointing at the annex.
  ///
  /// Nulls the `IsolateHandle`'s inner pointer, reclaims
  /// `create_param_allocations`, and drops the slot storage. The annex
  /// allocation itself stays alive and `ANNEX_SLOT` keeps pointing at it,
  /// so code that runs during the subsequent V8 teardown GC (weak
  /// callbacks, guaranteed finalizers, embedder slot drops) can still
  /// resolve the annex through `get_annex()` / `get_annex_mut()`.
  ///
  /// The returned pointer must be passed exactly once to
  /// [`Self::finish_annex_dispose`] (or, on the snapshot path, dropped by
  /// [`Self::dispose_annex`]).
  ///
  /// # Safety
  ///
  /// Called once per isolate, from teardown paths only.
  pub(crate) unsafe fn prepare_annex_for_dispose(
    &mut self,
  ) -> (*mut IsolateAnnex, Box<dyn Any>) {
    let annex_ptr =
      self.get_data_internal(Self::ANNEX_SLOT) as *mut IsolateAnnex;
    assert!(!annex_ptr.is_null());

    // Each step below operates through the raw pointer rather than a
    // long-lived `&mut IsolateAnnex`. The borrows we form here are
    // narrowly-scoped expressions that end before any user-controlled
    // Drop runs. This matters because slot Drops, weak callbacks, and
    // guaranteed finalizers may re-enter the isolate (e.g. via
    // `Isolate::thread_safe_handle()`) and resolve the annex through
    // `get_annex()`. An outer `&mut IsolateAnnex` held across that
    // re-entry would alias the shared borrow they obtain.

    // SAFETY: `annex_ptr` is non-null and points at a live `IsolateAnnex`
    // (ANNEX_SLOT is only cleared by code further down this teardown
    // path).
    unsafe {
      // Release Globals dropped by threads that could not touch the
      // isolate, then null the `IsolateHandle` so handles outliving the
      // isolate see a disposed state.
      (*annex_ptr)
        .global_liveness()
        .close_deferred_global_resets();
      (*annex_ptr).global_liveness().dispose();
      (*annex_ptr).isolate_handle.dispose();
    }

    // Reclaim `create_param_allocations` so the caller can keep it alive
    // for as long as V8 needs (during snapshot blob creation, V8 reads
    // external references out of it).
    let create_param_allocations =
      unsafe { (*annex_ptr).create_param_allocations.take().unwrap() };

    // Move slots out before dropping them. A user Drop may re-enter the
    // annex; holding `&mut (*annex_ptr).slots` across that would alias
    // any `&IsolateAnnex` the re-entry obtains.
    let slots = unsafe { std::mem::take(&mut (*annex_ptr).slots) };
    drop(slots);

    (annex_ptr, create_param_allocations)
  }

  /// Drain `finalizer_map` and invoke any guaranteed finalizers.
  ///
  /// New finalizers registered by the running callbacks land in the
  /// annex's now-empty `finalizer_map` and will be picked up by the next
  /// call (currently the second drain in [`OwnedIsolate::drop`] after V8
  /// finishes its teardown GC).
  ///
  /// # Safety
  ///
  /// `annex_ptr` must point at a live `IsolateAnnex` with `ANNEX_SLOT`
  /// still referencing it, so re-entrant callbacks resolve the annex
  /// through normal accessors.
  pub(crate) unsafe fn run_remaining_guaranteed_finalizers(
    annex_ptr: *mut IsolateAnnex,
  ) {
    // Take the map out under a narrow borrow so the for-loop below
    // borrows a local instead of `(*annex_ptr).finalizer_map`. Callbacks
    // re-entering the annex via `get_annex_mut()` would otherwise alias
    // an in-flight `&mut`.
    let mut map = unsafe { std::mem::take(&mut (*annex_ptr).finalizer_map) };
    for finalizer in map.drain() {
      if let FinalizerCallback::Guaranteed(callback) = finalizer {
        callback();
      }
    }
  }

  /// Free the annex allocation after V8's final teardown.
  ///
  /// Drains any guaranteed finalizers V8's teardown GC may have
  /// registered, then drops the annex box. `ANNEX_SLOT` is not cleared
  /// because the isolate is gone — its embedder data storage no longer
  /// exists.
  ///
  /// # Safety
  ///
  /// `annex_ptr` must be the pointer returned from a matching
  /// [`Self::prepare_annex_for_dispose`] call, and the V8 isolate must
  /// already be fully disposed (so no further callbacks can fire).
  pub(crate) unsafe fn finish_annex_dispose(annex_ptr: *mut IsolateAnnex) {
    unsafe { Self::run_remaining_guaranteed_finalizers(annex_ptr) };
    unsafe { drop(Box::from_raw(annex_ptr)) };
  }

  /// Snapshot-path teardown.
  ///
  /// Used by [`OwnedIsolate::create_blob`], which consumes the isolate
  /// before V8 has run its final dispose. Cleans up the annex synchronously
  /// (no weak-callback re-entry to worry about here) and nulls `ANNEX_SLOT`
  /// so the snapshot creator's later isolate-dispose sees a clean slot.
  unsafe fn dispose_annex(&mut self) -> Box<dyn Any> {
    let (annex_ptr, create_param_allocations) =
      unsafe { self.prepare_annex_for_dispose() };
    unsafe { Self::run_remaining_guaranteed_finalizers(annex_ptr) };
    let taken_annex =
      self.take_data_internal(Self::ANNEX_SLOT) as *mut IsolateAnnex;
    debug_assert_eq!(taken_annex, annex_ptr);
    unsafe { drop(Box::from_raw(annex_ptr)) };
    create_param_allocations
  }

  #[inline(always)]
  fn get_annex(&self) -> &IsolateAnnex {
    let annex_ptr =
      self.get_data_internal(Self::ANNEX_SLOT) as *const IsolateAnnex;
    assert!(!annex_ptr.is_null());
    unsafe { &*annex_ptr }
  }

  #[inline(always)]
  fn get_annex_mut(&mut self) -> &mut IsolateAnnex {
    let annex_ptr =
      self.get_data_internal(Self::ANNEX_SLOT) as *mut IsolateAnnex;
    assert!(!annex_ptr.is_null());
    unsafe { &mut *annex_ptr }
  }

  /// Returns a non-null pointer to the isolate's annex data.
  /// This is used by scopes to cache the annex pointer and avoid
  /// repeated FFI calls to `v8__Isolate__GetData`.
  #[inline(always)]
  pub(crate) fn get_annex_ptr(&self) -> NonNull<IsolateAnnex> {
    let annex_ptr =
      self.get_data_internal(Self::ANNEX_SLOT) as *mut IsolateAnnex;
    debug_assert!(!annex_ptr.is_null());
    unsafe { NonNull::new_unchecked(annex_ptr) }
  }

  pub(crate) fn set_snapshot_creator(
    &mut self,
    snapshot_creator: SnapshotCreator,
  ) {
    let prev = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .replace(snapshot_creator);
    assert!(prev.is_none());
  }

  pub(crate) fn get_finalizer_map(&self) -> &FinalizerMap {
    &self.get_annex().finalizer_map
  }

  pub(crate) fn get_finalizer_map_mut(&mut self) -> &mut FinalizerMap {
    &mut self.get_annex_mut().finalizer_map
  }

  pub(crate) fn live_weak_count_mut(&mut self) -> &mut usize {
    &mut self.get_annex_mut().live_weak_count
  }

  /// Release one live-`Weak` count. Saturating, so that a mispaired
  /// `Weak::from_raw` (which is `unsafe` and could hand the same raw
  /// pointer out twice) can't wrap the counter and leave `try_into_shared`
  /// panicking forever about weak handles that don't exist.
  pub(crate) fn release_live_weak(&mut self) {
    let count = self.live_weak_count_mut();
    debug_assert!(*count > 0, "live weak count underflow");
    *count = count.saturating_sub(1);
  }

  /// Retrieve embedder-specific data from the isolate.
  /// Returns NULL if SetData has never been called for the given `slot`.
  pub fn get_data(&self, slot: u32) -> *mut c_void {
    self.get_data_internal(Self::INTERNAL_DATA_SLOT_COUNT + slot)
  }

  /// Associate embedder-specific data with the isolate. `slot` has to be
  /// between 0 and `Isolate::get_number_of_data_slots()`.
  #[inline(always)]
  pub fn set_data(&mut self, slot: u32, data: *mut c_void) {
    self.set_data_internal(Self::INTERNAL_DATA_SLOT_COUNT + slot, data);
  }

  /// Returns the maximum number of available embedder data slots. Valid slots
  /// are in the range of `0 <= n < Isolate::get_number_of_data_slots()`.
  pub fn get_number_of_data_slots(&self) -> u32 {
    let n = unsafe { v8__Isolate__GetNumberOfDataSlots(self.as_real_ptr()) };
    n - Self::INTERNAL_DATA_SLOT_COUNT
  }

  #[inline(always)]
  pub(crate) fn get_data_internal(&self, slot: u32) -> *mut c_void {
    unsafe { v8__Isolate__GetData(self.as_real_ptr(), slot) }
  }

  #[inline(always)]
  pub(crate) fn set_data_internal(&mut self, slot: u32, data: *mut c_void) {
    unsafe { v8__Isolate__SetData(self.as_real_ptr(), slot, data) }
  }

  /// Get the value of the slot and replace it with a null pointer.
  #[inline(always)]
  fn take_data_internal(&mut self, slot: u32) -> *mut c_void {
    let ptr = self.get_data_internal(slot);
    self.set_data_internal(slot, null_mut());
    ptr
  }

  // pub(crate) fn init_scope_root(&mut self) {
  //   ScopeData::new_root(self);
  // }

  // pub(crate) fn dispose_scope_root(&mut self) {
  //   ScopeData::drop_root(self);
  // }

  // /// Returns a pointer to the `ScopeData` struct for the current scope.
  // #[inline(always)]
  // pub(crate) fn get_current_scope_data(&self) -> Option<NonNull<ScopeData>> {
  //   let scope_data_ptr = self.get_data_internal(Self::CURRENT_SCOPE_DATA_SLOT);
  //   NonNull::new(scope_data_ptr).map(NonNull::cast)
  // }

  // /// Updates the slot that stores a `ScopeData` pointer for the current scope.
  // #[inline(always)]
  // pub(crate) fn set_current_scope_data(
  //   &mut self,
  //   scope_data: Option<NonNull<ScopeData>>,
  // ) {
  //   let scope_data_ptr = scope_data
  //     .map(NonNull::cast)
  //     .map_or_else(null_mut, NonNull::as_ptr);
  //   self.set_data_internal(Self::CURRENT_SCOPE_DATA_SLOT, scope_data_ptr);
  // }

  /// Get a reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot<T: 'static>(&self) -> Option<&T> {
    self.get_annex().get_slot::<T>()
  }

  /// Get a mutable reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot_mut<T: 'static>(&mut self) -> Option<&mut T> {
    self.get_annex_mut().get_slot_mut::<T>()
  }

  /// Use with Isolate::get_slot and Isolate::get_slot_mut to associate state
  /// with an Isolate.
  ///
  /// This method gives ownership of value to the Isolate. Exactly one object of
  /// each type can be associated with an Isolate. If called more than once with
  /// an object of the same type, the earlier version will be dropped and
  /// replaced.
  ///
  /// Returns true if value was set without replacing an existing value.
  ///
  /// The value will be dropped when the isolate is dropped.
  #[inline(always)]
  pub fn set_slot<T: 'static>(&mut self, value: T) -> bool {
    self.get_annex_mut().set_slot(value)
  }

  /// Removes the embedder data added with `set_slot()` and returns it if it exists.
  #[inline(always)]
  pub fn remove_slot<T: 'static>(&mut self) -> Option<T> {
    self.get_annex_mut().remove_slot::<T>()
  }

  /// Sets this isolate as the entered one for the current thread.
  /// Saves the previously entered one (if any), so that it can be
  /// restored when exiting.  Re-entering an isolate is allowed.
  ///
  /// rusty_v8 note: Unlike in the C++ API, the isolate is entered when it is
  /// constructed and exited when dropped.
  #[inline(always)]
  pub unsafe fn enter(&self) {
    unsafe {
      v8__Isolate__Enter(self.as_real_ptr());
    }
  }

  /// Exits this isolate by restoring the previously entered one in the
  /// current thread.  The isolate may still stay the same, if it was
  /// entered more than once.
  ///
  /// Requires: self == Isolate::GetCurrent().
  ///
  /// rusty_v8 note: Unlike in the C++ API, the isolate is entered when it is
  /// constructed and exited when dropped.
  #[inline(always)]
  pub unsafe fn exit(&self) {
    unsafe {
      v8__Isolate__Exit(self.as_real_ptr());
    }
  }

  /// Optional notification that the system is running low on memory.
  /// V8 uses these notifications to guide heuristics.
  /// It is allowed to call this function from another thread while
  /// the isolate is executing long running JavaScript code.
  #[inline(always)]
  pub fn memory_pressure_notification(&mut self, level: MemoryPressureLevel) {
    unsafe {
      v8__Isolate__MemoryPressureNotification(self.as_real_ptr(), level as u8)
    }
  }

  /// Optional notification that a context has been disposed. V8 uses these
  /// notifications to guide heuristics on e.g. GC or compilers.
  ///
  /// `dependants` signals whether the disposed context possibly had any
  /// dependants.
  #[inline(always)]
  pub fn context_disposed_notification(
    &mut self,
    dependants: ContextDependants,
  ) {
    unsafe {
      v8__Isolate__ContextDisposedNotification(self.as_real_ptr(), dependants);
    }
  }

  /// Clears the set of objects held strongly by the heap. This set of
  /// objects are originally built when a WeakRef is created or
  /// successfully dereferenced.
  ///
  /// This is invoked automatically after microtasks are run. See
  /// MicrotasksPolicy for when microtasks are run.
  ///
  /// This needs to be manually invoked only if the embedder is manually
  /// running microtasks via a custom MicrotaskQueue class's PerformCheckpoint.
  /// In that case, it is the embedder's responsibility to make this call at a
  /// time which does not interrupt synchronous ECMAScript code execution.
  #[inline(always)]
  pub fn clear_kept_objects(&mut self) {
    unsafe { v8__Isolate__ClearKeptObjects(self.as_real_ptr()) }
  }

  /// Optional notification that the system is running low on memory.
  /// V8 uses these notifications to attempt to free memory.
  #[inline(always)]
  pub fn low_memory_notification(&mut self) {
    unsafe { v8__Isolate__LowMemoryNotification(self.as_real_ptr()) }
  }

  /// Tells the VM whether the embedder is currently idle or not.
  ///
  /// This is consulted by V8's CPU profiler: samples taken while the embedder
  /// is idle (for instance, blocked waiting for I/O in the event loop) are
  /// attributed to the "(idle)" node instead of being counted as running code.
  /// Embedders that don't call this end up reporting ~100% CPU usage in tools
  /// like Chrome DevTools even when the program is doing nothing.
  ///
  /// Must be called on the isolate's own thread while no JavaScript is
  /// executing (e.g. right before parking the event loop, and again with
  /// `false` once it resumes).
  #[inline(always)]
  pub fn set_idle(&mut self, is_idle: bool) {
    unsafe { v8__Isolate__SetIdle(self.as_real_ptr(), is_idle) }
  }

  /// Synchronously collect a CPU profiling sample in all CPU profilers
  /// attached to this isolate. This does not affect the number of ticks
  /// recorded for the current top node.
  ///
  /// When `trace_id` is `Some`, the sample is tagged with that identifier,
  /// which is useful to associate the sample with a trace event.
  #[inline(always)]
  pub fn collect_cpu_profiler_sample(&mut self, trace_id: Option<u64>) {
    let trace_id_ptr = match &trace_id {
      Some(id) => id as *const u64,
      None => std::ptr::null(),
    };
    unsafe { v8__CpuProfiler__CollectSample(self.as_real_ptr(), trace_id_ptr) }
  }

  /// Generate more detailed source positions for code objects. This results in
  /// better accuracy when mapping CPU profiling samples back to script source,
  /// at the cost of some additional memory and CPU overhead.
  #[inline(always)]
  pub fn use_detailed_source_positions_for_profiling(&mut self) {
    unsafe {
      v8__CpuProfiler__UseDetailedSourcePositionsForProfiling(
        self.as_real_ptr(),
      )
    }
  }

  /// Get statistics about the heap memory usage.
  #[inline(always)]
  pub fn get_heap_statistics(&mut self) -> HeapStatistics {
    let inner = unsafe {
      let mut s = MaybeUninit::zeroed();
      v8__Isolate__GetHeapStatistics(self.as_real_ptr(), s.as_mut_ptr());
      s.assume_init()
    };
    HeapStatistics(inner)
  }

  /// Returns the number of spaces in the heap.
  #[inline(always)]
  pub fn number_of_heap_spaces(&mut self) -> usize {
    unsafe { v8__Isolate__NumberOfHeapSpaces(self.as_real_ptr()) }
  }

  /// Get the memory usage of a space in the heap.
  ///
  /// \param space_statistics The HeapSpaceStatistics object to fill in
  ///   statistics.
  /// \param index The index of the space to get statistics from, which ranges
  ///   from 0 to NumberOfHeapSpaces() - 1.
  /// \returns true on success.
  #[inline(always)]
  pub fn get_heap_space_statistics(
    &mut self,
    index: usize,
  ) -> Option<HeapSpaceStatistics> {
    let inner = unsafe {
      let mut s = MaybeUninit::zeroed();
      if !v8__Isolate__GetHeapSpaceStatistics(
        self.as_real_ptr(),
        s.as_mut_ptr(),
        index,
      ) {
        return None;
      }
      s.assume_init()
    };
    Some(HeapSpaceStatistics(inner))
  }

  /// Get code and metadata statistics for the heap.
  ///
  /// \returns true on success.
  #[inline(always)]
  pub fn get_heap_code_and_metadata_statistics(
    &mut self,
  ) -> Option<HeapCodeStatistics> {
    let inner = unsafe {
      let mut s = MaybeUninit::zeroed();
      if !v8__Isolate__GetHeapCodeAndMetadataStatistics(
        self.as_real_ptr(),
        s.as_mut_ptr(),
      ) {
        return None;
      }
      s.assume_init()
    };
    Some(HeapCodeStatistics(inner))
  }

  /// Tells V8 to capture current stack trace when uncaught exception occurs
  /// and report it to the message listeners. The option is off by default.
  #[inline(always)]
  pub fn set_capture_stack_trace_for_uncaught_exceptions(
    &mut self,
    capture: bool,
    frame_limit: i32,
  ) {
    unsafe {
      v8__Isolate__SetCaptureStackTraceForUncaughtExceptions(
        self.as_real_ptr(),
        capture,
        frame_limit,
      );
    }
  }

  /// Adds a message listener (errors only).
  ///
  /// The same message listener can be added more than once and in that
  /// case it will be called more than once for each message.
  ///
  /// The exception object will be passed to the callback.
  #[inline(always)]
  pub fn add_message_listener(&mut self, callback: MessageCallback) -> bool {
    unsafe { v8__Isolate__AddMessageListener(self.as_real_ptr(), callback) }
  }

  /// Adds a message listener for the specified message levels.
  #[inline(always)]
  pub fn add_message_listener_with_error_level(
    &mut self,
    callback: MessageCallback,
    message_levels: MessageErrorLevel,
  ) -> bool {
    unsafe {
      v8__Isolate__AddMessageListenerWithErrorLevel(
        self.as_real_ptr(),
        callback,
        message_levels,
      )
    }
  }

  /// This specifies the callback called when the stack property of Error
  /// is accessed.
  ///
  /// PrepareStackTraceCallback is called when the stack property of an error is
  /// first accessed. The return value will be used as the stack value. If this
  /// callback is registed, the |Error.prepareStackTrace| API will be disabled.
  /// |sites| is an array of call sites, specified in
  /// https://v8.dev/docs/stack-trace-api
  #[inline(always)]
  pub fn set_prepare_stack_trace_callback<'s>(
    &mut self,
    callback: impl MapFnTo<PrepareStackTraceCallback<'s>>,
  ) {
    // Note: the C++ API returns a MaybeLocal but V8 asserts at runtime when
    // it's empty. That is, you can't return None and that's why the Rust API
    // expects Local<Value> instead of Option<Local<Value>>.
    unsafe {
      v8__Isolate__SetPrepareStackTraceCallback(
        self.as_real_ptr(),
        callback.map_fn_to(),
      );
    };
  }

  /// Set the PromiseHook callback for various promise lifecycle
  /// events.
  #[inline(always)]
  pub fn set_promise_hook(&mut self, hook: PromiseHook) {
    unsafe { v8__Isolate__SetPromiseHook(self.as_real_ptr(), hook) }
  }

  /// Set callback to notify about promise reject with no handler, or
  /// revocation of such a previous notification once the handler is added.
  #[inline(always)]
  pub fn set_promise_reject_callback(
    &mut self,
    callback: PromiseRejectCallback,
  ) {
    unsafe {
      v8__Isolate__SetPromiseRejectCallback(self.as_real_ptr(), callback)
    }
  }

  #[inline(always)]
  pub fn set_wasm_async_resolve_promise_callback(
    &mut self,
    callback: WasmAsyncResolvePromiseCallback,
  ) {
    unsafe {
      v8__Isolate__SetWasmAsyncResolvePromiseCallback(
        self.as_real_ptr(),
        callback,
      )
    }
  }

  #[inline(always)]
  pub fn set_allow_wasm_code_generation_callback(
    &mut self,
    callback: AllowWasmCodeGenerationCallback,
  ) {
    unsafe {
      v8__Isolate__SetAllowWasmCodeGenerationCallback(
        self.as_real_ptr(),
        callback,
      );
    }
  }

  #[inline(always)]
  /// This specifies the callback called by the upcoming importa.meta
  /// language feature to retrieve host-defined meta data for a module.
  pub fn set_host_initialize_import_meta_object_callback(
    &mut self,
    callback: HostInitializeImportMetaObjectCallback,
  ) {
    unsafe {
      v8__Isolate__SetHostInitializeImportMetaObjectCallback(
        self.as_real_ptr(),
        callback,
      );
    }
  }

  /// This specifies the callback called by the upcoming dynamic
  /// import() language feature to load modules.
  #[inline(always)]
  pub fn set_host_import_module_dynamically_callback(
    &mut self,
    callback: impl HostImportModuleDynamicallyCallback,
  ) {
    unsafe {
      v8__Isolate__SetHostImportModuleDynamicallyCallback(
        self.as_real_ptr(),
        callback.to_c_fn(),
      );
    }
  }

  /// This specifies the callback called by the upcoming dynamic
  /// import() and import.source() language feature to load modules.
  ///
  /// This API is experimental and is expected to be changed or removed in the
  /// future. The callback is currently only called when for source-phase
  /// imports. Evaluation-phase imports use the existing
  /// HostImportModuleDynamicallyCallback callback.
  #[inline(always)]
  pub fn set_host_import_module_with_phase_dynamically_callback(
    &mut self,
    callback: impl HostImportModuleWithPhaseDynamicallyCallback,
  ) {
    unsafe {
      v8__Isolate__SetHostImportModuleWithPhaseDynamicallyCallback(
        self.as_real_ptr(),
        callback.to_c_fn(),
      );
    }
  }

  /// This specifies the callback called by the upcoming `ShadowRealm`
  /// construction language feature to retrieve host created globals.
  pub fn set_host_create_shadow_realm_context_callback(
    &mut self,
    callback: HostCreateShadowRealmContextCallback,
  ) {
    #[inline]
    unsafe extern "C" fn rust_shadow_realm_callback(
      initiator_context: Local<Context>,
    ) -> *mut Context {
      let scope = pin!(unsafe { CallbackScope::new(initiator_context) });
      let mut scope = scope.init();
      let isolate = scope.as_ref();
      let callback = isolate
        .get_slot::<HostCreateShadowRealmContextCallback>()
        .unwrap();
      let context = callback(&mut scope);
      context.map_or_else(null_mut, |l| l.as_non_null().as_ptr())
    }

    // Windows x64 ABI: MaybeLocal<Context> must be returned on the stack.
    #[cfg(target_os = "windows")]
    unsafe extern "C" fn rust_shadow_realm_callback_windows(
      rv: *mut *mut Context,
      initiator_context: Local<Context>,
    ) -> *mut *mut Context {
      unsafe {
        let ret = rust_shadow_realm_callback(initiator_context);
        rv.write(ret);
      }
      rv
    }

    let slot_didnt_exist_before = self.set_slot(callback);
    if slot_didnt_exist_before {
      unsafe {
        #[cfg(target_os = "windows")]
        v8__Isolate__SetHostCreateShadowRealmContextCallback(
          self.as_real_ptr(),
          rust_shadow_realm_callback_windows,
        );
        #[cfg(not(target_os = "windows"))]
        v8__Isolate__SetHostCreateShadowRealmContextCallback(
          self.as_real_ptr(),
          rust_shadow_realm_callback,
        );
      }
    }
  }

  /// Sets a callback for counting the number of times a feature of V8 is used.
  #[inline(always)]
  pub fn set_use_counter_callback(&mut self, callback: UseCounterCallback) {
    unsafe {
      v8__Isolate__SetUseCounterCallback(self.as_real_ptr(), callback);
    }
  }

  /// Enables the host application to receive a notification before a
  /// garbage collection. Allocations are allowed in the callback function,
  /// but the callback is not re-entrant: if the allocation inside it will
  /// trigger the garbage collection, the callback won't be called again.
  /// It is possible to specify the GCType filter for your callback. But it is
  /// not possible to register the same callback function two times with
  /// different GCType filters.
  #[allow(clippy::not_unsafe_ptr_arg_deref)] // False positive.
  #[inline(always)]
  pub fn add_gc_prologue_callback(
    &mut self,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  ) {
    unsafe {
      v8__Isolate__AddGCPrologueCallback(
        self.as_real_ptr(),
        callback,
        data,
        gc_type_filter,
      );
    }
  }

  /// This function removes callback which was installed by
  /// AddGCPrologueCallback function.
  #[allow(clippy::not_unsafe_ptr_arg_deref)] // False positive.
  #[inline(always)]
  pub fn remove_gc_prologue_callback(
    &mut self,
    callback: GcCallbackWithData,
    data: *mut c_void,
  ) {
    unsafe {
      v8__Isolate__RemoveGCPrologueCallback(self.as_real_ptr(), callback, data)
    }
  }

  /// Enables the host application to receive a notification after a
  /// garbage collection.
  #[allow(clippy::not_unsafe_ptr_arg_deref)] // False positive.
  #[inline(always)]
  pub fn add_gc_epilogue_callback(
    &mut self,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  ) {
    unsafe {
      v8__Isolate__AddGCEpilogueCallback(
        self.as_real_ptr(),
        callback,
        data,
        gc_type_filter,
      );
    }
  }

  /// This function removes a callback which was added by
  /// `AddGCEpilogueCallback`.
  #[allow(clippy::not_unsafe_ptr_arg_deref)] // False positive.
  #[inline(always)]
  pub fn remove_gc_epilogue_callback(
    &mut self,
    callback: GcCallbackWithData,
    data: *mut c_void,
  ) {
    unsafe {
      v8__Isolate__RemoveGCEpilogueCallback(self.as_real_ptr(), callback, data)
    }
  }

  /// This specifies the callback called by V8 when JS is trying to dynamically
  /// execute code using `eval` or the `Function` constructor.
  ///
  /// The callback can decide whether to allow code generation and, if so,
  /// modify the source code beforehand. V8 skips it only when the context
  /// allows code generation from strings (see
  /// [`Context::set_allow_generation_from_strings`]) *and* the source is
  /// already a string; see [`ModifyCodeGenerationFromStringsCallback`] for the
  /// exact conditions.
  ///
  /// Calling this again replaces the previously installed callback.
  pub fn set_modify_code_generation_from_strings_callback(
    &mut self,
    callback: ModifyCodeGenerationFromStringsCallback,
  ) {
    unsafe extern "C" fn rust_modify_code_generation_callback<'s>(
      context: Local<'s, Context>,
      source: Local<'s, Value>,
      is_code_like: bool,
    ) -> RawModifyCodeGenerationFromStringsResult {
      let scope = pin!(unsafe { CallbackScope::new(context) });
      let mut scope = scope.init();
      let callback = *scope
        .as_ref()
        .get_slot::<ModifyCodeGenerationFromStringsCallback>()
        .unwrap();
      let result = callback(&mut scope, source, is_code_like);
      // `modified_source` outlives `scope` even though the raw handle slot is
      // read here and only handed to V8 once `scope` has been dropped. A
      // `CallbackScope` built from a `Local<Context>` has `NEEDS_SCOPE ==
      // false`, so it opens no `HandleScope` of its own and handles created by
      // the callback belong to whichever scope V8 had active when it called
      // us. If that ever changes, this needs an `EscapableHandleScope`.
      RawModifyCodeGenerationFromStringsResult {
        codegen_allowed: result.codegen_allowed,
        modified_source: result
          .modified_source
          .map_or_else(null_mut, |s| s.as_non_null().as_ptr()),
      }
    }

    let slot_didnt_exist_before = self.set_slot(callback);
    if slot_didnt_exist_before {
      unsafe {
        v8__Isolate__SetModifyCodeGenerationFromStringsCallback(
          self.as_real_ptr(),
          rust_modify_code_generation_callback,
        );
      }
    }
  }

  /// Add a callback to invoke in case the heap size is close to the heap limit.
  /// If multiple callbacks are added, only the most recently added callback is
  /// invoked.
  #[allow(clippy::not_unsafe_ptr_arg_deref)] // False positive.
  #[inline(always)]
  pub fn add_near_heap_limit_callback(
    &mut self,
    callback: NearHeapLimitCallback,
    data: *mut c_void,
  ) {
    unsafe {
      v8__Isolate__AddNearHeapLimitCallback(self.as_real_ptr(), callback, data)
    };
  }

  /// Remove the given callback and restore the heap limit to the given limit.
  /// If the given limit is zero, then it is ignored. If the current heap size
  /// is greater than the given limit, then the heap limit is restored to the
  /// minimal limit that is possible for the current heap size.
  #[inline(always)]
  pub fn remove_near_heap_limit_callback(
    &mut self,
    callback: NearHeapLimitCallback,
    heap_limit: usize,
  ) {
    unsafe {
      v8__Isolate__RemoveNearHeapLimitCallback(
        self.as_real_ptr(),
        callback,
        heap_limit,
      );
    };
  }

  /// Adjusts the amount of registered external memory. Used to give V8 an
  /// indication of the amount of externally allocated memory that is kept
  /// alive by JavaScript objects. V8 uses this to decide when to perform
  /// global garbage collections. Registering externally allocated memory
  /// will trigger global garbage collections more often than it would
  /// otherwise in an attempt to garbage collect the JavaScript objects
  /// that keep the externally allocated memory alive.
  #[inline(always)]
  pub fn adjust_amount_of_external_allocated_memory(
    &mut self,
    change_in_bytes: i64,
  ) -> i64 {
    unsafe {
      v8__Isolate__AdjustAmountOfExternalAllocatedMemory(
        self.as_real_ptr(),
        change_in_bytes,
      )
    }
  }

  #[inline(always)]
  pub fn get_cpp_heap(&mut self) -> Option<&Heap> {
    unsafe { v8__Isolate__GetCppHeap(self.as_real_ptr()).as_ref() }
  }

  #[inline(always)]
  pub fn set_oom_error_handler(&mut self, callback: OomErrorCallback) {
    unsafe { v8__Isolate__SetOOMErrorHandler(self.as_real_ptr(), callback) };
  }

  /// Returns the policy controlling how Microtasks are invoked.
  #[inline(always)]
  pub fn get_microtasks_policy(&self) -> MicrotasksPolicy {
    unsafe { v8__Isolate__GetMicrotasksPolicy(self.as_real_ptr()) }
  }

  /// Returns the policy controlling how Microtasks are invoked.
  #[inline(always)]
  pub fn set_microtasks_policy(&mut self, policy: MicrotasksPolicy) {
    unsafe { v8__Isolate__SetMicrotasksPolicy(self.as_real_ptr(), policy) }
  }

  /// Runs the default MicrotaskQueue until it gets empty and perform other
  /// microtask checkpoint steps, such as calling ClearKeptObjects. Asserts that
  /// the MicrotasksPolicy is not kScoped. Any exceptions thrown by microtask
  /// callbacks are swallowed.
  #[inline(always)]
  pub fn perform_microtask_checkpoint(&mut self) {
    unsafe { v8__Isolate__PerformMicrotaskCheckpoint(self.as_real_ptr()) }
  }

  /// Enqueues the callback to the default MicrotaskQueue
  #[inline(always)]
  pub fn enqueue_microtask(&mut self, microtask: Local<Function>) {
    crate::handle::assert_locals_in_isolate(self);
    unsafe { v8__Isolate__EnqueueMicrotask(self.as_real_ptr(), &*microtask) }
  }

  /// Set whether calling Atomics.wait (a function that may block) is allowed in
  /// this isolate. This can also be configured via
  /// CreateParams::allow_atomics_wait.
  #[inline(always)]
  pub fn set_allow_atomics_wait(&mut self, allow: bool) {
    unsafe { v8__Isolate__SetAllowAtomicsWait(self.as_real_ptr(), allow) }
  }

  /// Embedder injection point for `WebAssembly.compileStreaming(source)`.
  /// The expectation is that the embedder sets it at most once.
  ///
  /// The callback receives the source argument (string, Promise, etc.)
  /// and an instance of [WasmStreaming]. The [WasmStreaming] instance
  /// can outlive the callback and is used to feed data chunks to V8
  /// asynchronously.
  #[inline(always)]
  pub fn set_wasm_streaming_callback<F>(&mut self, _: F)
  where
    F: UnitType
      + for<'a, 'b, 'c> Fn(
        &'c mut PinScope<'a, 'b>,
        Local<'a, Value>,
        WasmStreaming<false>,
      ),
  {
    unsafe {
      v8__Isolate__SetWasmStreamingCallback(
        self.as_real_ptr(),
        trampoline::<F>(),
      )
    }
  }

  /// Notification that the embedder has changed the time zone, daylight savings
  /// time or other date / time configuration parameters. V8 keeps a cache of
  /// various values used for date / time computation. This notification will
  /// reset those cached values for the current context so that date / time
  /// configuration changes would be reflected.
  ///
  /// This API should not be called more than needed as it will negatively impact
  /// the performance of date operations.
  #[inline(always)]
  pub fn date_time_configuration_change_notification(
    &mut self,
    time_zone_detection: TimeZoneDetection,
  ) {
    unsafe {
      v8__Isolate__DateTimeConfigurationChangeNotification(
        self.as_real_ptr(),
        time_zone_detection,
      );
    }
  }

  /// Returns true if there is ongoing background work within V8 that will
  /// eventually post a foreground task, like asynchronous WebAssembly
  /// compilation.
  #[inline(always)]
  pub fn has_pending_background_tasks(&self) -> bool {
    unsafe { v8__Isolate__HasPendingBackgroundTasks(self.as_real_ptr()) }
  }

  /// Request garbage collection with a specific embedderstack state in this
  /// Isolate. It is only valid to call this function if --expose_gc was
  /// specified.
  ///
  /// This should only be used for testing purposes and not to enforce a garbage
  /// collection schedule. It has strong negative impact on the garbage
  /// collection performance. Use IdleNotificationDeadline() or
  /// LowMemoryNotification() instead to influence the garbage collection
  /// schedule.
  #[inline(always)]
  pub fn request_garbage_collection_for_testing(
    &mut self,
    r#type: GarbageCollectionType,
  ) {
    unsafe {
      v8__Isolate__RequestGarbageCollectionForTesting(
        self.as_real_ptr(),
        match r#type {
          GarbageCollectionType::Full => 0,
          GarbageCollectionType::Minor => 1,
        },
      );
    }
  }

  /// Disposes the isolate.  The isolate must not be entered by any
  /// thread to be disposable.
  pub(crate) unsafe fn dispose(&mut self) {
    // No test case in rusty_v8 show this, but there have been situations in
    // deno where dropping Annex before the states causes a segfault.
    unsafe {
      v8__Isolate__Dispose(self.as_real_ptr());
    }
  }

  /// Take a heap snapshot. The callback is invoked one or more times
  /// with byte slices containing the snapshot serialized as JSON.
  /// It's the callback's responsibility to reassemble them into
  /// a single document, e.g., by writing them to a file.
  /// Note that Chrome DevTools refuses to load snapshots without
  /// a .heapsnapshot suffix.
  pub fn take_heap_snapshot<F>(&mut self, mut callback: F)
  where
    F: FnMut(&[u8]) -> bool,
  {
    unsafe extern "C" fn trampoline<F>(
      arg: *mut c_void,
      data: *const u8,
      size: usize,
    ) -> bool
    where
      F: FnMut(&[u8]) -> bool,
    {
      unsafe {
        let mut callback = NonNull::<F>::new_unchecked(arg as _);
        if size > 0 {
          (callback.as_mut())(std::slice::from_raw_parts(data, size))
        } else {
          (callback.as_mut())(&[])
        }
      }
    }

    let arg = addr_of_mut!(callback);
    unsafe {
      v8__HeapProfiler__TakeHeapSnapshot(
        self.as_real_ptr(),
        trampoline::<F>,
        arg as _,
      );
    }
  }

  /// Set the default context to be included in the snapshot blob.
  /// The snapshot will not contain the global proxy, and we expect one or a
  /// global object template to create one, to be provided upon deserialization.
  ///
  /// # Panics
  ///
  /// Panics if the isolate was not created using [`Isolate::snapshot_creator`]
  #[inline(always)]
  pub fn set_default_context(&mut self, context: Local<Context>) {
    crate::handle::assert_locals_in_isolate(self);
    let snapshot_creator = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .as_mut()
      .unwrap();
    snapshot_creator.set_default_context(context);
  }

  /// Add additional context to be included in the snapshot blob.
  /// The snapshot will include the global proxy.
  ///
  /// Returns the index of the context in the snapshot blob.
  ///
  /// # Panics
  ///
  /// Panics if the isolate was not created using [`Isolate::snapshot_creator`]
  #[inline(always)]
  pub fn add_context(&mut self, context: Local<Context>) -> usize {
    crate::handle::assert_locals_in_isolate(self);
    let snapshot_creator = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .as_mut()
      .unwrap();
    snapshot_creator.add_context(context)
  }

  /// Attach arbitrary `v8::Data` to the isolate snapshot, which can be
  /// retrieved via `HandleScope::get_context_data_from_snapshot_once()` after
  /// deserialization. This data does not survive when a new snapshot is created
  /// from an existing snapshot.
  ///
  /// # Panics
  ///
  /// Panics if the isolate was not created using [`Isolate::snapshot_creator`]
  #[inline(always)]
  pub fn add_isolate_data<T>(&mut self, data: Local<T>) -> usize
  where
    for<'l> Local<'l, T>: Into<Local<'l, Data>>,
  {
    crate::handle::assert_locals_in_isolate(self);
    let snapshot_creator = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .as_mut()
      .unwrap();
    snapshot_creator.add_isolate_data(data)
  }

  /// Attach arbitrary `v8::Data` to the context snapshot, which can be
  /// retrieved via `HandleScope::get_context_data_from_snapshot_once()` after
  /// deserialization. This data does not survive when a new snapshot is
  /// created from an existing snapshot.
  ///
  /// # Panics
  ///
  /// Panics if the isolate was not created using [`Isolate::snapshot_creator`]
  #[inline(always)]
  pub fn add_context_data<T>(
    &mut self,
    context: Local<Context>,
    data: Local<T>,
  ) -> usize
  where
    for<'l> Local<'l, T>: Into<Local<'l, Data>>,
  {
    crate::handle::assert_locals_in_isolate(self);
    let snapshot_creator = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .as_mut()
      .unwrap();
    snapshot_creator.add_context_data(context, data)
  }
}

pub(crate) struct IsolateAnnex {
  // Wrapped in `Option` so teardown can `take()` it through a `&mut
  // IsolateAnnex` without having to move ownership of the whole annex out
  // of `ANNEX_SLOT`. Only `prepare_annex_for_dispose` consumes it; that
  // function runs at most once per annex, so the `unwrap()` there can
  // never observe a `None`.
  create_param_allocations: Option<Box<dyn Any>>,
  slots: HashMap<TypeId, RawSlot, BuildTypeIdHasher>,
  finalizer_map: FinalizerMap,
  maybe_snapshot_creator: Option<SnapshotCreator>,
  isolate_handle: IsolateHandle,
  global_liveness: NonNull<IsolateLiveness>,
  /// Number of live `Weak` handles, maintained so `try_into_shared()` can
  /// reject isolates with outstanding weaks (their GC callbacks are not
  /// thread-safe against the owning `Weak`).
  live_weak_count: usize,
  /// Whether an embedder cppgc heap was passed via `CreateParams`. V8
  /// attaches a default cppgc heap of its own, so `GetCppHeap()` can't
  /// distinguish; `try_into_shared()` rejects only embedder heaps.
  has_embedder_cpp_heap: bool,
}

impl IsolateAnnex {
  fn new(isolate: &Isolate, create_param_allocations: Box<dyn Any>) -> Self {
    // Globals may be dropped after their host isolate is disposed. Keep this
    // tiny liveness cell valid so those late drops can observe the null isolate
    // pointer without retaining an Arc per Global.
    let global_liveness = Box::leak(Box::new(IsolateLiveness::new(isolate)));
    Self {
      create_param_allocations: Some(create_param_allocations),
      slots: HashMap::default(),
      finalizer_map: FinalizerMap::default(),
      maybe_snapshot_creator: None,
      isolate_handle: IsolateHandle::new(isolate),
      global_liveness: NonNull::from(global_liveness),
      live_weak_count: 0,
      has_embedder_cpp_heap: false,
    }
  }

  #[inline(always)]
  fn global_liveness(&self) -> &IsolateLiveness {
    unsafe { self.global_liveness.as_ref() }
  }

  #[inline(always)]
  pub(crate) fn get_slot<T: 'static>(&self) -> Option<&T> {
    self
      .slots
      .get(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.borrow::<T>() })
  }

  #[inline(always)]
  pub(crate) fn get_slot_mut<T: 'static>(&mut self) -> Option<&mut T> {
    self
      .slots
      .get_mut(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.borrow_mut::<T>() })
  }

  #[inline(always)]
  pub(crate) fn set_slot<T: 'static>(&mut self, value: T) -> bool {
    self
      .slots
      .insert(TypeId::of::<T>(), RawSlot::new(value))
      .is_none()
  }

  #[inline(always)]
  pub(crate) fn remove_slot<T: 'static>(&mut self) -> Option<T> {
    self
      .slots
      .remove(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.into_inner::<T>() })
  }
}

/// A unique obligation to reset one V8 global-handle cell.
///
/// The cell is stable until it is reset or its isolate is disposed. Moving the
/// pointer between threads is safe, but resetting it is only safe while the
/// destination thread holds the isolate's `v8::Locker`.
struct DeferredGlobalReset(NonNull<Data>);

// SAFETY: This token is only created by `Global::drop`; that `Global` uniquely
// owns the cell and cannot use it again. The token is moved through a
// mutex-protected queue and consumed only by a thread holding the V8 lock.
unsafe impl Send for DeferredGlobalReset {}

impl DeferredGlobalReset {
  fn reset(self) {
    unsafe { v8__Global__Reset(self.0.as_ptr()) };
  }
}

// Zero is reserved for a thread whose ID has not been requested yet. IDs are
// never reused; abort before wrapping rather than risk confusing two threads.
static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! {
  // Const-initialized and destructor-free, so this remains readable while
  // other TLS values are being destroyed. `Global::clone`/`drop` hit this on
  // every call.
  static CURRENT_THREAD_ID: std::cell::Cell<usize> =
    const { std::cell::Cell::new(0) };
}

#[inline(always)]
fn current_thread_id() -> usize {
  CURRENT_THREAD_ID.with(|current| {
    let id = current.get();
    if id != 0 {
      return id;
    }

    // This cold path deliberately avoids `std::thread::current()`: it may run
    // from another TLS destructor, where panicking would abort the process.
    // A relaxed global counter is sufficient because IDs are only compared.
    let id = NEXT_THREAD_ID
      .fetch_update(
        std::sync::atomic::Ordering::Relaxed,
        std::sync::atomic::Ordering::Relaxed,
        |next| next.checked_add(1),
      )
      .unwrap_or_else(|_| std::process::abort());
    current.set(id);
    id
  })
}

pub(crate) struct IsolateLiveness {
  isolate: AtomicPtr<RealIsolate>,
  /// The thread the isolate was created on. Meaningful only while the
  /// isolate is not shared: a non-shared isolate never leaves it.
  home_thread: usize,
  shared: std::sync::atomic::AtomicBool,
  /// V8 cells whose Rust `Global` owners were dropped by threads that could
  /// not touch the isolate. Drained at the next checkpoint: lock and unlock
  /// boundaries for shared isolates, and `Global` creation or home-thread
  /// `Global` drops for all isolates. `None` after teardown's final drain.
  deferred_global_resets: Mutex<Option<Vec<DeferredGlobalReset>>>,
  /// Length hint for `deferred_global_resets`, so drain checkpoints are a
  /// relaxed load in the common (empty) case instead of a mutex
  /// acquisition. Updated under the mutex.
  deferred_len: std::sync::atomic::AtomicUsize,
}

impl IsolateLiveness {
  #[inline(always)]
  fn new(isolate: &Isolate) -> Self {
    Self {
      isolate: AtomicPtr::new(isolate.as_real_ptr()),
      home_thread: current_thread_id(),
      shared: std::sync::atomic::AtomicBool::new(false),
      deferred_global_resets: Mutex::new(Some(Vec::new())),
      deferred_len: std::sync::atomic::AtomicUsize::new(0),
    }
  }

  #[inline(always)]
  fn dispose(&self) {
    self
      .isolate
      .store(null_mut(), std::sync::atomic::Ordering::Relaxed);
  }

  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut RealIsolate {
    self.isolate.load(std::sync::atomic::Ordering::Relaxed)
  }

  pub(crate) fn mark_shared(&self) {
    assert!(
      self.on_home_thread(),
      "an isolate must be converted to SharedIsolate on its home thread"
    );
    // Release/Acquire so a thread that observes `shared == false` (and
    // takes the immediate-reset path in `Global::drop`) cannot do so
    // after the isolate has started migrating between threads.
    self
      .shared
      .store(true, std::sync::atomic::Ordering::Release);
  }

  #[inline(always)]
  pub(crate) fn is_shared(&self) -> bool {
    self.shared.load(std::sync::atomic::Ordering::Acquire)
  }

  /// The one gate for touching a `Global`'s V8 cell. Covers both cases:
  /// a shared isolate needs its `Locker` held here, a non-shared one
  /// needs the home thread. It has to be checked for non-shared isolates
  /// too — `Global` is `Send`, so a plain `OwnedIsolate` can stay on
  /// thread A while one of its `Global`s is used on thread B, racing A's
  /// GC.
  pub(crate) fn assert_access_allowed(&self) {
    if !self.is_shared() {
      // A non-shared OwnedIsolate cannot leave its home thread through safe
      // Rust. Being on that thread therefore also excludes racing teardown,
      // so the hot path needs no mutex.
      assert!(
        !self.get_isolate_ptr().is_null(),
        "attempt to access Handle hosted by disposed Isolate"
      );
      assert!(
        self.on_home_thread(),
        "accessing a Global requires being on its isolate's thread"
      );
      return;
    }

    // Sample liveness while holding the reset queue: teardown closes the
    // queue under this mutex before disposing. If this thread holds the V8
    // lock, that lock prevents teardown after the mutex is released and for
    // the duration of the caller's access.
    let q = self.deferred_global_resets.lock().unwrap();
    let isolate_is_live = q.is_some() && !self.get_isolate_ptr().is_null();
    let allowed = isolate_is_live && self.on_isolate_thread();
    drop(q);
    assert!(
      isolate_is_live,
      "attempt to access Handle hosted by disposed Isolate"
    );
    assert!(
      allowed,
      "accessing a Global belonging to a shared isolate requires holding \
       its Locker on the current thread"
    );
  }

  /// True when the current thread may touch the isolate's handle
  /// storage: it holds the isolate's `v8::Locker` (shared), or it is the
  /// isolate's home thread (non-shared).
  #[inline(always)]
  pub(crate) fn on_home_thread(&self) -> bool {
    current_thread_id() == self.home_thread
  }

  pub(crate) fn on_isolate_thread(&self) -> bool {
    if self.is_shared() {
      let isolate = self.get_isolate_ptr();
      // A disposed isolate cannot be locked by anyone.
      !isolate.is_null() && crate::locker::thread_holds_lock(isolate)
    } else {
      self.on_home_thread()
    }
  }

  /// Release a `Global`'s cell now if the current thread may touch the
  /// isolate; otherwise queue it for the next lock acquisition (shared)
  /// or isolate teardown (non-shared).
  ///
  /// While the queue is `Some`, teardown has not started: its final drain
  /// closes the queue under this mutex before the isolate is disposed, so
  /// the isolate pointer read below stays valid for the duration of this
  /// call. The null check is belt and braces on top of that invariant: a
  /// disposed isolate means the cell is already gone.
  pub(crate) fn reset_or_defer_global(&self, data: NonNull<Data>) {
    let mut q = self.deferred_global_resets.lock().unwrap();
    let Some(v) = q.as_mut() else { return };
    if self.get_isolate_ptr().is_null() {
      return;
    }
    if self.on_isolate_thread() {
      drop(q);
      DeferredGlobalReset(data).reset();
    } else {
      v.push(DeferredGlobalReset(data));
      self
        .deferred_len
        .store(v.len(), std::sync::atomic::Ordering::Relaxed);
    }
  }

  /// Cheap drain checkpoint: a relaxed load when the queue is empty.
  /// The caller must be allowed to touch the isolate (home thread, or
  /// holding the lock of a shared isolate).
  #[inline(always)]
  pub(crate) fn maybe_drain_deferred_global_resets(&self) {
    if self.deferred_len.load(std::sync::atomic::Ordering::Relaxed) > 0 {
      self.drain_deferred_global_resets();
    }
  }

  /// Called while allowed to touch the isolate.
  pub(crate) fn drain_deferred_global_resets(&self) {
    let drained = {
      let mut q = self.deferred_global_resets.lock().unwrap();
      self
        .deferred_len
        .store(0, std::sync::atomic::Ordering::Relaxed);
      match q.as_mut() {
        Some(v) => std::mem::take(v),
        None => return,
      }
    };
    for reset in drained {
      reset.reset();
    }
  }

  /// Final drain during teardown, with the isolate's lock held. Late
  /// droppers that arrive after this see a closed queue and do nothing;
  /// their cells are freed with the isolate.
  pub(crate) fn close_deferred_global_resets(&self) {
    let drained = {
      let mut q = self.deferred_global_resets.lock().unwrap();
      self
        .deferred_len
        .store(0, std::sync::atomic::Ordering::Relaxed);
      q.take()
    };
    for reset in drained.into_iter().flatten() {
      reset.reset();
    }
  }
}

impl Debug for IsolateAnnex {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("IsolateAnnex")
      .field("isolate_handle", &self.isolate_handle)
      .finish()
  }
}

pub(crate) struct IsolateHandleInner {
  /// Safety invariants:
  /// - The 'main thread' must lock the mutex and reset `isolate` to null just
  ///   before the isolate is disposed.
  /// - Any other thread must lock the mutex while it's reading/using the
  ///   `isolate` pointer.
  // These two fields can be replaced with a `Mutex<*mut RealIsolate>` once
  // `Mutex::data_ptr()` is stabilized.
  isolate: UnsafeCell<*mut RealIsolate>,
  isolate_mutex: Mutex<()>,
}

unsafe impl Send for IsolateHandleInner {}
unsafe impl Sync for IsolateHandleInner {}

/// IsolateHandle is a thread-safe reference to an Isolate. Its main use is to
/// terminate execution of a running isolate from another thread.
///
/// It is created with [`Isolate::thread_safe_handle()`].
///
/// IsolateHandle is Cloneable, Send, and Sync.
#[derive(Clone)]
pub struct IsolateHandle(Arc<IsolateHandleInner>);

impl fmt::Debug for IsolateHandle {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    if let Ok(_lock) = self.0.isolate_mutex.try_lock() {
      // SAFETY: mutex lock is held
      let ptr = unsafe { *self.0.isolate.get() };
      f.debug_struct("IsolateHandle")
        .field("isolate_ptr", &ptr)
        .finish()
    } else {
      f.debug_struct("IsolateHandle").finish_non_exhaustive()
    }
  }
}

impl IsolateHandle {
  #[inline(always)]
  fn new(isolate: &Isolate) -> Self {
    let inner = Arc::new(IsolateHandleInner {
      isolate: UnsafeCell::new(isolate.as_real_ptr()),
      isolate_mutex: Mutex::new(()),
    });
    Self(inner)
  }

  /// Set the inner isolate pointer to null.
  fn dispose(&self) {
    let _lock = self.0.isolate_mutex.lock().unwrap();
    // SAFETY: mutex lock is held
    unsafe { *self.0.isolate.get() = null_mut() }
  }

  /// Access the isolate, if it hasn't yet been disposed of.
  ///
  /// A lock is taken on the pointer and held for the scope of `f`, which
  /// means the isolate can't be dropped until after `f` returns. If you
  /// do something with the isolate afterwards, that needs to be verified
  /// to be safe separately.
  pub(crate) fn with_isolate_ptr<R>(
    &self,
    f: impl FnOnce(NonNull<RealIsolate>) -> R,
  ) -> Option<R> {
    let _lock = self.0.isolate_mutex.lock().unwrap();
    // SAFETY: mutex lock is held
    let ptr = unsafe { *self.0.isolate.get() };
    NonNull::new(ptr).map(f)
  }

  /// Access the pointer for this isolate - it may be null.
  ///
  /// # Safety
  /// This function must only be called from the main thread associated with
  /// the V8 isolate.
  // TODO: have this return an `Option<NonNull<RealIsolate>>`
  pub(crate) unsafe fn get_isolate_ptr(&self) -> *mut RealIsolate {
    // SAFETY: this function must only be called from the main thread of the
    // isolate. On that thread, the caller cannot race with teardown code that
    // sets this pointer to null.
    unsafe { *self.0.isolate.get() }
  }

  /// Forcefully terminate the current thread of JavaScript execution
  /// in the given isolate.
  ///
  /// This method can be used by any thread even if that thread has not
  /// acquired the V8 lock with a Locker object.
  ///
  /// Returns false if Isolate was already destroyed.
  #[inline(always)]
  pub fn terminate_execution(&self) -> bool {
    self
      .with_isolate_ptr(|isolate| unsafe {
        v8__Isolate__TerminateExecution(isolate.as_ptr())
      })
      .is_some()
  }

  /// Resume execution capability in the given isolate, whose execution
  /// was previously forcefully terminated using TerminateExecution().
  ///
  /// When execution is forcefully terminated using TerminateExecution(),
  /// the isolate can not resume execution until all JavaScript frames
  /// have propagated the uncatchable exception which is generated.  This
  /// method allows the program embedding the engine to handle the
  /// termination event and resume execution capability, even if
  /// JavaScript frames remain on the stack.
  ///
  /// This method can be used by any thread even if that thread has not
  /// acquired the V8 lock with a Locker object.
  ///
  /// Returns false if Isolate was already destroyed.
  #[inline(always)]
  pub fn cancel_terminate_execution(&self) -> bool {
    self
      .with_isolate_ptr(|isolate| unsafe {
        v8__Isolate__CancelTerminateExecution(isolate.as_ptr())
      })
      .is_some()
  }

  /// Is V8 terminating JavaScript execution.
  ///
  /// Returns true if JavaScript execution is currently terminating
  /// because of a call to TerminateExecution.  In that case there are
  /// still JavaScript frames on the stack and the termination
  /// exception is still active.
  ///
  /// Returns false if Isolate was already destroyed.
  #[inline(always)]
  pub fn is_execution_terminating(&self) -> bool {
    self
      .with_isolate_ptr(|isolate| unsafe {
        v8__Isolate__IsExecutionTerminating(isolate.as_ptr())
      })
      .unwrap_or(false)
  }

  /// Request V8 to interrupt long running JavaScript code and invoke
  /// the given |callback| passing the given |data| to it. After |callback|
  /// returns control will be returned to the JavaScript code.
  /// There may be a number of interrupt requests in flight.
  /// Can be called from another thread without acquiring a |Locker|.
  /// Registered |callback| must not reenter interrupted Isolate.
  ///
  /// Returns false if Isolate was already destroyed.
  // Clippy warns that this method is dereferencing a raw pointer, but it is
  // not: https://github.com/rust-lang/rust-clippy/issues/3045
  #[allow(clippy::not_unsafe_ptr_arg_deref)]
  #[inline(always)]
  pub fn request_interrupt(
    &self,
    callback: InterruptCallback,
    data: *mut c_void,
  ) -> bool {
    self
      .with_isolate_ptr(|isolate| unsafe {
        v8__Isolate__RequestInterrupt(isolate.as_ptr(), callback, data)
      })
      .is_some()
  }
}

/// Same as Isolate but gets disposed when it goes out of scope.
#[derive(Debug)]
pub struct OwnedIsolate {
  cxx_isolate: NonNull<RealIsolate>,
}

/// The reason an [`OwnedIsolate`] could not be converted into a
/// [`crate::SharedIsolate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IntoSharedErrorKind {
  SnapshotCreator,
  LiveWeakHandlesOrPendingFinalizers,
  EmbedderCppHeap,
  AnotherIsolateEntered,
}

impl fmt::Display for IntoSharedErrorKind {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    let message = match self {
      Self::SnapshotCreator => "snapshot-creator isolates cannot be shared",
      Self::LiveWeakHandlesOrPendingFinalizers => {
        "isolates with live v8::Weak handles or pending finalizers cannot be shared"
      }
      Self::EmbedderCppHeap => {
        "isolates with an embedder cppgc heap cannot be shared"
      }
      Self::AnotherIsolateEntered => {
        "try_into_shared() must be called with no other isolate entered on top of this one"
      }
    };
    f.write_str(message)
  }
}

/// A failed shared-isolate conversion.
///
/// The original isolate is retained so the rejected state can be cleaned up
/// and conversion retried, or a snapshot creator can still produce its blob.
/// Call [`Self::into_isolate`] to recover it. Dropping this error without
/// recovering the isolate leaks it, because some rejected states (notably a
/// snapshot creator or an isolate below another entered isolate) cannot run
/// [`OwnedIsolate::drop`] safely at that point.
#[derive(Debug)]
pub struct IntoSharedError {
  kind: IntoSharedErrorKind,
  isolate: Option<OwnedIsolate>,
}

impl IntoSharedError {
  pub fn kind(&self) -> IntoSharedErrorKind {
    self.kind
  }

  pub fn into_isolate(mut self) -> OwnedIsolate {
    self.isolate.take().unwrap()
  }
}

impl Drop for IntoSharedError {
  fn drop(&mut self) {
    if let Some(isolate) = self.isolate.take() {
      // `OwnedIsolate::drop` may itself panic for the state represented by this
      // error. Leaking is the only generally safe default; callers that can fix
      // the rejected state recover ownership with `into_isolate()`.
      forget(isolate);
    }
  }
}

impl fmt::Display for IntoSharedError {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    fmt::Display::fmt(&self.kind, f)
  }
}

impl std::error::Error for IntoSharedError {}

impl OwnedIsolate {
  pub(crate) fn new(cxx_isolate: *mut RealIsolate) -> Self {
    let isolate = Self::new_already_entered(cxx_isolate);
    unsafe {
      isolate.enter();
    }
    isolate
  }

  pub(crate) fn new_already_entered(cxx_isolate: *mut RealIsolate) -> Self {
    let cxx_isolate = NonNull::new(cxx_isolate).unwrap();
    let owned_isolate: OwnedIsolate = Self { cxx_isolate };
    // owned_isolate.init_scope_root();
    owned_isolate
  }

  /// Try to convert this isolate into a [`crate::SharedIsolate`] that can be
  /// locked and used from any thread. The isolate is exited on the
  /// current thread; all further access goes through
  /// [`crate::SharedIsolate::lock`].
  ///
  /// # Safety
  ///
  /// A shared isolate migrates between threads together with everything
  /// attached to it, without `Send` bounds the type system can check.
  /// The caller must ensure that all embedder state hanging off the
  /// isolate is `Send`: isolate slot values ([`Isolate::set_slot`]),
  /// embedder data ([`Isolate::set_data`]), callbacks and their captured
  /// state, and the allocations referenced by its `CreateParams`. All of
  /// it may be accessed and eventually dropped on whichever thread holds
  /// the lock or drops the `SharedIsolate`.
  ///
  /// This obligation covers the isolate's whole remaining lifetime, not
  /// just the state present at this call: anything attached later
  /// through a [`crate::Locker`] (e.g. `set_slot` under the lock) must
  /// be `Send` too — nothing checks it at insertion time.
  ///
  /// The `OwnedIsolate` must still be on the thread where it was created.
  /// This is guaranteed by its `!Send` type in safe Rust; callers that
  /// reconstruct or transfer one through unsafe code must preserve that
  /// invariant.
  ///
  /// No reference obtained through [`crate::Global::open`] may still be live
  /// when this method is called. After sharing, opening a Global is unsafe and
  /// its reference must not outlive the [`crate::Locker`] under which it was
  /// opened or cross threads. Prefer creating a [`crate::Local`] under a handle
  /// scope. Other access to a Global belonging to this isolate, including
  /// cloning, hashing, and comparing, requires holding its Locker. Dropping one
  /// is the sole exception and may happen on any thread.
  ///
  /// # Errors
  ///
  /// Returns the original isolate together with the rejection reason if this is
  /// a snapshot-creator isolate, if it has live
  /// [`crate::Weak`] handles or pending finalizers, if it has a cppgc
  /// heap attached, or if another isolate is entered on top of this one
  /// on the current thread.
  ///
  /// # Panics
  ///
  /// Panics if the isolate is no longer on the thread where it was created.
  pub unsafe fn try_into_shared(
    self,
  ) -> Result<crate::SharedIsolate, IntoSharedError> {
    let error_kind = if unsafe {
      !std::ptr::eq(self.cxx_isolate.as_ptr(), v8__Isolate__GetCurrent())
    } {
      Some(IntoSharedErrorKind::AnotherIsolateEntered)
    } else {
      let annex = self.get_annex();
      if annex.maybe_snapshot_creator.is_some() {
        Some(IntoSharedErrorKind::SnapshotCreator)
      } else if annex.live_weak_count != 0 || !annex.finalizer_map.is_empty() {
        Some(IntoSharedErrorKind::LiveWeakHandlesOrPendingFinalizers)
      } else if annex.has_embedder_cpp_heap {
        Some(IntoSharedErrorKind::EmbedderCppHeap)
      } else {
        None
      }
    };

    if let Some(kind) = error_kind {
      return Err(IntoSharedError {
        kind,
        isolate: Some(self),
      });
    }

    unsafe {
      let isolate_handle = self.thread_safe_handle();
      self.global_liveness().as_ref().mark_shared();
      self.exit();
      let cxx_isolate = self.cxx_isolate;
      forget(self);
      Ok(crate::SharedIsolate::new(cxx_isolate, isolate_handle))
    }
  }
}

impl Drop for OwnedIsolate {
  fn drop(&mut self) {
    unsafe {
      let snapshot_creator = self.get_annex_mut().maybe_snapshot_creator.take();
      assert!(
        snapshot_creator.is_none(),
        "If isolate was created using v8::Isolate::snapshot_creator, you should use v8::OwnedIsolate::create_blob before dropping an isolate."
      );
      // Safety: We need to check `this == Isolate::GetCurrent()` before calling exit()
      assert!(
        std::ptr::eq(self.cxx_isolate.as_mut(), v8__Isolate__GetCurrent()),
        "v8::OwnedIsolate instances must be dropped in the reverse order of creation. They are entered upon creation and exited upon being dropped."
      );
      // self.dispose_scope_root();
      self.exit();
      let (annex_ptr, _create_param_allocations) =
        self.prepare_annex_for_dispose();
      // Drain finalizers registered up to this point, before V8's final
      // teardown GC has a chance to fire weak callbacks that need the
      // annex.
      Isolate::run_remaining_guaranteed_finalizers(annex_ptr);
      Platform::notify_isolate_shutdown(&get_current_platform(), self);
      // V8's final teardown runs here. `ANNEX_SLOT` still references the
      // (drained) annex, so any re-entrant access from weak callbacks or
      // embedder code resolves normally instead of panicking on a null
      // slot.
      self.dispose();
      // Drain finalizers V8 may have registered during teardown, then free
      // the annex allocation. V8 has fully disposed the isolate, so its
      // embedder data storage no longer exists and `ANNEX_SLOT` needs no
      // explicit clearing.
      Isolate::finish_annex_dispose(annex_ptr);
    }
  }
}

impl OwnedIsolate {
  /// Creates a snapshot data blob.
  /// This must not be called from within a handle scope.
  ///
  /// # Panics
  ///
  /// Panics if the isolate was not created using [`Isolate::snapshot_creator`]
  #[inline(always)]
  pub fn create_blob(
    mut self,
    function_code_handling: FunctionCodeHandling,
  ) -> Option<StartupData> {
    let mut snapshot_creator =
      self.get_annex_mut().maybe_snapshot_creator.take().unwrap();

    // create_param_allocations is needed during CreateBlob
    // so v8 can read external references
    let _create_param_allocations = unsafe {
      // self.dispose_scope_root();
      self.dispose_annex()
    };

    // The isolate is owned by the snapshot creator; we need to forget it
    // here as the snapshot creator will drop it when running the destructor.
    std::mem::forget(self);
    snapshot_creator.create_blob(function_code_handling)
  }
}

impl Deref for OwnedIsolate {
  type Target = Isolate;
  fn deref(&self) -> &Self::Target {
    unsafe {
      std::mem::transmute::<&NonNull<RealIsolate>, &Isolate>(&self.cxx_isolate)
    }
  }
}

impl DerefMut for OwnedIsolate {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe {
      std::mem::transmute::<&mut NonNull<RealIsolate>, &mut Isolate>(
        &mut self.cxx_isolate,
      )
    }
  }
}

impl AsMut<Isolate> for OwnedIsolate {
  fn as_mut(&mut self) -> &mut Isolate {
    self
  }
}

impl AsMut<Isolate> for Isolate {
  fn as_mut(&mut self) -> &mut Isolate {
    self
  }
}

/// Collection of V8 heap information.
///
/// Instances of this class can be passed to v8::Isolate::GetHeapStatistics to
/// get heap statistics from V8.
pub struct HeapStatistics(v8__HeapStatistics);

impl HeapStatistics {
  #[inline(always)]
  pub fn total_heap_size(&self) -> usize {
    self.0.total_heap_size_
  }

  #[inline(always)]
  pub fn total_heap_size_executable(&self) -> usize {
    self.0.total_heap_size_executable_
  }

  #[inline(always)]
  pub fn total_physical_size(&self) -> usize {
    self.0.total_physical_size_
  }

  #[inline(always)]
  pub fn total_available_size(&self) -> usize {
    self.0.total_available_size_
  }

  #[inline(always)]
  pub fn total_global_handles_size(&self) -> usize {
    self.0.total_global_handles_size_
  }

  #[inline(always)]
  pub fn used_global_handles_size(&self) -> usize {
    self.0.used_global_handles_size_
  }

  #[inline(always)]
  pub fn used_heap_size(&self) -> usize {
    self.0.used_heap_size_
  }

  #[inline(always)]
  pub fn heap_size_limit(&self) -> usize {
    self.0.heap_size_limit_
  }

  #[inline(always)]
  pub fn malloced_memory(&self) -> usize {
    self.0.malloced_memory_
  }

  #[inline(always)]
  pub fn external_memory(&self) -> usize {
    self.0.external_memory_
  }

  #[inline(always)]
  pub fn peak_malloced_memory(&self) -> usize {
    self.0.peak_malloced_memory_
  }

  #[inline(always)]
  pub fn number_of_native_contexts(&self) -> usize {
    self.0.number_of_native_contexts_
  }

  #[inline(always)]
  pub fn number_of_detached_contexts(&self) -> usize {
    self.0.number_of_detached_contexts_
  }

  /// Returns the total number of bytes allocated since the Isolate was created.
  /// This includes all heap objects allocated in any space (new, old, code,
  /// etc.).
  #[inline(always)]
  pub fn total_allocated_bytes(&self) -> u64 {
    self.0.total_allocated_bytes_
  }

  /// Returns a 0/1 boolean, which signifies whether the V8 overwrite heap
  /// garbage with a bit pattern.
  #[inline(always)]
  pub fn does_zap_garbage(&self) -> bool {
    self.0.does_zap_garbage_
  }
}

pub struct HeapSpaceStatistics(v8__HeapSpaceStatistics);

impl HeapSpaceStatistics {
  pub fn space_name(&self) -> &'static CStr {
    unsafe { CStr::from_ptr(self.0.space_name_) }
  }

  pub fn space_size(&self) -> usize {
    self.0.space_size_
  }

  pub fn space_used_size(&self) -> usize {
    self.0.space_used_size_
  }

  pub fn space_available_size(&self) -> usize {
    self.0.space_available_size_
  }

  pub fn physical_space_size(&self) -> usize {
    self.0.physical_space_size_
  }
}

pub struct HeapCodeStatistics(v8__HeapCodeStatistics);

impl HeapCodeStatistics {
  pub fn code_and_metadata_size(&self) -> usize {
    self.0.code_and_metadata_size_
  }

  pub fn bytecode_and_metadata_size(&self) -> usize {
    self.0.bytecode_and_metadata_size_
  }

  pub fn external_script_source_size(&self) -> usize {
    self.0.external_script_source_size_
  }

  pub fn cpu_profiler_metadata_size(&self) -> usize {
    self.0.cpu_profiler_metadata_size_
  }
}

impl<'s, F> MapFnFrom<F> for PrepareStackTraceCallback<'s>
where
  F: UnitType
    + for<'a> Fn(
      &mut PinScope<'s, 'a>,
      Local<'s, Value>,
      Local<'s, Array>,
    ) -> Local<'s, Value>,
{
  // Windows x64 ABI: MaybeLocal<Value> returned on the stack.
  #[cfg(target_os = "windows")]
  fn mapping() -> Self {
    let f = |ret_ptr, context, error, sites| {
      let scope = pin!(unsafe { CallbackScope::new(context) });
      let mut scope: crate::PinnedRef<CallbackScope> = scope.init();
      let r = (F::get())(&mut scope, error, sites);
      unsafe { std::ptr::write(ret_ptr, &*r as *const _) };
      ret_ptr
    };
    f.to_c_fn()
  }

  // System V ABI
  #[cfg(not(target_os = "windows"))]
  fn mapping() -> Self {
    let f = |context, error, sites| {
      let scope = pin!(unsafe { CallbackScope::new(context) });
      let mut scope: crate::PinnedRef<CallbackScope> = scope.init();

      let r = (F::get())(&mut scope, error, sites);
      PrepareStackTraceCallbackRet(&*r as *const _)
    };
    f.to_c_fn()
  }
}

/// A special hasher that is optimized for hashing `std::any::TypeId` values.
/// `TypeId` values are actually 64-bit values which themselves come out of some
/// hash function, so it's unnecessary to shuffle their bits any further.
#[derive(Clone, Default)]
pub(crate) struct TypeIdHasher {
  state: Option<u64>,
}

impl Hasher for TypeIdHasher {
  fn write(&mut self, _bytes: &[u8]) {
    panic!("TypeIdHasher::write() called unexpectedly");
  }

  #[inline]
  fn write_u64(&mut self, value: u64) {
    // The internal hash function of TypeId only takes the bottom 64-bits, even on versions
    // of Rust that use a 128-bit TypeId.
    let prev_state = self.state.replace(value);
    debug_assert_eq!(prev_state, None);
  }

  #[inline]
  fn finish(&self) -> u64 {
    self.state.unwrap()
  }
}

/// Factory for instances of `TypeIdHasher`. This is the type that one would
/// pass to the constructor of some map/set type in order to make it use
/// `TypeIdHasher` instead of the default hasher implementation.
#[derive(Copy, Clone, Default)]
pub(crate) struct BuildTypeIdHasher;

impl BuildHasher for BuildTypeIdHasher {
  type Hasher = TypeIdHasher;

  #[inline]
  fn build_hasher(&self) -> Self::Hasher {
    Default::default()
  }
}

const _: () = {
  assert!(
    size_of::<TypeId>() == size_of::<u64>()
      || size_of::<TypeId>() == size_of::<u128>()
  );
  assert!(
    align_of::<TypeId>() == align_of::<u64>()
      || align_of::<TypeId>() == align_of::<u128>()
  );
};

pub(crate) struct RawSlot {
  data: RawSlotData,
  dtor: Option<RawSlotDtor>,
}

type RawSlotData = MaybeUninit<usize>;
type RawSlotDtor = unsafe fn(&mut RawSlotData) -> ();

impl RawSlot {
  #[inline]
  pub fn new<T: 'static>(value: T) -> Self {
    if Self::needs_box::<T>() {
      Self::new_internal(Box::new(value))
    } else {
      Self::new_internal(value)
    }
  }

  // SAFETY: a valid value of type `T` must haven been stored in the slot
  // earlier. There is no verification that the type param provided by the
  // caller is correct.
  #[inline]
  pub unsafe fn borrow<T: 'static>(&self) -> &T {
    unsafe {
      if Self::needs_box::<T>() {
        &*(self.data.as_ptr() as *const Box<T>)
      } else {
        &*(self.data.as_ptr() as *const T)
      }
    }
  }

  // Safety: see [`RawSlot::borrow`].
  #[inline]
  pub unsafe fn borrow_mut<T: 'static>(&mut self) -> &mut T {
    unsafe {
      if Self::needs_box::<T>() {
        &mut *(self.data.as_mut_ptr() as *mut Box<T>)
      } else {
        &mut *(self.data.as_mut_ptr() as *mut T)
      }
    }
  }

  // Safety: see [`RawSlot::borrow`].
  #[inline]
  pub unsafe fn into_inner<T: 'static>(self) -> T {
    unsafe {
      let value = if Self::needs_box::<T>() {
        *std::ptr::read(self.data.as_ptr() as *mut Box<T>)
      } else {
        std::ptr::read(self.data.as_ptr() as *mut T)
      };
      forget(self);
      value
    }
  }

  const fn needs_box<T: 'static>() -> bool {
    size_of::<T>() > size_of::<RawSlotData>()
      || align_of::<T>() > align_of::<RawSlotData>()
  }

  #[inline]
  fn new_internal<B: 'static>(value: B) -> Self {
    assert!(!Self::needs_box::<B>());
    let mut self_ = Self {
      data: RawSlotData::zeroed(),
      dtor: None,
    };
    unsafe {
      ptr::write(self_.data.as_mut_ptr() as *mut B, value);
    }
    if needs_drop::<B>() {
      self_.dtor.replace(Self::drop_internal::<B>);
    };
    self_
  }

  // SAFETY: a valid value of type `T` or `Box<T>` must be stored in the slot.
  unsafe fn drop_internal<B: 'static>(data: &mut RawSlotData) {
    assert!(!Self::needs_box::<B>());
    unsafe {
      drop_in_place(data.as_mut_ptr() as *mut B);
    }
  }
}

impl Drop for RawSlot {
  fn drop(&mut self) {
    if let Some(dtor) = self.dtor {
      unsafe { dtor(&mut self.data) };
    }
  }
}

impl AsRef<Isolate> for OwnedIsolate {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.cxx_isolate) }
  }
}
impl AsRef<Isolate> for Isolate {
  fn as_ref(&self) -> &Isolate {
    self
  }
}
