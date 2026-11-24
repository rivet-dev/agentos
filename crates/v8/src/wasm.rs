// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use std::ffi::c_void;
use std::ptr::null;
use std::ptr::null_mut;

use crate::ArrayBuffer;
use crate::Isolate;
use crate::IsolateHandle;
use crate::Local;
use crate::PinScope;
use crate::Value;
use crate::WasmMemoryObject;
use crate::WasmModuleObject;
use crate::binding::const_memory_span_t;
use crate::function::FunctionCallbackArguments;
use crate::function::FunctionCallbackInfo;
use crate::isolate::RealIsolate;
use crate::scope::GetIsolate;
use crate::scope::callback_scope;
use crate::support::MapFnFrom;
use crate::support::MapFnTo;
use crate::support::Opaque;
use crate::support::ToCFn;
use crate::support::UnitType;
use crate::support::char;

// Type-erased std::shared_ptr<v8::WasmStreaming>. Assumes it's safe
// to move around (no backlinks). Not generally true for shared_ptrs
// but it is in this case - other shared_ptrs that point to the same
// v8::WasmStreaming exist but are managed by V8 and don't leak out.
//
// We don't use crate::support::SharedPtr because it only allows
// immutable borrows and derefs to avoid aliasing but that's not
// a problem here, only a single instance outside V8 exists.
//
// Note: uses *mut u8 rather than e.g. usize to enforce !Send and !Sync.
#[repr(C)]
struct WasmStreamingSharedPtr([*mut u8; 2]);

/// The V8 interface for WebAssembly streaming compilation.
/// When streaming compilation is initiated, V8 passes a [Self]
/// object to the embedder such that the embedder can pass the
/// input bytes for streaming compilation to V8.
#[repr(C)]
pub struct WasmStreaming<const HAS_COMPILED_MODULE_BYTES: bool>(
  WasmStreamingSharedPtr,
  // The isolate that started the streaming compilation, whose promise
  // `abort()` rejects.
  IsolateHandle,
);

impl<const HAS_COMPILED_MODULE_BYTES: bool>
  WasmStreaming<HAS_COMPILED_MODULE_BYTES>
{
  /// Pass a new chunk of bytes to WebAssembly streaming compilation.
  #[inline(always)]
  pub fn on_bytes_received(&mut self, data: &[u8]) {
    unsafe {
      v8__WasmStreaming__OnBytesReceived(
        &mut self.0,
        data.as_ptr(),
        data.len(),
      );
    }
  }

  /// Abort streaming compilation. If {exception} has a value, then the promise
  /// associated with streaming compilation is rejected with that value. If
  /// {exception} does not have value, the promise does not get rejected.
  #[inline(always)]
  ///
  /// # Panics
  ///
  /// Panics if `exception` belongs to another isolate than the one that
  /// started the streaming compilation.
  pub fn abort(mut self, exception: Option<Local<Value>>) {
    if exception.is_some() {
      crate::handle::assert_locals_in_isolate_of(&self.1);
    }
    let exception = exception.map_or(null(), |v| &*v as *const Value);
    unsafe { v8__WasmStreaming__Abort(&mut self.0, exception) }
  }

  /// Sets the UTF-8 encoded source URL for the `Script` object. This must be
  /// called before [`Self::finish()`].
  #[inline(always)]
  pub fn set_url(&mut self, url: &str) {
    // Although not documented, V8 requires the url to be null terminated.
    // See https://chromium-review.googlesource.com/c/v8/v8/+/3289148.
    let null_terminated_url = format!("{url}\0");
    unsafe {
      v8__WasmStreaming__SetUrl(
        &mut self.0,
        null_terminated_url.as_ptr() as *const char,
        url.len(),
      );
    }
  }
}

