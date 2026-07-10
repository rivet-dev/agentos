// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::Array;
use crate::CallbackScope;
use crate::Context;
use crate::Data;
use crate::ExternalReferences;
use crate::FixedArray;
use crate::Function;
use crate::FunctionCodeHandling;
use crate::HandleScope;
use crate::Local;
use crate::Message;
use crate::Module;
use crate::Object;
use crate::Promise;
use crate::PromiseResolver;
use crate::StartupData;
use crate::String;
use crate::Value;
use crate::binding::v8__HeapSpaceStatistics;
use crate::binding::v8__HeapStatistics;
use crate::binding::v8__Isolate__UseCounterFeature;
pub use crate::binding::v8__ModuleImportPhase as ModuleImportPhase;
use crate::cppgc::Heap;
use crate::function::FunctionCallbackInfo;
use crate::gc::GCCallbackFlags;
use crate::gc::GCType;
use crate::handle::FinalizerCallback;
use crate::handle::FinalizerMap;
use crate::isolate_create_params::CreateParams;
use crate::isolate_create_params::raw;
use crate::promise::PromiseRejectMessage;
use crate::scope::data::ScopeData;
use crate::snapshot::SnapshotCreator;
use crate::support::Allocated;
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
use std::ffi::CStr;

use std::any::Any;
use std::any::TypeId;
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
use std::ptr;
use std::ptr::NonNull;
use std::ptr::addr_of_mut;
use std::ptr::drop_in_place;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::Mutex;

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

/// Memory pressure level for the MemoryPressureNotification.
/// None hints V8 that there is no memory pressure.
/// Moderate hints V8 to speed up incremental garbage collection at the cost
/// of higher latency due to garbage collection pauses.
/// Critical hints V8 to free memory as soon as possible. Garbage collection
/// pauses at this level will be large.
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
  *mut Isolate,
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
  + for<'s> FnOnce(
    &mut HandleScope<'s>,
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

