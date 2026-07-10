use crate::ArrayBuffer;
use crate::CallbackScope;
use crate::Context;
use crate::ContextScope;
use crate::Exception;
use crate::Global;
use crate::HandleScope;
use crate::Isolate;
use crate::Local;
use crate::Object;
use crate::SharedArrayBuffer;
use crate::String;
use crate::Value;
use crate::WasmModuleObject;

use crate::support::CxxVTable;
use crate::support::FieldOffset;
use crate::support::MaybeBool;

use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::ptr::addr_of;

// Must be == sizeof(v8::ValueDeserializer::Delegate),
// see v8__ValueDeserializer__Delegate__CONSTRUCT().
#[repr(C)]
pub struct CxxValueDeserializerDelegate {
  _cxx_vtable: CxxVTable,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__ValueDeserializer__Delegate__ReadHostObject(
  this: &CxxValueDeserializerDelegate,
  isolate: *mut Isolate,
) -> *const Object {
  let value_deserializer_heap =
    unsafe { ValueDeserializerHeap::dispatch(this) };
  let scope = unsafe { &mut CallbackScope::new(isolate.as_mut().unwrap()) };
  let context = Local::new(scope, &value_deserializer_heap.context);
  let scope = &mut ContextScope::new(scope, context);

  match value_deserializer_heap
    .value_deserializer_impl
    .read_host_object(scope, &value_deserializer_heap.cxx_value_deserializer)
  {
    None => std::ptr::null(),
    Some(x) => x.as_non_null().as_ptr(),
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__ValueDeserializer__Delegate__GetSharedArrayBufferFromId(
  this: &CxxValueDeserializerDelegate,
  isolate: *mut Isolate,
  transfer_id: u32,
) -> *const SharedArrayBuffer {
  let value_deserializer_heap =
    unsafe { ValueDeserializerHeap::dispatch(this) };
  let scope = unsafe { &mut CallbackScope::new(isolate.as_mut().unwrap()) };
  let context = Local::new(scope, &value_deserializer_heap.context);
  let scope = &mut ContextScope::new(scope, context);

  match value_deserializer_heap
    .value_deserializer_impl
    .get_shared_array_buffer_from_id(scope, transfer_id)
  {
    None => std::ptr::null(),
    Some(x) => x.as_non_null().as_ptr(),
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__ValueDeserializer__Delegate__GetWasmModuleFromId(
  this: &mut CxxValueDeserializerDelegate,
  isolate: *mut Isolate,
  clone_id: u32,
) -> *const WasmModuleObject {
  let value_deserializer_heap =
    unsafe { ValueDeserializerHeap::dispatch(this) };
  let scope = unsafe { &mut CallbackScope::new(isolate.as_mut().unwrap()) };
  let context = Local::new(scope, &value_deserializer_heap.context);
  let scope = &mut ContextScope::new(scope, context);

  match value_deserializer_heap
    .value_deserializer_impl
    .get_wasm_module_from_id(scope, clone_id)
  {
    None => std::ptr::null(),
    Some(x) => x.as_non_null().as_ptr(),
  }
}

unsafe extern "C" {
  fn v8__ValueDeserializer__Delegate__CONSTRUCT(
    buf: *mut MaybeUninit<CxxValueDeserializerDelegate>,
  );
}

// Must be == sizeof(v8::ValueDeserializer),
// see v8__ValueDeserializer__CONSTRUCT().
#[repr(C)]
pub struct CxxValueDeserializer {
  _cxx_vtable: CxxVTable,
}

unsafe extern "C" {
  fn v8__ValueDeserializer__CONSTRUCT(
    buf: *mut MaybeUninit<CxxValueDeserializer>,
    isolate: *mut Isolate,
    data: *const u8,
    size: usize,
    delegate: *mut CxxValueDeserializerDelegate,
  );

  fn v8__ValueDeserializer__DESTRUCT(this: *mut CxxValueDeserializer);

  fn v8__ValueDeserializer__TransferArrayBuffer(
    this: *mut CxxValueDeserializer,
    transfer_id: u32,
    array_buffer: Local<ArrayBuffer>,
  );

  fn v8__ValueDeserializer__TransferSharedArrayBuffer(
    this: *mut CxxValueDeserializer,
    transfer_id: u32,
    array_buffer: Local<SharedArrayBuffer>,
  );

  fn v8__ValueDeserializer__SetSupportsLegacyWireFormat(
    this: *mut CxxValueDeserializer,
    supports_legacy_wire_format: bool,
  );

  fn v8__ValueDeserializer__ReadHeader(
    this: *mut CxxValueDeserializer,
    context: Local<Context>,
  ) -> MaybeBool;

  fn v8__ValueDeserializer__ReadValue(
    this: *mut CxxValueDeserializer,
    context: Local<Context>,
  ) -> *const Value;

  fn v8__ValueDeserializer__ReadUint32(
    this: *mut CxxValueDeserializer,
    value: *mut u32,
  ) -> bool;

  fn v8__ValueDeserializer__ReadUint64(
    this: *mut CxxValueDeserializer,
    value: *mut u64,
  ) -> bool;

  fn v8__ValueDeserializer__ReadDouble(
    this: *mut CxxValueDeserializer,
    value: *mut f64,
  ) -> bool;

  fn v8__ValueDeserializer__ReadRawBytes(
    this: *mut CxxValueDeserializer,
    length: usize,
    data: *mut *const c_void,
  ) -> bool;

  fn v8__ValueDeserializer__GetWireFormatVersion(
    this: *mut CxxValueDeserializer,
  ) -> u32;
}

/// The ValueDeserializerImpl trait allows for
/// custom callback functions used by v8.
pub trait ValueDeserializerImpl {
  fn read_host_object<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    _value_deserializer: &dyn ValueDeserializerHelper,
  ) -> Option<Local<'s, Object>> {
    let msg =
      String::new(scope, "Deno deserializer: read_host_object not implemented")
        .unwrap();
    let exc = Exception::error(scope, msg);
    scope.throw_exception(exc);
    None
  }

  fn get_shared_array_buffer_from_id<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    _transfer_id: u32,
  ) -> Option<Local<'s, SharedArrayBuffer>> {
    let msg = String::new(
      scope,
      "Deno deserializer: get_shared_array_buffer_from_id not implemented",
    )
    .unwrap();
    let exc = Exception::error(scope, msg);
    scope.throw_exception(exc);
    None
  }

  fn get_wasm_module_from_id<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    _clone_id: u32,
  ) -> Option<Local<'s, WasmModuleObject>> {
    let msg = String::new(
      scope,
      "Deno deserializer: get_wasm_module_from_id not implemented",
    )
    .unwrap();
    let exc = Exception::error(scope, msg);
    scope.throw_exception(exc);
    None
  }
}