impl WasmStreaming<false> {
  /// {Finish} should be called after all received bytes where passed to
  /// {OnBytesReceived} to tell V8 that there will be no more bytes. {Finish}
  /// must not be called after {Abort} has been called already.
  /// If {SetHasCompiledModuleBytes()} was called before, a {caching_callback}
  /// can be passed which can inspect the full received wire bytes and set cached
  /// module bytes which will be deserialized then. This callback will happen
  /// synchronously within this call; the callback is not stored.
  #[inline(always)]
  pub fn finish(mut self) {
    unsafe { v8__WasmStreaming__Finish(&mut self.0, None) }
  }

  /// Mark that the embedder has (potentially) cached compiled module bytes (i.e.
  /// a serialized {CompiledWasmModule}) that could match this streaming request.
  /// This will cause V8 to skip streaming compilation.
  /// The embedder should then pass a callback to the {Finish} method to pass the
  /// serialized bytes, after potentially checking their validity against the
  /// full received wire bytes.
  #[inline(always)]
  pub fn set_has_compiled_module_bytes(mut self) -> WasmStreaming<true> {
    unsafe {
      v8__WasmStreaming__SetHasCompiledModuleBytes(&mut self.0);
      std::mem::transmute(self)
    }
  }
}

impl WasmStreaming<true> {
  /// {Finish} should be called after all received bytes where passed to
  /// {OnBytesReceived} to tell V8 that there will be no more bytes. {Finish}
  /// must not be called after {Abort} has been called already.
  /// If {SetHasCompiledModuleBytes()} was called before, a {caching_callback}
  /// can be passed which can inspect the full received wire bytes and set cached
  /// module bytes which will be deserialized then. This callback will happen
  /// synchronously within this call; the callback is not stored.
  #[inline(always)]
  pub fn finish<F>(mut self, f: F)
  where
    F: MapFnTo<ModuleCachingCallback>,
  {
    unsafe { v8__WasmStreaming__Finish(&mut self.0, Some(f.map_fn_to())) }
  }
}

impl<const HAS_COMPILED_MODULE_BYTES: bool> Drop
  for WasmStreaming<HAS_COMPILED_MODULE_BYTES>
{
  fn drop(&mut self) {
    unsafe { v8__WasmStreaming__shared_ptr_DESTRUCT(&mut self.0) }
  }
}

impl WasmModuleObject {
  /// Efficiently re-create a WasmModuleObject, without recompiling, from
  /// a CompiledWasmModule.
  #[inline(always)]
  pub fn from_compiled_module<'s>(
    scope: &PinScope<'s, '_>,
    compiled_module: &CompiledWasmModule,
  ) -> Option<Local<'s, WasmModuleObject>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__WasmModuleObject__FromCompiledModule(
          sd.get_isolate_ptr(),
          compiled_module.0,
        )
      })
    }
  }

  /// Get the compiled module for this module object. The compiled module can be
  /// shared by several module objects.
  #[inline(always)]
  pub fn get_compiled_module(&self) -> CompiledWasmModule {
    let ptr = unsafe { v8__WasmModuleObject__GetCompiledModule(self) };
    CompiledWasmModule(ptr)
  }

  /// Compile a Wasm module from the provided uncompiled bytes.
  #[inline(always)]
  pub fn compile<'s>(
    scope: &PinScope<'s, '_>,
    wire_bytes: &[u8],
  ) -> Option<Local<'s, WasmModuleObject>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__WasmModuleObject__Compile(
          sd.get_isolate_ptr(),
          wire_bytes.as_ptr(),
          wire_bytes.len(),
        )
      })
    }
  }
}

#[repr(C)]
pub struct ModuleCachingInterface(Opaque);

impl ModuleCachingInterface {
  /// Get the full wire bytes, to check against the cached version.
  #[inline(always)]
  pub fn get_wire_bytes(&self) -> &[u8] {
    unsafe {
      let span = v8__ModuleCachingInterface__GetWireBytes(self);
      std::slice::from_raw_parts(span.data, span.size)
    }
  }