#[cfg(all(target_family = "windows", target_arch = "x86_64"))]
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
    + for<'s> FnOnce(
      &mut HandleScope<'s>,
      Local<'s, Data>,
      Local<'s, Value>,
      Local<'s, String>,
      Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>>,
{
  #[inline(always)]
  fn to_c_fn(self) -> RawHostImportModuleDynamicallyCallback {
    #[inline(always)]
    fn scope_adapter<'s, F: HostImportModuleDynamicallyCallback>(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_attributes: Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>> {
      let scope = &mut unsafe { CallbackScope::new(context) };
      (F::get())(
        scope,
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

    #[cfg(all(target_family = "windows", target_arch = "x86_64"))]
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
  + for<'s> FnOnce(
    &mut HandleScope<'s>,
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

#[cfg(all(target_family = "windows", target_arch = "x86_64"))]
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
    + for<'s> FnOnce(
      &mut HandleScope<'s>,
      Local<'s, Data>,
      Local<'s, Value>,
      Local<'s, String>,
      ModuleImportPhase,
      Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>>,
{
  #[inline(always)]
  fn to_c_fn(self) -> RawHostImportModuleWithPhaseDynamicallyCallback {
    #[inline(always)]
    fn scope_adapter<'s, F: HostImportModuleWithPhaseDynamicallyCallback>(
      context: Local<'s, Context>,
      host_defined_options: Local<'s, Data>,
      resource_name: Local<'s, Value>,
      specifier: Local<'s, String>,
      import_phase: ModuleImportPhase,
      import_attributes: Local<'s, FixedArray>,
    ) -> Option<Local<'s, Promise>> {
      let scope = &mut unsafe { CallbackScope::new(context) };
      (F::get())(
        scope,
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

    #[cfg(all(target_family = "windows", target_arch = "x86_64"))]
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
  for<'s> fn(scope: &mut HandleScope<'s>) -> Option<Local<'s, Context>>;

pub type GcCallbackWithData = unsafe extern "C" fn(
  isolate: *mut Isolate,
  r#type: GCType,
  flags: GCCallbackFlags,
  data: *mut c_void,
);

pub type InterruptCallback =
  unsafe extern "C" fn(isolate: &mut Isolate, data: *mut c_void);

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

// Windows x64 ABI: MaybeLocal<Value> returned on the stack.
#[cfg(target_os = "windows")]
pub type PrepareStackTraceCallback<'s> =
  unsafe extern "C" fn(
    *mut *const Value,
    Local<'s, Context>,
    Local<'s, Value>,
    Local<'s, Array>,
  ) -> *mut *const Value;

// System V ABI: MaybeLocal<Value> returned in a register.
// System V i386 ABI: Local<Value> returned in hidden pointer (struct).
#[cfg(not(target_os = "windows"))]
#[repr(C)]
pub struct PrepareStackTraceCallbackRet(*const Value);

#[cfg(not(target_os = "windows"))]
pub type PrepareStackTraceCallback<'s> =
  unsafe extern "C" fn(
    Local<'s, Context>,
    Local<'s, Value>,
    Local<'s, Array>,
  ) -> PrepareStackTraceCallbackRet;

pub type UseCounterFeature = v8__Isolate__UseCounterFeature;
pub type UseCounterCallback =
  unsafe extern "C" fn(&mut Isolate, UseCounterFeature);

unsafe extern "C" {
  fn v8__Isolate__New(params: *const raw::CreateParams) -> *mut Isolate;
  fn v8__Isolate__Dispose(this: *mut Isolate);
  fn v8__Isolate__GetNumberOfDataSlots(this: *const Isolate) -> u32;
  fn v8__Isolate__GetData(isolate: *const Isolate, slot: u32) -> *mut c_void;
  fn v8__Isolate__SetData(
    isolate: *const Isolate,
    slot: u32,
    data: *mut c_void,
  );
  fn v8__Isolate__Enter(this: *mut Isolate);
  fn v8__Isolate__Exit(this: *mut Isolate);
  fn v8__Isolate__GetCurrent() -> *mut Isolate;
  fn v8__Isolate__MemoryPressureNotification(this: *mut Isolate, level: u8);
  fn v8__Isolate__ClearKeptObjects(isolate: *mut Isolate);
  fn v8__Isolate__LowMemoryNotification(isolate: *mut Isolate);
  fn v8__Isolate__GetHeapStatistics(
    this: *mut Isolate,
    s: *mut v8__HeapStatistics,
  );
  fn v8__Isolate__SetCaptureStackTraceForUncaughtExceptions(
    this: *mut Isolate,
    capture: bool,
    frame_limit: i32,
  );
  fn v8__Isolate__AddMessageListener(
    isolate: *mut Isolate,
    callback: MessageCallback,
  ) -> bool;
  fn v8__Isolate__AddMessageListenerWithErrorLevel(
    isolate: *mut Isolate,
    callback: MessageCallback,
    message_levels: MessageErrorLevel,
  ) -> bool;
  fn v8__Isolate__AddGCPrologueCallback(
    isolate: *mut Isolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  );
  fn v8__Isolate__RemoveGCPrologueCallback(
    isolate: *mut Isolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
  );
  fn v8__Isolate__AddGCEpilogueCallback(
    isolate: *mut Isolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
    gc_type_filter: GCType,
  );
  fn v8__Isolate__RemoveGCEpilogueCallback(
    isolate: *mut Isolate,
    callback: GcCallbackWithData,
    data: *mut c_void,
  );
  fn v8__Isolate__NumberOfHeapSpaces(isolate: *mut Isolate) -> size_t;
  fn v8__Isolate__GetHeapSpaceStatistics(
    isolate: *mut Isolate,
    space_statistics: *mut v8__HeapSpaceStatistics,
    index: size_t,
  ) -> bool;
  fn v8__Isolate__AddNearHeapLimitCallback(
    isolate: *mut Isolate,
    callback: NearHeapLimitCallback,
    data: *mut c_void,
  );
  fn v8__Isolate__RemoveNearHeapLimitCallback(
    isolate: *mut Isolate,
    callback: NearHeapLimitCallback,
    heap_limit: usize,
  );
  fn v8__Isolate__SetOOMErrorHandler(
    isolate: *mut Isolate,
    callback: OomErrorCallback,
  );
  fn v8__Isolate__AdjustAmountOfExternalAllocatedMemory(
    isolate: *mut Isolate,
    change_in_bytes: i64,
  ) -> i64;
  fn v8__Isolate__GetCppHeap(isolate: *mut Isolate) -> *mut Heap;
  fn v8__Isolate__SetPrepareStackTraceCallback(
    isolate: *mut Isolate,
    callback: PrepareStackTraceCallback,
  );
  fn v8__Isolate__SetPromiseHook(isolate: *mut Isolate, hook: PromiseHook);
  fn v8__Isolate__SetPromiseRejectCallback(
    isolate: *mut Isolate,
    callback: PromiseRejectCallback,
  );
  fn v8__Isolate__SetWasmAsyncResolvePromiseCallback(
    isolate: *mut Isolate,
    callback: WasmAsyncResolvePromiseCallback,
  );
  fn v8__Isolate__SetAllowWasmCodeGenerationCallback(
    isolate: *mut Isolate,
    callback: AllowWasmCodeGenerationCallback,
  );
  fn v8__Isolate__SetHostInitializeImportMetaObjectCallback(
    isolate: *mut Isolate,
    callback: HostInitializeImportMetaObjectCallback,
  );
  fn v8__Isolate__SetHostImportModuleDynamicallyCallback(
    isolate: *mut Isolate,
    callback: RawHostImportModuleDynamicallyCallback,
  );
  fn v8__Isolate__SetHostImportModuleWithPhaseDynamicallyCallback(
    isolate: *mut Isolate,
    callback: RawHostImportModuleWithPhaseDynamicallyCallback,
  );
  #[cfg(not(target_os = "windows"))]
  fn v8__Isolate__SetHostCreateShadowRealmContextCallback(
    isolate: *mut Isolate,
    callback: unsafe extern "C" fn(
      initiator_context: Local<Context>,
    ) -> *mut Context,
  );
  #[cfg(target_os = "windows")]
  fn v8__Isolate__SetHostCreateShadowRealmContextCallback(
    isolate: *mut Isolate,
    callback: unsafe extern "C" fn(
      rv: *mut *mut Context,
      initiator_context: Local<Context>,
    ) -> *mut *mut Context,
  );
  fn v8__Isolate__SetUseCounterCallback(
    isolate: *mut Isolate,
    callback: UseCounterCallback,
  );
  fn v8__Isolate__RequestInterrupt(
    isolate: *const Isolate,
    callback: InterruptCallback,
    data: *mut c_void,
  );
  fn v8__Isolate__TerminateExecution(isolate: *const Isolate);
  fn v8__Isolate__IsExecutionTerminating(isolate: *const Isolate) -> bool;
  fn v8__Isolate__CancelTerminateExecution(isolate: *const Isolate);
  fn v8__Isolate__GetMicrotasksPolicy(
    isolate: *const Isolate,
  ) -> MicrotasksPolicy;
  fn v8__Isolate__SetMicrotasksPolicy(
    isolate: *mut Isolate,
    policy: MicrotasksPolicy,
  );
  fn v8__Isolate__PerformMicrotaskCheckpoint(isolate: *mut Isolate);
  fn v8__Isolate__EnqueueMicrotask(
    isolate: *mut Isolate,
    function: *const Function,
  );
  fn v8__Isolate__SetAllowAtomicsWait(isolate: *mut Isolate, allow: bool);
  fn v8__Isolate__SetWasmStreamingCallback(
    isolate: *mut Isolate,
    callback: unsafe extern "C" fn(*const FunctionCallbackInfo),
  );
  fn v8__Isolate__DateTimeConfigurationChangeNotification(
    isolate: *mut Isolate,
    time_zone_detection: TimeZoneDetection,
  );
  fn v8__Isolate__HasPendingBackgroundTasks(isolate: *const Isolate) -> bool;
  fn v8__Isolate__RequestGarbageCollectionForTesting(
    isolate: *mut Isolate,
    r#type: usize,
  );

  fn v8__HeapProfiler__TakeHeapSnapshot(
    isolate: *mut Isolate,
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
#[repr(C)]
#[derive(Debug)]
pub struct Isolate(Opaque);

impl Isolate {
  // Isolate data slots used internally by rusty_v8.
  const ANNEX_SLOT: u32 = 0;
  const CURRENT_SCOPE_DATA_SLOT: u32 = 1;
  const INTERNAL_DATA_SLOT_COUNT: u32 = 2;

  #[inline(always)]
  fn assert_embedder_data_slot_count_and_offset_correct(&self) {
    assert!(
      unsafe { v8__Isolate__GetNumberOfDataSlots(self) }
        >= Self::INTERNAL_DATA_SLOT_COUNT
    )
  }

  fn new_impl(params: CreateParams) -> *mut Isolate {
    crate::V8::assert_initialized();
    let (raw_create_params, create_param_allocations) = params.finalize();
    let cxx_isolate = unsafe { v8__Isolate__New(&raw_create_params) };
    let isolate = unsafe { &mut *cxx_isolate };
    isolate.initialize(create_param_allocations);
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
    OwnedIsolate::new(Self::new_impl(params))
  }

  #[allow(clippy::new_ret_no_self)]
  pub fn snapshot_creator(
    external_references: Option<&'static ExternalReferences>,
    params: Option<CreateParams>,
  ) -> OwnedIsolate {
    SnapshotCreator::new(external_references, params)
  }

  #[allow(clippy::new_ret_no_self)]
  pub fn snapshot_creator_from_existing_snapshot(
    existing_snapshot_blob: impl Allocated<[u8]>,
    external_references: Option<&'static ExternalReferences>,
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
    IsolateHandle::new(self)
  }

  /// See [`IsolateHandle::terminate_execution`]
  #[inline(always)]
  pub fn terminate_execution(&self) -> bool {
    self.thread_safe_handle().terminate_execution()
  }

  /// See [`IsolateHandle::cancel_terminate_execution`]
  #[inline(always)]
  pub fn cancel_terminate_execution(&self) -> bool {
    self.thread_safe_handle().cancel_terminate_execution()
  }

  /// See [`IsolateHandle::is_execution_terminating`]
  #[inline(always)]
  pub fn is_execution_terminating(&self) -> bool {
    self.thread_safe_handle().is_execution_terminating()
  }

  pub(crate) fn create_annex(
    &mut self,
    create_param_allocations: Box<dyn Any>,
  ) {
    let annex_arc = Arc::new(IsolateAnnex::new(self, create_param_allocations));
    let annex_ptr = Arc::into_raw(annex_arc);
    assert!(self.get_data_internal(Self::ANNEX_SLOT).is_null());
    self.set_data_internal(Self::ANNEX_SLOT, annex_ptr as *mut _);
  }

  unsafe fn dispose_annex(&mut self) {
    // Set the `isolate` pointer inside the annex struct to null, so any
    // IsolateHandle that outlives the isolate will know that it can't call
    // methods on the isolate.
    let annex = self.get_annex_mut();
    {
      let _lock = annex.isolate_mutex.lock().unwrap();
      annex.isolate = null_mut();
    }

    // Clear slots and drop owned objects that were taken out of `CreateParams`.
    annex.create_param_allocations = Box::new(());
    annex.slots.clear();

    // Run through any remaining guaranteed finalizers.
    for finalizer in annex.finalizer_map.drain() {
      if let FinalizerCallback::Guaranteed(callback) = finalizer {
        callback();
      }
    }

    // Subtract one from the Arc<IsolateAnnex> reference count.
    unsafe { Arc::from_raw(annex) };
    self.set_data(0, null_mut());
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

  fn get_annex_arc(&self) -> Arc<IsolateAnnex> {
    let annex_ptr = self.get_annex();
    let annex_arc = unsafe { Arc::from_raw(annex_ptr) };
    let _ = Arc::into_raw(annex_arc.clone());
    annex_arc
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
    let n = unsafe { v8__Isolate__GetNumberOfDataSlots(self) };
    n - Self::INTERNAL_DATA_SLOT_COUNT
  }

  #[inline(always)]
  pub(crate) fn get_data_internal(&self, slot: u32) -> *mut c_void {
    unsafe { v8__Isolate__GetData(self, slot) }
  }

  #[inline(always)]
  pub(crate) fn set_data_internal(&mut self, slot: u32, data: *mut c_void) {
    unsafe { v8__Isolate__SetData(self, slot, data) }
  }

  pub(crate) fn init_scope_root(&mut self) {
    ScopeData::new_root(self);
  }

  pub(crate) fn dispose_scope_root(&mut self) {
    ScopeData::drop_root(self);
  }

  /// Returns a pointer to the `ScopeData` struct for the current scope.
  #[inline(always)]
  pub(crate) fn get_current_scope_data(&self) -> Option<NonNull<ScopeData>> {
    let scope_data_ptr = self.get_data_internal(Self::CURRENT_SCOPE_DATA_SLOT);
    NonNull::new(scope_data_ptr).map(NonNull::cast)
  }

  /// Updates the slot that stores a `ScopeData` pointer for the current scope.
  #[inline(always)]
  pub(crate) fn set_current_scope_data(
    &mut self,
    scope_data: Option<NonNull<ScopeData>>,
  ) {
    let scope_data_ptr = scope_data
      .map(NonNull::cast)
      .map_or_else(null_mut, NonNull::as_ptr);
    self.set_data_internal(Self::CURRENT_SCOPE_DATA_SLOT, scope_data_ptr);
  }

  /// Get a reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot<T: 'static>(&self) -> Option<&T> {
    self
      .get_annex()
      .slots
      .get(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.borrow::<T>() })
  }

  /// Get a mutable reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot_mut<T: 'static>(&mut self) -> Option<&mut T> {
    self
      .get_annex_mut()
      .slots
      .get_mut(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.borrow_mut::<T>() })
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
    self
      .get_annex_mut()
      .slots
      .insert(TypeId::of::<T>(), RawSlot::new(value))
      .is_none()
  }

  /// Removes the embedder data added with `set_slot()` and returns it if it exists.
  #[inline(always)]
  pub fn remove_slot<T: 'static>(&mut self) -> Option<T> {
    self
      .get_annex_mut()
      .slots
      .remove(&TypeId::of::<T>())
      .map(|slot| unsafe { slot.into_inner::<T>() })
  }

  /// Sets this isolate as the entered one for the current thread.
  /// Saves the previously entered one (if any), so that it can be
  /// restored when exiting.  Re-entering an isolate is allowed.
  ///
  /// rusty_v8 note: Unlike in the C++ API, the isolate is entered when it is
  /// constructed and exited when dropped.
  #[inline(always)]
  pub unsafe fn enter(&mut self) {
    unsafe {
      v8__Isolate__Enter(self);
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
  pub unsafe fn exit(&mut self) {
    unsafe {
      v8__Isolate__Exit(self);
    }
  }

  /// Optional notification that the system is running low on memory.
  /// V8 uses these notifications to guide heuristics.
  /// It is allowed to call this function from another thread while
  /// the isolate is executing long running JavaScript code.
  #[inline(always)]
  pub fn memory_pressure_notification(&mut self, level: MemoryPressureLevel) {
    unsafe { v8__Isolate__MemoryPressureNotification(self, level as u8) }
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
    unsafe { v8__Isolate__ClearKeptObjects(self) }
  }

  /// Optional notification that the system is running low on memory.
  /// V8 uses these notifications to attempt to free memory.
  #[inline(always)]
  pub fn low_memory_notification(&mut self) {
    unsafe { v8__Isolate__LowMemoryNotification(self) }
  }

  /// Get statistics about the heap memory usage.
  #[inline(always)]
  pub fn get_heap_statistics(&mut self) -> HeapStatistics {
    let inner = unsafe {
      let mut s = MaybeUninit::zeroed();
      v8__Isolate__GetHeapStatistics(self, s.as_mut_ptr());
      s.assume_init()
    };
    HeapStatistics(inner)
  }

  /// Returns the number of spaces in the heap.
  #[inline(always)]
  pub fn number_of_heap_spaces(&mut self) -> usize {
    unsafe { v8__Isolate__NumberOfHeapSpaces(self) }
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
      if !v8__Isolate__GetHeapSpaceStatistics(self, s.as_mut_ptr(), index) {
        return None;
      }
      s.assume_init()
    };
    Some(HeapSpaceStatistics(inner))
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
        self,
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
    unsafe { v8__Isolate__AddMessageListener(self, callback) }
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
        self,
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
      v8__Isolate__SetPrepareStackTraceCallback(self, callback.map_fn_to());
    };
  }

  /// Set the PromiseHook callback for various promise lifecycle
  /// events.
  #[inline(always)]
  pub fn set_promise_hook(&mut self, hook: PromiseHook) {
    unsafe { v8__Isolate__SetPromiseHook(self, hook) }
  }

  /// Set callback to notify about promise reject with no handler, or
  /// revocation of such a previous notification once the handler is added.
  #[inline(always)]
  pub fn set_promise_reject_callback(
    &mut self,
    callback: PromiseRejectCallback,
  ) {
    unsafe { v8__Isolate__SetPromiseRejectCallback(self, callback) }
  }

  #[inline(always)]
  pub fn set_wasm_async_resolve_promise_callback(
    &mut self,
    callback: WasmAsyncResolvePromiseCallback,
  ) {
    unsafe { v8__Isolate__SetWasmAsyncResolvePromiseCallback(self, callback) }
  }

  #[inline(always)]
  pub fn set_allow_wasm_code_generation_callback(
    &mut self,
    callback: AllowWasmCodeGenerationCallback,
  ) {
    unsafe {
      v8__Isolate__SetAllowWasmCodeGenerationCallback(self, callback);
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
      v8__Isolate__SetHostInitializeImportMetaObjectCallback(self, callback);
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
        self,
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
        self,
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
      let mut scope = unsafe { CallbackScope::new(initiator_context) };
      let callback = scope
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
          self,
          rust_shadow_realm_callback_windows,
        );
        #[cfg(not(target_os = "windows"))]
        v8__Isolate__SetHostCreateShadowRealmContextCallback(
          self,
          rust_shadow_realm_callback,
        );
      }
    }
  }

  /// Sets a callback for counting the number of times a feature of V8 is used.
  #[inline(always)]
  pub fn set_use_counter_callback(&mut self, callback: UseCounterCallback) {
    unsafe {
      v8__Isolate__SetUseCounterCallback(self, callback);
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
      v8__Isolate__AddGCPrologueCallback(self, callback, data, gc_type_filter);
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
    unsafe { v8__Isolate__RemoveGCPrologueCallback(self, callback, data) }
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
      v8__Isolate__AddGCEpilogueCallback(self, callback, data, gc_type_filter);
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
    unsafe { v8__Isolate__RemoveGCEpilogueCallback(self, callback, data) }
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
    unsafe { v8__Isolate__AddNearHeapLimitCallback(self, callback, data) };
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
      v8__Isolate__RemoveNearHeapLimitCallback(self, callback, heap_limit);
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
      v8__Isolate__AdjustAmountOfExternalAllocatedMemory(self, change_in_bytes)
    }
  }

  #[inline(always)]
  pub fn get_cpp_heap(&mut self) -> Option<&Heap> {
    unsafe { v8__Isolate__GetCppHeap(self).as_ref() }
  }

  #[inline(always)]
  pub fn set_oom_error_handler(&mut self, callback: OomErrorCallback) {
    unsafe { v8__Isolate__SetOOMErrorHandler(self, callback) };
  }

  /// Returns the policy controlling how Microtasks are invoked.
  #[inline(always)]
  pub fn get_microtasks_policy(&self) -> MicrotasksPolicy {
    unsafe { v8__Isolate__GetMicrotasksPolicy(self) }
  }

  /// Returns the policy controlling how Microtasks are invoked.
  #[inline(always)]
  pub fn set_microtasks_policy(&mut self, policy: MicrotasksPolicy) {
    unsafe { v8__Isolate__SetMicrotasksPolicy(self, policy) }
  }

  /// Runs the default MicrotaskQueue until it gets empty and perform other
  /// microtask checkpoint steps, such as calling ClearKeptObjects. Asserts that
  /// the MicrotasksPolicy is not kScoped. Any exceptions thrown by microtask
  /// callbacks are swallowed.
  #[inline(always)]
  pub fn perform_microtask_checkpoint(&mut self) {
    unsafe { v8__Isolate__PerformMicrotaskCheckpoint(self) }
  }

  /// Enqueues the callback to the default MicrotaskQueue
  #[inline(always)]
  pub fn enqueue_microtask(&mut self, microtask: Local<Function>) {
    unsafe { v8__Isolate__EnqueueMicrotask(self, &*microtask) }
  }

  /// Set whether calling Atomics.wait (a function that may block) is allowed in
  /// this isolate. This can also be configured via
  /// CreateParams::allow_atomics_wait.
  #[inline(always)]
  pub fn set_allow_atomics_wait(&mut self, allow: bool) {
    unsafe { v8__Isolate__SetAllowAtomicsWait(self, allow) }
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
    F: UnitType + Fn(&mut HandleScope, Local<Value>, WasmStreaming),
  {
    unsafe { v8__Isolate__SetWasmStreamingCallback(self, trampoline::<F>()) }
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
        self,
        time_zone_detection,
      );
    }
  }

  /// Returns true if there is ongoing background work within V8 that will
  /// eventually post a foreground task, like asynchronous WebAssembly
  /// compilation.
  #[inline(always)]
  pub fn has_pending_background_tasks(&self) -> bool {
    unsafe { v8__Isolate__HasPendingBackgroundTasks(self) }
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
        self,
        match r#type {
          GarbageCollectionType::Full => 0,
          GarbageCollectionType::Minor => 1,
        },
      );
    }
  }

  /// Disposes the isolate.  The isolate must not be entered by any
  /// thread to be disposable.
  unsafe fn dispose(&mut self) {
    // No test case in rusty_v8 show this, but there have been situations in
    // deno where dropping Annex before the states causes a segfault.
    unsafe {
      v8__Isolate__Dispose(self);
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
      v8__HeapProfiler__TakeHeapSnapshot(self, trampoline::<F>, arg as _);
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
    let snapshot_creator = self
      .get_annex_mut()
      .maybe_snapshot_creator
      .as_mut()
      .unwrap();
    snapshot_creator.add_context_data(context, data)
  }
}

