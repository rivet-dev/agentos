// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::ArrayBuffer;
use crate::Isolate;
use crate::Local;
use crate::Value;
use crate::WasmMemoryObject;
use crate::WasmModuleObject;
use crate::function::FunctionCallbackArguments;
use crate::function::FunctionCallbackInfo;
use crate::scope::CallbackScope;
use crate::scope::HandleScope;
use crate::support::Opaque;
use crate::support::UnitType;
use crate::support::char;
use std::ptr::null;
use std::ptr::null_mut;

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
pub struct WasmStreaming(WasmStreamingSharedPtr);

impl WasmStreaming {
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

  /// Should be called after all received bytes where passed to
  /// [`Self::on_bytes_received()`] to tell V8 that there will be no
  /// more bytes. Does not have to be called after [`Self::abort()`]
  /// has been called already.
  #[inline(always)]
  pub fn finish(mut self) {
    unsafe { v8__WasmStreaming__Finish(&mut self.0) }
  }

  /// Abort streaming compilation. If {exception} has a value, then the promise
  /// associated with streaming compilation is rejected with that value. If
  /// {exception} does not have value, the promise does not get rejected.
  #[inline(always)]
  pub fn abort(mut self, exception: Option<Local<Value>>) {
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

impl Drop for WasmStreaming {
  fn drop(&mut self) {
    unsafe { v8__WasmStreaming__shared_ptr_DESTRUCT(&mut self.0) }
  }
}

impl WasmModuleObject {
  /// Efficiently re-create a WasmModuleObject, without recompiling, from
  /// a CompiledWasmModule.
  #[inline(always)]
  pub fn from_compiled_module<'s>(
    scope: &mut HandleScope<'s>,
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
    scope: &mut HandleScope<'s>,
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

impl WasmMemoryObject {
  /// Returns underlying ArrayBuffer.
  #[inline(always)]
  pub fn buffer(&self) -> Local<ArrayBuffer> {
    unsafe { Local::from_raw(v8__WasmMemoryObject__Buffer(self)) }.unwrap()
  }
}

pub(crate) fn trampoline<F>()
-> unsafe extern "C" fn(*const FunctionCallbackInfo)
where
  F: UnitType + Fn(&mut HandleScope, Local<Value>, WasmStreaming),
{
  unsafe extern "C" fn c_fn<F>(info: *const FunctionCallbackInfo)
  where
    F: UnitType + Fn(&mut HandleScope, Local<Value>, WasmStreaming),
  {
    let info = unsafe { &*info };
    let scope = &mut unsafe { CallbackScope::new(info) };
    let args = FunctionCallbackArguments::from_function_callback_info(info);
    let data = args.data();
    let zero = null_mut();
    let mut that = WasmStreamingSharedPtr([zero, zero]);
    unsafe {
      v8__WasmStreaming__Unpack(scope.get_isolate_ptr(), &*data, &mut that);
    };
    let source = args.get(0);
    (F::get())(scope, source, WasmStreaming(that));
  }
  c_fn::<F>
}

unsafe extern "C" {
  fn v8__WasmStreaming__Unpack(
    isolate: *mut Isolate,
    value: *const Value,
    that: *mut WasmStreamingSharedPtr, // Out parameter.
  );
  fn v8__WasmStreaming__shared_ptr_DESTRUCT(this: *mut WasmStreamingSharedPtr);
  fn v8__WasmStreaming__OnBytesReceived(
    this: *mut WasmStreamingSharedPtr,
    data: *const u8,
    len: usize,
  );
  fn v8__WasmStreaming__Finish(this: *mut WasmStreamingSharedPtr);
  fn v8__WasmStreaming__Abort(
    this: *mut WasmStreamingSharedPtr,
    exception: *const Value,
  );
  fn v8__WasmStreaming__SetUrl(
    this: *mut WasmStreamingSharedPtr,
    url: *const char,
    len: usize,
  );

  fn v8__WasmModuleObject__FromCompiledModule(
    isolate: *mut Isolate,
    compiled_module: *const InternalCompiledWasmModule,
  ) -> *const WasmModuleObject;
  fn v8__WasmModuleObject__GetCompiledModule(
    this: *const WasmModuleObject,
  ) -> *mut InternalCompiledWasmModule;
  fn v8__WasmModuleObject__Compile(
    isolate: *mut Isolate,
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
}