  /// Pass serialized (cached) compiled module bytes, to be deserialized and
  /// used as the result of this streaming compilation.
  /// The passed bytes will only be accessed inside this callback, i.e.
  /// lifetime can end after the call.
  /// The return value indicates whether V8 could use the passed bytes; {false}
  /// would be returned on e.g. version mismatch.
  /// This method can only be called once.
  #[inline(always)]
  pub fn set_cached_compiled_module_bytes(&mut self, bytes: &[u8]) -> bool {
    unsafe {
      v8__ModuleCachingInterface__SetCachedCompiledModuleBytes(
        self,
        const_memory_span_t {
          data: bytes.as_ptr(),
          size: bytes.len(),
        },
      )
    }
  }
}

pub type ModuleCachingCallback =
  unsafe extern "C" fn(*mut ModuleCachingInterface);

impl<F> MapFnFrom<F> for ModuleCachingCallback
where
  F: UnitType + Fn(&mut ModuleCachingInterface),
{
  fn mapping() -> Self {
    let f = |mci: *mut ModuleCachingInterface| {
      (F::get())(unsafe { &mut *mci });
    };
    f.to_c_fn()
  }
}

// Type-erased v8::CompiledWasmModule. We need this because the C++
// v8::CompiledWasmModule must be destructed because its private fields hold
// pointers that must be freed, but v8::CompiledWasmModule itself doesn't have
// a destructor. Therefore, in order to avoid memory leaks, the Rust-side
// CompiledWasmModule must be a pointer to a C++ allocation of
// v8::CompiledWasmModule.
#[repr(C)]
struct InternalCompiledWasmModule(Opaque);

/// Wrapper around a compiled WebAssembly module, which is potentially shared by
/// different WasmModuleObjects.
pub struct CompiledWasmModule(*mut InternalCompiledWasmModule);

impl CompiledWasmModule {
  /// Get the (wasm-encoded) wire bytes that were used to compile this module.
  #[inline(always)]
  pub fn get_wire_bytes_ref(&self) -> &[u8] {
    let mut len = 0isize;
    unsafe {
      let ptr = v8__CompiledWasmModule__GetWireBytesRef(self.0, &mut len);
      std::slice::from_raw_parts(ptr, len.try_into().unwrap())
    }
  }

  #[inline(always)]
  pub fn source_url(&self) -> &str {
    let mut len = 0;
    unsafe {
      let ptr = v8__CompiledWasmModule__SourceUrl(self.0, &mut len);
      let bytes = std::slice::from_raw_parts(ptr as _, len);
      std::str::from_utf8_unchecked(bytes)
    }
  }
}

// TODO(andreubotella): Safety???
unsafe impl Send for CompiledWasmModule {}
unsafe impl Sync for CompiledWasmModule {}

impl Drop for CompiledWasmModule {
  fn drop(&mut self) {
    unsafe { v8__CompiledWasmModule__DELETE(self.0) }
  }
}

// Type-erased v8::WasmModuleCompilation allocated on the C++ heap.
#[repr(C)]
struct InternalWasmModuleCompilation(Opaque);

/// An interface for asynchronous WebAssembly module compilation, to be used
/// e.g. for implementing source phase imports.
///
/// Note: This interface is experimental and can change or be removed without
/// notice.
pub struct WasmModuleCompilation(*mut InternalWasmModuleCompilation);

// OnBytesReceived can be called from any thread per V8 documentation.
unsafe impl Send for WasmModuleCompilation {}

impl WasmModuleCompilation {
  /// Start an asynchronous module compilation. This can be called on any
  /// thread.
  #[inline(always)]
  pub fn new() -> Self {
    unsafe { WasmModuleCompilation(v8__WasmModuleCompilation__NEW()) }
  }