pub(crate) struct IsolateAnnex {
  create_param_allocations: Box<dyn Any>,
  slots: HashMap<TypeId, RawSlot, BuildTypeIdHasher>,
  finalizer_map: FinalizerMap,
  maybe_snapshot_creator: Option<SnapshotCreator>,
  // The `isolate` and `isolate_mutex` fields are there so an `IsolateHandle`
  // (which may outlive the isolate itself) can determine whether the isolate
  // is still alive, and if so, get a reference to it. Safety rules:
  // - The 'main thread' must lock the mutex and reset `isolate` to null just
  //   before the isolate is disposed.
  // - Any other thread must lock the mutex while it's reading/using the
  //   `isolate` pointer.
  isolate: *mut Isolate,
  isolate_mutex: Mutex<()>,
}

unsafe impl Send for IsolateAnnex {}
unsafe impl Sync for IsolateAnnex {}

impl IsolateAnnex {
  fn new(
    isolate: &mut Isolate,
    create_param_allocations: Box<dyn Any>,
  ) -> Self {
    Self {
      create_param_allocations,
      slots: HashMap::default(),
      finalizer_map: FinalizerMap::default(),
      maybe_snapshot_creator: None,
      isolate,
      isolate_mutex: Mutex::new(()),
    }
  }
}