fn cast_to_ptr<T>(t: &T) -> *mut T {
  t as *const _ as *mut _
}

/// The ValueDeserializerHeap object contains all objects related to a
/// deserializer. This object has to be pinned to the heap because of the Cpp
/// pointers that have to remain valid. Moving this object would result in the
/// Cpp pointer to the delegate to become invalid and thus causing the delegate
/// callback to fail. Additionally the deserializer and implementation are also
/// pinned in memory because these have to be accessable from within the
/// delegate callback methods.
pub struct ValueDeserializerHeap<'a> {
  value_deserializer_impl: Box<dyn ValueDeserializerImpl + 'a>,
  cxx_value_deserializer: CxxValueDeserializer,
  cxx_value_deserializer_delegate: CxxValueDeserializerDelegate,
  context: Global<Context>,
}

impl ValueDeserializerHeap<'_> {
  fn get_cxx_value_deserializer_delegate_offset()
  -> FieldOffset<CxxValueDeserializerDelegate> {
    let buf = std::mem::MaybeUninit::<Self>::uninit();
    let delegate =
      unsafe { addr_of!((*buf.as_ptr()).cxx_value_deserializer_delegate) };
    FieldOffset::from_ptrs(buf.as_ptr(), delegate)
  }

  /// Starting from 'this' pointer a ValueDeserializerHeap ref can be created
  pub unsafe fn dispatch(
    value_serializer_delegate: &CxxValueDeserializerDelegate,
  ) -> &Self {
    unsafe {
      Self::get_cxx_value_deserializer_delegate_offset()
        .to_embedder::<Self>(value_serializer_delegate)
    }
  }
}

impl Drop for ValueDeserializerHeap<'_> {
  fn drop(&mut self) {
    unsafe {
      v8__ValueDeserializer__DESTRUCT(&mut self.cxx_value_deserializer);
    };
  }
}

/// Trait used for direct read from the deserialization buffer.
/// Mostly used by the read_host_object callback function in the
/// ValueDeserializerImpl trait to create custom deserialization logic.
pub trait ValueDeserializerHelper {
  fn get_cxx_value_deserializer(&self) -> &CxxValueDeserializer;

  fn read_header(&self, context: Local<Context>) -> Option<bool> {
    unsafe {
      v8__ValueDeserializer__ReadHeader(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        context,
      )
    }
    .into()
  }