  /// Pass a new chunk of bytes to WebAssembly compilation. The buffer is
  /// owned by the caller and will not be accessed after this call returns.
  /// Can be called from any thread.
  #[inline(always)]
  pub fn on_bytes_received(&mut self, data: &[u8]) {
    unsafe {
      v8__WasmModuleCompilation__OnBytesReceived(
        self.0,
        data.as_ptr(),
        data.len(),
      );
    }
  }

  /// Finish compilation. Must be called on the main thread after all bytes
  /// were passed to [`Self::on_bytes_received`].
  ///
  /// The `resolution_callback` will eventually be called with either the
  /// compiled module or a compilation error. The callback receives `&Isolate`
  /// so that [`crate::Global`] handles can be created from the [`Local`]
  /// handles to persist them beyond the callback.
  ///
  /// Must not be called after [`Self::abort`].
  #[inline(always)]
  pub fn finish(
    self,
    scope: &mut PinScope,
    caching_callback: Option<ModuleCachingCallback>,
    resolution_callback: impl FnOnce(
      &Isolate,
      Result<Local<'_, WasmModuleObject>, Local<'_, Value>>,
    ) + 'static,
  ) {
    // Capture the isolate pointer in the closure so it doesn't need to be
    // threaded through C++.
    let isolate_ptr = scope.get_isolate_ptr();
    let wrapped = move |module: *const WasmModuleObject,
                        error: *const Value| {
      let isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
      if !module.is_null() {
        resolution_callback(
          &isolate,
          Ok(unsafe { Local::from_raw(module) }.unwrap()),
        );
      } else {
        resolution_callback(
          &isolate,
          Err(unsafe { Local::from_raw(error) }.unwrap()),
        );
      }
    };

    // Double-box with Option: the outer Box gives us a thin pointer suitable
    // for void*. The Option allows the trampoline to .take() the closure
    // (FnOnce semantics) without freeing the outer allocation, which is
    // ref-counted by shared_ptr on the C++ side.
    #[allow(clippy::type_complexity)]
    let boxed: Box<
      Option<Box<dyn FnOnce(*const WasmModuleObject, *const Value)>>,
    > = Box::new(Some(Box::new(wrapped)));
    let data = Box::into_raw(boxed) as *mut c_void;

    unsafe {
      v8__WasmModuleCompilation__Finish(
        self.0,
        scope.get_isolate_ptr(),
        caching_callback,
        resolution_trampoline,
        data,
        drop_resolution_data,
      );
    }
  }

  /// Abort compilation. Can be called from any thread.
  /// Must not be called repeatedly, or after [`Self::finish`].
  #[inline(always)]
  pub fn abort(self) {
    unsafe { v8__WasmModuleCompilation__Abort(self.0) }
  }

  /// Mark that the embedder has (potentially) cached compiled module bytes
  /// (i.e. a serialized [`CompiledWasmModule`]) that could match this
  /// compilation request. This will cause V8 to skip streaming compilation.
  /// The embedder should then pass a caching callback to [`Self::finish`].
  #[inline(always)]
  pub fn set_has_compiled_module_bytes(&mut self) {
    unsafe {
      v8__WasmModuleCompilation__SetHasCompiledModuleBytes(self.0);
    }
  }

  /// Sets a callback which is called whenever a significant number of new
  /// functions are ready for serialization.
  #[inline(always)]
  pub fn set_more_functions_can_be_serialized_callback(
    &mut self,
    callback: impl Fn(CompiledWasmModule) + Send + 'static,
  ) {
    let boxed: Box<Box<dyn Fn(CompiledWasmModule) + Send>> =
      Box::new(Box::new(callback));
    let data = Box::into_raw(boxed) as *mut c_void;

    unsafe {
      v8__WasmModuleCompilation__SetMoreFunctionsCanBeSerializedCallback(
        self.0,
        serialization_trampoline,
        data,
        drop_serialization_data,
      );
    }
  }