impl Debug for IsolateAnnex {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("IsolateAnnex")
      .field("isolate", &self.isolate)
      .field("isolate_mutex", &self.isolate_mutex)
      .finish()
  }
}

/// IsolateHandle is a thread-safe reference to an Isolate. It's main use is to
/// terminate execution of a running isolate from another thread.
///
/// It is created with Isolate::thread_safe_handle().
///
/// IsolateHandle is Cloneable, Send, and Sync.
#[derive(Clone, Debug)]
pub struct IsolateHandle(Arc<IsolateAnnex>);

impl IsolateHandle {
  // This function is marked unsafe because it must be called only with either
  // IsolateAnnex::mutex locked, or from the main thread associated with the V8
  // isolate.
  pub(crate) unsafe fn get_isolate_ptr(&self) -> *mut Isolate {
    self.0.isolate
  }

  #[inline(always)]
  fn new(isolate: &Isolate) -> Self {
    Self(isolate.get_annex_arc())
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
    let _lock = self.0.isolate_mutex.lock().unwrap();
    if self.0.isolate.is_null() {
      false
    } else {
      unsafe { v8__Isolate__TerminateExecution(self.0.isolate) };
      true
    }
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
    let _lock = self.0.isolate_mutex.lock().unwrap();
    if self.0.isolate.is_null() {
      false
    } else {
      unsafe { v8__Isolate__CancelTerminateExecution(self.0.isolate) };
      true
    }
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
    let _lock = self.0.isolate_mutex.lock().unwrap();
    if self.0.isolate.is_null() {
      false
    } else {
      unsafe { v8__Isolate__IsExecutionTerminating(self.0.isolate) }
    }
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
    let _lock = self.0.isolate_mutex.lock().unwrap();
    if self.0.isolate.is_null() {
      false
    } else {
      unsafe { v8__Isolate__RequestInterrupt(self.0.isolate, callback, data) };
      true
    }
  }
}