  fn read_value<'s>(
    &self,
    context: Local<'s, Context>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      Local::from_raw(v8__ValueDeserializer__ReadValue(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        context,
      ))
    }
  }

  fn read_uint32(&self, value: &mut u32) -> bool {
    unsafe {
      v8__ValueDeserializer__ReadUint32(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        value,
      )
    }
  }

  fn read_uint64(&self, value: &mut u64) -> bool {
    unsafe {
      v8__ValueDeserializer__ReadUint64(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        value,
      )
    }
  }

  fn read_double(&self, value: &mut f64) -> bool {
    unsafe {
      v8__ValueDeserializer__ReadDouble(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        value,
      )
    }
  }

  fn read_raw_bytes(&self, length: usize) -> Option<&[u8]> {
    let mut data: *const c_void = std::ptr::null_mut();
    let ok = unsafe {
      v8__ValueDeserializer__ReadRawBytes(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        length,
        &mut data,
      )
    };
    if ok {
      assert!(!data.is_null());
      unsafe { Some(std::slice::from_raw_parts(data as *const u8, length)) }
    } else {
      None
    }
  }

  fn transfer_array_buffer(
    &self,
    transfer_id: u32,
    array_buffer: Local<ArrayBuffer>,
  ) {
    unsafe {
      v8__ValueDeserializer__TransferArrayBuffer(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        transfer_id,
        array_buffer,
      );
    };
  }

  fn transfer_shared_array_buffer(
    &self,
    transfer_id: u32,
    shared_array_buffer: Local<SharedArrayBuffer>,
  ) {
    unsafe {
      v8__ValueDeserializer__TransferSharedArrayBuffer(
        cast_to_ptr(self.get_cxx_value_deserializer()),
        transfer_id,
        shared_array_buffer,
      );
    };
  }

  fn get_wire_format_version(&self) -> u32 {
    unsafe {
      v8__ValueDeserializer__GetWireFormatVersion(cast_to_ptr(
        self.get_cxx_value_deserializer(),
      ))
    }
  }
}

impl ValueDeserializerHelper for CxxValueDeserializer {
  fn get_cxx_value_deserializer(&self) -> &CxxValueDeserializer {
    self
  }
}

impl ValueDeserializerHelper for ValueDeserializerHeap<'_> {
  fn get_cxx_value_deserializer(&self) -> &CxxValueDeserializer {
    &self.cxx_value_deserializer
  }
}

impl ValueDeserializerHelper for ValueDeserializer<'_> {
  fn get_cxx_value_deserializer(&self) -> &CxxValueDeserializer {
    &self.value_deserializer_heap.cxx_value_deserializer
  }
}

/// ValueDeserializer is a stack object used as entry-point for an owned and
/// pinned heap object ValueDeserializerHeap.
/// The 'a lifetime is the lifetime of the ValueDeserializerImpl implementation.
/// The 's lifetime is the lifetime of the HandleScope which is used to retrieve
/// a Local<'s, Context> for the CallbackScopes
pub struct ValueDeserializer<'a> {
  value_deserializer_heap: Pin<Box<ValueDeserializerHeap<'a>>>,
  // ValueDeserializerHeap is already !Send and !Sync
  // but this is just making it explicit
  _phantom: std::marker::PhantomData<*mut ()>,
}

impl<'a> ValueDeserializer<'a> {
  pub fn new<D: ValueDeserializerImpl + 'a>(
    scope: &mut HandleScope,
    value_deserializer_impl: Box<D>,
    data: &[u8],
  ) -> Self {
    let context = scope.get_current_context();
    // create dummy ValueDeserializerHeap and move to heap + pin to address
    let value_deserializer_heap_ptr =
      Box::into_raw(Box::new(ValueDeserializerHeap {
        value_deserializer_impl,
        cxx_value_deserializer: CxxValueDeserializer {
          _cxx_vtable: CxxVTable(std::ptr::null()),
        },
        cxx_value_deserializer_delegate: CxxValueDeserializerDelegate {
          _cxx_vtable: CxxVTable(std::ptr::null()),
        },
        context: Global::new(scope, context),
      }));

    unsafe {
      let delegate_ptr = std::ptr::addr_of_mut!(
        (*value_deserializer_heap_ptr).cxx_value_deserializer_delegate
      );
      let deserializer_ptr = std::ptr::addr_of_mut!(
        (*value_deserializer_heap_ptr).cxx_value_deserializer
      );
      v8__ValueDeserializer__Delegate__CONSTRUCT(
        delegate_ptr
          .cast::<std::mem::MaybeUninit<CxxValueDeserializerDelegate>>(),
      );

      v8__ValueDeserializer__CONSTRUCT(
        deserializer_ptr.cast::<std::mem::MaybeUninit<CxxValueDeserializer>>(),
        scope.get_isolate_ptr(),
        data.as_ptr(),
        data.len(),
        delegate_ptr,
      );
    };

    // SAFETY: pointer from Box::into_raw is valid
    let value_deserializer_heap =
      Pin::new(unsafe { Box::from_raw(value_deserializer_heap_ptr) });

    ValueDeserializer {
      value_deserializer_heap,
      _phantom: std::marker::PhantomData,
    }
  }
}

impl ValueDeserializer<'_> {
  pub fn set_supports_legacy_wire_format(
    &self,
    supports_legacy_wire_format: bool,
  ) {
    unsafe {
      v8__ValueDeserializer__SetSupportsLegacyWireFormat(
        cast_to_ptr(&self.value_deserializer_heap.cxx_value_deserializer),
        supports_legacy_wire_format,
      );
    }
  }

  pub fn read_value<'t>(
    &self,
    context: Local<'t, Context>,
  ) -> Option<Local<'t, Value>> {
    self.value_deserializer_heap.read_value(context)
  }
}