  /// Sets the UTF-8 encoded source URL for the `Script` object. This must
  /// be called before [`Self::finish`].
  #[inline(always)]
  pub fn set_url(&mut self, url: &str) {
    // V8 requires the url to be null terminated.
    let null_terminated_url = format!("{url}\0");
    unsafe {
      v8__WasmModuleCompilation__SetUrl(
        self.0,
        null_terminated_url.as_ptr() as *const char,
        url.len(),
      );
    }
  }
}

impl Default for WasmModuleCompilation {
  fn default() -> Self {
    Self::new()
  }
}

impl Drop for WasmModuleCompilation {
  fn drop(&mut self) {
    unsafe { v8__WasmModuleCompilation__DELETE(self.0) }
  }
}

unsafe extern "C" fn resolution_trampoline(
  data: *mut c_void,
  module: *const WasmModuleObject,
  error: *const Value,
) {
  // Take the closure out of the Option without freeing the outer Box.
  // The outer Box is ref-counted by shared_ptr on the C++ side and will
  // be freed via drop_resolution_data when the last copy is destroyed.
  let slot = unsafe {
    &mut *(data
      as *mut Option<Box<dyn FnOnce(*const WasmModuleObject, *const Value)>>)
  };
  let callback = slot.take().unwrap();
  callback(module, error);
}

unsafe extern "C" fn drop_resolution_data(data: *mut c_void) {
  let _ = unsafe {
    Box::from_raw(
      data
        as *mut Option<Box<dyn FnOnce(*const WasmModuleObject, *const Value)>>,
    )
  };
}

unsafe extern "C" fn serialization_trampoline(
  data: *mut c_void,
  compiled_module: *mut InternalCompiledWasmModule,
) {
  let callback =
    unsafe { &**(data as *const Box<dyn Fn(CompiledWasmModule) + Send>) };
  callback(CompiledWasmModule(compiled_module));
}

unsafe extern "C" fn drop_serialization_data(data: *mut c_void) {
  let _ = unsafe {
    Box::from_raw(data as *mut Box<dyn Fn(CompiledWasmModule) + Send>)
  };
}

impl WasmMemoryObject {
  /// Returns underlying ArrayBuffer.
  #[inline(always)]
  pub fn buffer(&self) -> Local<'_, ArrayBuffer> {
    unsafe { Local::from_raw(v8__WasmMemoryObject__Buffer(self)) }.unwrap()
  }
}

pub(crate) fn trampoline<F>()
-> unsafe extern "C" fn(*const FunctionCallbackInfo)
where
  F: UnitType
    + for<'a, 'b, 'c> Fn(
      &'c mut PinScope<'a, 'b>,
      Local<'a, Value>,
      WasmStreaming<false>,
    ),
{
  unsafe extern "C" fn c_fn<F>(info: *const FunctionCallbackInfo)
  where
    F: UnitType
      + for<'a, 'b, 'c> Fn(
        &'c mut PinScope<'a, 'b>,
        Local<'a, Value>,
        WasmStreaming<false>,
      ),
  {
    let info = unsafe { &*info };
    callback_scope!(unsafe scope, info);
    let args = FunctionCallbackArguments::from_function_callback_info(info);
    let data = args.data();
    let zero = null_mut();
    let mut that = WasmStreamingSharedPtr([zero, zero]);
    unsafe {
      v8__WasmStreaming__Unpack(scope.get_isolate_ptr(), &*data, &mut that);
    };
    let source = args.get(0);
    let isolate = scope.thread_safe_handle();
    (F::get())(scope, source, WasmStreaming(that, isolate));
  }
  c_fn::<F>
}