/// Same as Isolate but gets disposed when it goes out of scope.
#[derive(Debug)]
pub struct OwnedIsolate {
  cxx_isolate: NonNull<Isolate>,
}

impl OwnedIsolate {
  pub(crate) fn new(cxx_isolate: *mut Isolate) -> Self {
    let mut isolate = Self::new_already_entered(cxx_isolate);
    unsafe {
      isolate.enter();
    }
    isolate
  }

  pub(crate) fn new_already_entered(cxx_isolate: *mut Isolate) -> Self {
    let cxx_isolate = NonNull::new(cxx_isolate).unwrap();
    let mut owned_isolate = Self { cxx_isolate };
    owned_isolate.init_scope_root();
    owned_isolate
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
        self.cxx_isolate.as_mut() as *mut Isolate == v8__Isolate__GetCurrent(),
        "v8::OwnedIsolate instances must be dropped in the reverse order of creation. They are entered upon creation and exited upon being dropped."
      );
      self.dispose_scope_root();
      self.exit();
      self.dispose_annex();
      self.dispose();
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
    unsafe {
      self.dispose_scope_root();
      self.dispose_annex();
    }
    // The isolate is owned by the snapshot creator; we need to forget it
    // here as the snapshot creator will drop it when running the destructor.
    std::mem::forget(self);
    snapshot_creator.create_blob(function_code_handling)
  }
}

impl Deref for OwnedIsolate {
  type Target = Isolate;
  fn deref(&self) -> &Self::Target {
    unsafe { self.cxx_isolate.as_ref() }
  }
}

impl DerefMut for OwnedIsolate {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { self.cxx_isolate.as_mut() }
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

impl<'s, F> MapFnFrom<F> for PrepareStackTraceCallback<'s>
where
  F: UnitType
    + Fn(
      &mut HandleScope<'s>,
      Local<'s, Value>,
      Local<'s, Array>,
    ) -> Local<'s, Value>,
{
  // Windows x64 ABI: MaybeLocal<Value> returned on the stack.
  #[cfg(target_os = "windows")]
  fn mapping() -> Self {
    let f = |ret_ptr, context, error, sites| {
      let mut scope: CallbackScope = unsafe { CallbackScope::new(context) };
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
      let mut scope: CallbackScope = unsafe { CallbackScope::new(context) };
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