unsafe extern "C" {
  fn v8__WasmStreaming__Unpack(
    isolate: *mut RealIsolate,
    value: *const Value,
    that: *mut WasmStreamingSharedPtr, // Out parameter.
  );
  fn v8__WasmStreaming__shared_ptr_DESTRUCT(this: *mut WasmStreamingSharedPtr);
  fn v8__WasmStreaming__SetHasCompiledModuleBytes(
    this: *mut WasmStreamingSharedPtr,
  );
  fn v8__WasmStreaming__OnBytesReceived(
    this: *mut WasmStreamingSharedPtr,
    data: *const u8,
    len: usize,
  );
  fn v8__WasmStreaming__Finish(
    this: *mut WasmStreamingSharedPtr,
    callback: Option<ModuleCachingCallback>,
  );
  fn v8__WasmStreaming__Abort(
    this: *mut WasmStreamingSharedPtr,
    exception: *const Value,
  );
  fn v8__WasmStreaming__SetUrl(
    this: *mut WasmStreamingSharedPtr,
    url: *const char,
    len: usize,
  );

  fn v8__ModuleCachingInterface__GetWireBytes(
    interface: *const ModuleCachingInterface,
  ) -> const_memory_span_t;
  fn v8__ModuleCachingInterface__SetCachedCompiledModuleBytes(
    interface: *mut ModuleCachingInterface,
    bytes: const_memory_span_t,
  ) -> bool;

  fn v8__WasmModuleObject__FromCompiledModule(
    isolate: *mut RealIsolate,
    compiled_module: *const InternalCompiledWasmModule,
  ) -> *const WasmModuleObject;
  fn v8__WasmModuleObject__GetCompiledModule(
    this: *const WasmModuleObject,
  ) -> *mut InternalCompiledWasmModule;
  fn v8__WasmModuleObject__Compile(
    isolate: *mut RealIsolate,
    wire_bytes_data: *const u8,
    length: usize,
  ) -> *mut WasmModuleObject;

  fn v8__CompiledWasmModule__GetWireBytesRef(
    this: *mut InternalCompiledWasmModule,
    length: *mut isize,
  ) -> *const u8;
  fn v8__CompiledWasmModule__SourceUrl(
    this: *mut InternalCompiledWasmModule,
    length: *mut usize,
  ) -> *const char;
  fn v8__CompiledWasmModule__DELETE(this: *mut InternalCompiledWasmModule);

  fn v8__WasmMemoryObject__Buffer(
    this: *const WasmMemoryObject,
  ) -> *mut ArrayBuffer;

  fn v8__WasmModuleCompilation__NEW() -> *mut InternalWasmModuleCompilation;
  fn v8__WasmModuleCompilation__DELETE(
    this: *mut InternalWasmModuleCompilation,
  );
  fn v8__WasmModuleCompilation__OnBytesReceived(
    this: *mut InternalWasmModuleCompilation,
    bytes: *const u8,
    size: usize,
  );
  fn v8__WasmModuleCompilation__Finish(
    this: *mut InternalWasmModuleCompilation,
    isolate: *mut RealIsolate,
    caching_callback: Option<ModuleCachingCallback>,
    resolution_callback: unsafe extern "C" fn(
      *mut c_void,
      *const WasmModuleObject,
      *const Value,
    ),
    resolution_data: *mut c_void,
    drop_resolution_data: unsafe extern "C" fn(*mut c_void),
  );
  fn v8__WasmModuleCompilation__Abort(this: *mut InternalWasmModuleCompilation);
  fn v8__WasmModuleCompilation__SetHasCompiledModuleBytes(
    this: *mut InternalWasmModuleCompilation,
  );
  fn v8__WasmModuleCompilation__SetMoreFunctionsCanBeSerializedCallback(
    this: *mut InternalWasmModuleCompilation,
    callback: unsafe extern "C" fn(
      *mut c_void,
      *mut InternalCompiledWasmModule,
    ),
    data: *mut c_void,
    drop_data: unsafe extern "C" fn(*mut c_void),
  );
  fn v8__WasmModuleCompilation__SetUrl(
    this: *mut InternalWasmModuleCompilation,
    url: *const char,
    length: usize,
  );
}
