// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use std::cell::Cell;
use std::ffi::c_void;
use std::ops::Deref;
use std::ptr::NonNull;
use std::ptr::null;
use std::slice;

use crate::ArrayBuffer;
use crate::DataView;
use crate::Isolate;
use crate::IsolateGroup;
use crate::Local;
use crate::SharedArrayBuffer;
use crate::Value;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;
use crate::support::MaybeBool;
use crate::support::Opaque;
use crate::support::Shared;
use crate::support::SharedPtrBase;
use crate::support::SharedRef;
use crate::support::UniquePtr;
use crate::support::UniqueRef;
use crate::support::long;

unsafe extern "C" {
  fn v8__ArrayBuffer__Allocator__NewDefaultAllocator() -> *mut Allocator;
  fn v8__ArrayBuffer__Allocator__NewDefaultAllocator__with_group(
    group: *const IsolateGroup,
  ) -> *mut Allocator;
  fn v8__ArrayBuffer__Allocator__DELETE(this: *mut Allocator);
  fn v8__ArrayBuffer__New__with_byte_length(
    isolate: *mut RealIsolate,
    byte_length: usize,
  ) -> *const ArrayBuffer;
  fn v8__ArrayBuffer__MaybeNew(
    isolate: *mut RealIsolate,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
  ) -> *const ArrayBuffer;
  fn v8__ArrayBuffer__New__with_backing_store(
    isolate: *mut RealIsolate,
    backing_store: *const SharedRef<BackingStore>,
  ) -> *const ArrayBuffer;
  fn v8__ArrayBuffer__Detach(
    this: *const ArrayBuffer,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__ArrayBuffer__SetDetachKey(this: *const ArrayBuffer, key: *const Value);
  fn v8__ArrayBuffer__Data(this: *const ArrayBuffer) -> *mut c_void;
  fn v8__ArrayBuffer__IsDetachable(this: *const ArrayBuffer) -> bool;
  fn v8__ArrayBuffer__WasDetached(this: *const ArrayBuffer) -> bool;
  fn v8__ArrayBuffer__IsResizableByUserJavaScript(
    this: *const ArrayBuffer,
  ) -> bool;
  fn v8__ArrayBuffer__IsImmutable(this: *const ArrayBuffer) -> bool;
  fn v8__ArrayBuffer__ByteLength(this: *const ArrayBuffer) -> usize;
  fn v8__ArrayBuffer__GetBackingStore(
    this: *const ArrayBuffer,
  ) -> SharedRef<BackingStore>;
  fn v8__ArrayBuffer__NewBackingStore__with_byte_length(
    isolate: *mut RealIsolate,
    byte_length: usize,
  ) -> *mut BackingStore;
  fn v8__ArrayBuffer__NewBackingStore__with_mode(
    isolate: *mut RealIsolate,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
    on_failure: BackingStoreOnFailureMode,
  ) -> *mut BackingStore;
  fn v8__ArrayBuffer__NewResizableBackingStore(
    byte_length: usize,
    max_byte_length: usize,
  ) -> *mut BackingStore;
  fn v8__ArrayBuffer__NewBackingStore__with_data(
    data: *mut c_void,
    byte_length: usize,
    deleter: BackingStoreDeleterCallback,
    deleter_data: *mut c_void,
  ) -> *mut BackingStore;
  fn v8__BackingStore__Data(this: *const BackingStore) -> *mut c_void;
  fn v8__BackingStore__ByteLength(this: *const BackingStore) -> usize;
  fn v8__BackingStore__MaxByteLength(this: *const BackingStore) -> usize;
  fn v8__BackingStore__IsShared(this: *const BackingStore) -> bool;
  fn v8__BackingStore__IsResizableByUserJavaScript(
    this: *const BackingStore,
  ) -> bool;
  fn v8__BackingStore__DELETE(this: *mut BackingStore);

  fn v8__DataView__New(
    arraybuffer: *const ArrayBuffer,
    byte_offset: usize,
    length: usize,
  ) -> *const DataView;
  fn v8__DataView__New__with_shared_buffer(
    shared_array_buffer: *const SharedArrayBuffer,
    byte_offset: usize,
    length: usize,
  ) -> *const DataView;

  fn std__shared_ptr__v8__BackingStore__COPY(
    ptr: *const SharedPtrBase<BackingStore>,
  ) -> SharedPtrBase<BackingStore>;
  fn std__shared_ptr__v8__BackingStore__CONVERT__std__unique_ptr(
    unique_ptr: UniquePtr<BackingStore>,
  ) -> SharedPtrBase<BackingStore>;
  fn std__shared_ptr__v8__BackingStore__get(
    ptr: *const SharedPtrBase<BackingStore>,
  ) -> *mut BackingStore;
  fn std__shared_ptr__v8__BackingStore__reset(
    ptr: *mut SharedPtrBase<BackingStore>,
  );
  fn std__shared_ptr__v8__BackingStore__use_count(
    ptr: *const SharedPtrBase<BackingStore>,
  ) -> long;

  fn std__shared_ptr__v8__ArrayBuffer__Allocator__COPY(
    ptr: *const SharedPtrBase<Allocator>,
  ) -> SharedPtrBase<Allocator>;
  fn std__shared_ptr__v8__ArrayBuffer__Allocator__CONVERT__std__unique_ptr(
    unique_ptr: UniquePtr<Allocator>,
  ) -> SharedPtrBase<Allocator>;
  fn std__shared_ptr__v8__ArrayBuffer__Allocator__get(
    ptr: *const SharedPtrBase<Allocator>,
  ) -> *mut Allocator;
  fn std__shared_ptr__v8__ArrayBuffer__Allocator__reset(
    ptr: *mut SharedPtrBase<Allocator>,
  );
  fn std__shared_ptr__v8__ArrayBuffer__Allocator__use_count(
    ptr: *const SharedPtrBase<Allocator>,
  ) -> long;
}

// Rust allocator feature is only available in non-sandboxed mode
#[cfg(not(feature = "v8_enable_sandbox"))]
unsafe extern "C" {
  fn v8__ArrayBuffer__Allocator__NewRustAllocator(
    handle: *const c_void,
    vtable: *const RustAllocatorVtable<c_void>,
  ) -> *mut Allocator;
}

/// Whether the memory of a newly allocated backing store is zero-initialized
/// or left uninitialized.
///
/// Requesting uninitialized memory is faster, but the embedder must then make
/// sure the contents are never observed before they are written. That
/// obligation cannot be checked, so the constructors that leave memory
/// uninitialized — [`ArrayBuffer::maybe_new_uninitialized`],
/// [`ArrayBuffer::new_backing_store_uninitialized`] and
/// [`SharedArrayBuffer::new_backing_store_uninitialized`] — are `unsafe`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackingStoreInitializationMode {
  #[default]
  ZeroInitialized,
  Uninitialized,
}

/// What a backing store allocation should do when it cannot satisfy the
/// request, even after garbage collection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackingStoreOnFailureMode {
  /// Return no backing store, letting the caller handle the failure.
  ReturnNull,
  /// Crash the process with an out-of-memory error.
  #[default]
  OutOfMemory,
}

/// A thread-safe allocator that V8 uses to allocate |ArrayBuffer|'s memory.
/// The allocator is a global V8 setting. It has to be set via
/// Isolate::CreateParams.
///
/// Memory allocated through this allocator by V8 is accounted for as external
/// memory by V8. Note that V8 keeps track of the memory for all internalized
/// |ArrayBuffer|s. Responsibility for tracking external memory (using
/// Isolate::AdjustAmountOfExternalAllocatedMemory) is handed over to the
/// embedder upon externalization and taken over upon internalization (creating
/// an internalized buffer from an existing buffer).
///
/// Note that it is unsafe to call back into V8 from any of the allocator
/// functions.
///
/// This is called v8::ArrayBuffer::Allocator in C++. Rather than use the
/// namespace array_buffer, which will contain only the Allocator we opt in Rust
/// to allow it to live in the top level: v8::Allocator
#[repr(C)]
#[derive(Debug)]
pub struct Allocator(Opaque);

/// A wrapper around the V8 Allocator class.
#[cfg(not(feature = "v8_enable_sandbox"))]
#[repr(C)]
pub struct RustAllocatorVtable<T> {
  pub allocate: unsafe extern "C" fn(handle: &T, len: usize) -> *mut c_void,
  pub allocate_uninitialized:
    unsafe extern "C" fn(handle: &T, len: usize) -> *mut c_void,
  pub free: unsafe extern "C" fn(handle: &T, data: *mut c_void, len: usize),
  pub drop: unsafe extern "C" fn(handle: *const T),
}

impl Shared for Allocator {
  fn clone(ptr: &SharedPtrBase<Self>) -> SharedPtrBase<Self> {
    unsafe { std__shared_ptr__v8__ArrayBuffer__Allocator__COPY(ptr) }
  }
  fn from_unique_ptr(unique_ptr: UniquePtr<Self>) -> SharedPtrBase<Self> {
    unsafe {
      std__shared_ptr__v8__ArrayBuffer__Allocator__CONVERT__std__unique_ptr(
        unique_ptr,
      )
    }
  }
  fn get(ptr: &SharedPtrBase<Self>) -> *const Self {
    unsafe { std__shared_ptr__v8__ArrayBuffer__Allocator__get(ptr) }
  }
  fn reset(ptr: &mut SharedPtrBase<Self>) {
    unsafe { std__shared_ptr__v8__ArrayBuffer__Allocator__reset(ptr) }
  }
  fn use_count(ptr: &SharedPtrBase<Self>) -> long {
    unsafe { std__shared_ptr__v8__ArrayBuffer__Allocator__use_count(ptr) }
  }
}

/// malloc/free based convenience allocator.
#[inline(always)]
pub fn new_default_allocator() -> UniqueRef<Allocator> {
  unsafe {
    UniqueRef::from_raw(v8__ArrayBuffer__Allocator__NewDefaultAllocator())
  }
}

/// Convenience allocator scoped to an isolate group.
///
/// When the sandbox is enabled, this allocator allocates its backing memory
/// inside the sandbox belonging to `group`; otherwise it relies on
/// malloc/free.
///
/// V8 only declares this overload when it is built with pointer compression in
/// multi-cage mode, the same configuration that
/// [`IsolateGroup::can_create_new_groups`] reports on. `None` is returned when
/// the V8 this crate was linked against was built differently.
#[inline(always)]
pub fn new_default_allocator_for_group(
  group: &IsolateGroup,
) -> Option<UniqueRef<Allocator>> {
  let ptr = unsafe {
    v8__ArrayBuffer__Allocator__NewDefaultAllocator__with_group(group)
  };
  if ptr.is_null() {
    None
  } else {
    Some(unsafe { UniqueRef::from_raw(ptr) })
  }
}

/// Creates an allocator managed by Rust code.
///
/// Marked `unsafe` because the caller must ensure that `handle` is valid and matches what `vtable` expects.
///
/// Not usable in sandboxed mode
#[inline(always)]
#[cfg(not(feature = "v8_enable_sandbox"))]
pub unsafe fn new_rust_allocator<T: Sized + Send + Sync + 'static>(
  handle: *const T,
  vtable: &'static RustAllocatorVtable<T>,
) -> UniqueRef<Allocator> {
  unsafe {
    UniqueRef::from_raw(v8__ArrayBuffer__Allocator__NewRustAllocator(
      handle as *const c_void,
      vtable as *const RustAllocatorVtable<T>
        as *const RustAllocatorVtable<c_void>,
    ))
  }
}

#[test]
#[cfg(not(feature = "v8_enable_sandbox"))]
fn test_rust_allocator() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  unsafe extern "C" fn allocate(_: &AtomicUsize, _: usize) -> *mut c_void {
    unimplemented!()
  }
  unsafe extern "C" fn allocate_uninitialized(
    _: &AtomicUsize,
    _: usize,
  ) -> *mut c_void {
    unimplemented!()
  }
  unsafe extern "C" fn free(_: &AtomicUsize, _: *mut c_void, _: usize) {
    unimplemented!()
  }
  unsafe extern "C" fn drop(x: *const AtomicUsize) {
    unsafe {
      let arc = Arc::from_raw(x);
      arc.store(42, Ordering::SeqCst);
    }
  }

  let retval = Arc::new(AtomicUsize::new(0));

  let vtable: &'static RustAllocatorVtable<AtomicUsize> =
    &RustAllocatorVtable {
      allocate,
      allocate_uninitialized,
      free,
      drop,
    };
  unsafe { new_rust_allocator(Arc::into_raw(retval.clone()), vtable) };
  assert_eq!(retval.load(Ordering::SeqCst), 42);
  assert_eq!(Arc::strong_count(&retval), 1);
}

#[test]
fn test_default_allocator() {
  crate::initialize_v8();
  new_default_allocator();
}

impl Drop for Allocator {
  fn drop(&mut self) {
    unsafe { v8__ArrayBuffer__Allocator__DELETE(self) };
  }
}

pub type BackingStoreDeleterCallback = unsafe extern "C" fn(
  data: *mut c_void,
  byte_length: usize,
  deleter_data: *mut c_void,
);

#[cfg(not(feature = "v8_enable_sandbox"))]
pub(crate) mod sealed {
  pub trait Rawable {
    fn byte_len(&mut self) -> usize;
    fn into_raw(self) -> (*const (), *const u8);
    unsafe fn drop_raw(ptr: *const (), size: usize);
  }
}

#[cfg(not(feature = "v8_enable_sandbox"))]
macro_rules! rawable {
  ($ty:ty) => {
    impl sealed::Rawable for Box<[$ty]> {
      fn byte_len(&mut self) -> usize {
        self.as_mut().len() * std::mem::size_of::<$ty>()
      }

      fn into_raw(mut self) -> (*const (), *const u8) {
        // Thin the fat pointer
        let ptr = self.as_mut_ptr();
        std::mem::forget(self);
        (ptr as _, ptr as _)
      }

      unsafe fn drop_raw(ptr: *const (), len: usize) {
        // Fatten the thin pointer
        _ = unsafe {
          Self::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as _, len))
        };
      }
    }

    impl sealed::Rawable for Vec<$ty> {
      fn byte_len(&mut self) -> usize {
        Vec::<$ty>::len(self) * std::mem::size_of::<$ty>()
      }

      unsafe fn drop_raw(ptr: *const (), size: usize) {
        unsafe {
          <Box<[$ty]> as sealed::Rawable>::drop_raw(ptr, size);
        }
      }

      fn into_raw(self) -> (*const (), *const u8) {
        self.into_boxed_slice().into_raw()
      }
    }
  };
}

#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(u8);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(u16);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(u32);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(u64);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(i8);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(i16);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(i32);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(i64);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(f32);
#[cfg(not(feature = "v8_enable_sandbox"))]
rawable!(f64);

#[cfg(not(feature = "v8_enable_sandbox"))]
impl<T: Sized> sealed::Rawable for Box<T>
where
  T: AsMut<[u8]>,
{
  fn byte_len(&mut self) -> usize {
    self.as_mut().as_mut().len()
  }

  fn into_raw(mut self) -> (*const (), *const u8) {
    let data = self.as_mut().as_mut().as_mut_ptr();
    let ptr = Self::into_raw(self);
    (ptr as _, data)
  }

  unsafe fn drop_raw(ptr: *const (), _len: usize) {
    unsafe {
      _ = Self::from_raw(ptr as _);
    }
  }
}

/// A wrapper around the backing store (i.e. the raw memory) of an array buffer.
/// See a document linked in http://crbug.com/v8/9908 for more information.
///
/// The allocation and destruction of backing stores is generally managed by
/// V8. Clients should always use standard C++ memory ownership types (i.e.
/// std::unique_ptr and std::shared_ptr) to manage lifetimes of backing stores
/// properly, since V8 internal objects may alias backing stores.
///
/// This object does not keep the underlying |ArrayBuffer::Allocator| alive by
/// default. Use Isolate::CreateParams::array_buffer_allocator_shared when
/// creating the Isolate to make it hold a reference to the allocator itself.
#[repr(C)]
#[derive(Debug)]
pub struct BackingStore([usize; 6]);

unsafe impl Send for BackingStore {}

impl BackingStore {
  /// Return a pointer to the beginning of the memory block for this backing
  /// store. The pointer is only valid as long as this backing store object
  /// lives.
  ///
  /// Might return `None` if the backing store has zero length.
  #[inline(always)]
  pub fn data(&self) -> Option<NonNull<c_void>> {
    let raw_ptr =
      unsafe { v8__BackingStore__Data(self as *const _ as *mut Self) };
    NonNull::new(raw_ptr)
  }

  /// The length (in bytes) of this backing store.
  #[inline(always)]
  pub fn byte_length(&self) -> usize {
    unsafe { v8__BackingStore__ByteLength(self) }
  }

  /// The maximum length (in bytes) that this backing store can grow to.
  ///
  /// For a non-resizable backing store this is the same as
  /// [`BackingStore::byte_length`].
  #[inline(always)]
  pub fn max_byte_length(&self) -> usize {
    unsafe { v8__BackingStore__MaxByteLength(self) }
  }

  /// Indicates whether the backing store was created for an ArrayBuffer or
  /// a SharedArrayBuffer.
  #[inline(always)]
  pub fn is_shared(&self) -> bool {
    unsafe { v8__BackingStore__IsShared(self) }
  }

  /// Indicates whether the backing store was created for a resizable ArrayBuffer
  /// or a growable SharedArrayBuffer, and thus may be resized by user
  /// JavaScript code.
  #[inline(always)]
  pub fn is_resizable_by_user_javascript(&self) -> bool {
    unsafe { v8__BackingStore__IsResizableByUserJavaScript(self) }
  }
}

impl Deref for BackingStore {
  type Target = [Cell<u8>];

  /// Returns a [u8] slice refencing the data in the backing store.
  #[inline]
  fn deref(&self) -> &Self::Target {
    // We use a dangling pointer if `self.data()` returns None because it's UB
    // to create even an empty slice from a null pointer.
    let data = self
      .data()
      .unwrap_or_else(NonNull::dangling)
      .cast::<Cell<u8>>();
    let len = self.byte_length();
    unsafe { slice::from_raw_parts(data.as_ptr(), len) }
  }
}

impl Drop for BackingStore {
  #[inline]
  fn drop(&mut self) {
    unsafe { v8__BackingStore__DELETE(self) };
  }
}

impl Shared for BackingStore {
  #[inline]
  fn clone(ptr: &SharedPtrBase<Self>) -> SharedPtrBase<Self> {
    unsafe { std__shared_ptr__v8__BackingStore__COPY(ptr) }
  }
  #[inline]
  fn from_unique_ptr(unique_ptr: UniquePtr<Self>) -> SharedPtrBase<Self> {
    unsafe {
      std__shared_ptr__v8__BackingStore__CONVERT__std__unique_ptr(unique_ptr)
    }
  }
  #[inline]
  fn get(ptr: &SharedPtrBase<Self>) -> *const Self {
    unsafe { std__shared_ptr__v8__BackingStore__get(ptr) }
  }
  #[inline]
  fn reset(ptr: &mut SharedPtrBase<Self>) {
    unsafe { std__shared_ptr__v8__BackingStore__reset(ptr) }
  }
  #[inline]
  fn use_count(ptr: &SharedPtrBase<Self>) -> long {
    unsafe { std__shared_ptr__v8__BackingStore__use_count(ptr) }
  }
}

impl ArrayBuffer {
  /// The maximum length (in bytes) of an `ArrayBuffer`, i.e. the largest value
  /// that can be passed to [`ArrayBuffer::new`].
  ///
  /// This is V8's `ArrayBuffer::kMaxByteLength`, which V8 also exposes as
  /// `TypedArray::kMaxByteLength`.
  pub const MAX_BYTE_LENGTH: usize =
    crate::binding::v8__TypedArray__kMaxByteLength;

  /// Create a new ArrayBuffer. Allocate |byte_length| bytes.
  /// Allocated memory will be owned by a created ArrayBuffer and
  /// will be deallocated when it is garbage-collected,
  /// unless the object is externalized.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    byte_length: usize,
  ) -> Local<'s, ArrayBuffer> {
    unsafe {
      scope.cast_local(|sd| {
        v8__ArrayBuffer__New__with_byte_length(
          sd.get_isolate_ptr(),
          byte_length,
        )
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn with_backing_store<'s>(
    scope: &PinScope<'s, '_, ()>,
    backing_store: &SharedRef<BackingStore>,
  ) -> Local<'s, ArrayBuffer> {
    unsafe {
      scope.cast_local(|sd| {
        v8__ArrayBuffer__New__with_backing_store(
          sd.get_isolate_ptr(),
          backing_store,
        )
      })
    }
    .unwrap()
  }

  /// Create a new ArrayBuffer, allocating |byte_length| zero-initialized
  /// bytes.
  ///
  /// Unlike [`ArrayBuffer::new`], `None` is returned if the allocation fails
  /// rather than crashing the process.
  #[inline(always)]
  pub fn maybe_new<'s>(
    scope: &PinScope<'s, '_, ()>,
    byte_length: usize,
  ) -> Option<Local<'s, ArrayBuffer>> {
    unsafe {
      Self::maybe_new_with_mode(
        scope,
        byte_length,
        BackingStoreInitializationMode::ZeroInitialized,
      )
    }
  }

  /// Like [`ArrayBuffer::maybe_new`], but the newly allocated bytes are left
  /// uninitialized, which is faster but leaves whatever the allocator handed
  /// out visible in the buffer.
  ///
  /// # Safety
  ///
  /// The contents of the returned ArrayBuffer are uninitialized memory. The
  /// caller must overwrite every byte before the buffer is read, either from
  /// Rust or by JavaScript; in particular the buffer must not be made
  /// reachable from script until it has been fully initialized. Reading it
  /// beforehand is undefined behaviour and may disclose unrelated process
  /// memory.
  #[inline(always)]
  pub unsafe fn maybe_new_uninitialized<'s>(
    scope: &PinScope<'s, '_, ()>,
    byte_length: usize,
  ) -> Option<Local<'s, ArrayBuffer>> {
    unsafe {
      Self::maybe_new_with_mode(
        scope,
        byte_length,
        BackingStoreInitializationMode::Uninitialized,
      )
    }
  }

  /// # Safety
  ///
  /// See [`ArrayBuffer::maybe_new_uninitialized`]: passing
  /// [`BackingStoreInitializationMode::Uninitialized`] yields an ArrayBuffer
  /// whose contents must be fully written before they are read or exposed.
  #[inline(always)]
  unsafe fn maybe_new_with_mode<'s>(
    scope: &PinScope<'s, '_, ()>,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
  ) -> Option<Local<'s, ArrayBuffer>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__ArrayBuffer__MaybeNew(
          sd.get_isolate_ptr(),
          byte_length,
          initialization_mode,
        )
      })
    }
  }

  /// Data length in bytes.
  #[inline(always)]
  pub fn byte_length(&self) -> usize {
    unsafe { v8__ArrayBuffer__ByteLength(self) }
  }

  /// Returns true if this ArrayBuffer may be resized by user JavaScript code,
  /// i.e. whether it is a resizable ArrayBuffer. Equivalent to
  /// `get_backing_store().is_resizable_by_user_javascript()`.
  #[inline(always)]
  pub fn is_resizable_by_user_javascript(&self) -> bool {
    unsafe { v8__ArrayBuffer__IsResizableByUserJavaScript(self) }
  }

  /// Returns true if this ArrayBuffer is immutable.
  #[inline(always)]
  pub fn is_immutable(&self) -> bool {
    unsafe { v8__ArrayBuffer__IsImmutable(self) }
  }

  /// Returns true if this ArrayBuffer may be detached.
  #[inline(always)]
  pub fn is_detachable(&self) -> bool {
    unsafe { v8__ArrayBuffer__IsDetachable(self) }
  }

  /// Returns true if this ArrayBuffer was detached.
  #[inline(always)]
  pub fn was_detached(&self) -> bool {
    if self.byte_length() != 0 {
      return false;
    }
    unsafe { v8__ArrayBuffer__WasDetached(self) }
  }

  /// Detaches this ArrayBuffer and all its views (typed arrays).
  /// Detaching sets the byte length of the buffer and all typed arrays to zero,
  /// preventing JavaScript from ever accessing underlying backing store.
  /// ArrayBuffer should have been externalized and must be detachable. Returns
  /// `None` if the key didn't pass the `[[ArrayBufferDetachKey]]` check,
  /// and `Some(true)` otherwise.
  #[inline(always)]
  pub fn detach(&self, key: Option<Local<Value>>) -> Option<bool> {
    // V8 terminates when the ArrayBuffer is not detachable. Non-detachable
    // buffers are buffers that are in use by WebAssembly or asm.js.
    if self.is_detachable() {
      let key = key.map_or(null(), |v| &*v as *const Value);
      unsafe { v8__ArrayBuffer__Detach(self, key) }.into()
    } else {
      Some(true)
    }
  }

  /// Sets the `[[ArrayBufferDetachKey]]`.
  #[inline(always)]
  pub fn set_detach_key(&self, key: Local<Value>) {
    unsafe { v8__ArrayBuffer__SetDetachKey(self, &*key) };
  }

  /// More efficient shortcut for GetBackingStore()->Data().
  /// The returned pointer is valid as long as the ArrayBuffer is alive.
  #[inline(always)]
  pub fn data(&self) -> Option<NonNull<c_void>> {
    let raw_ptr = unsafe { v8__ArrayBuffer__Data(self) };
    NonNull::new(raw_ptr)
  }

  /// Get a shared pointer to the backing store of this array buffer. This
  /// pointer coordinates the lifetime management of the internal storage
  /// with any live ArrayBuffers on the heap, even across isolates. The embedder
  /// should not attempt to manage lifetime of the storage through other means.
  #[inline(always)]
  pub fn get_backing_store(&self) -> SharedRef<BackingStore> {
    unsafe { v8__ArrayBuffer__GetBackingStore(self) }
  }

  /// Returns a new standalone BackingStore that is allocated using the array
  /// buffer allocator of the isolate. The result can be later passed to
  /// ArrayBuffer::New.
  ///
  /// If the allocator returns nullptr, then the function may cause GCs in the
  /// given isolate and re-try the allocation. If GCs do not help, then the
  /// function will crash with an out-of-memory error.
  #[inline(always)]
  pub fn new_backing_store(
    scope: &mut Isolate,
    byte_length: usize,
  ) -> UniqueRef<BackingStore> {
    unsafe {
      UniqueRef::from_raw(v8__ArrayBuffer__NewBackingStore__with_byte_length(
        (*scope).as_real_ptr(),
        byte_length,
      ))
    }
  }

  /// Returns a new zero-initialized standalone BackingStore that is allocated
  /// using the array buffer allocator of the isolate, with control over what
  /// happens when the allocation fails.
  ///
  /// If the allocator returns null, then the function may cause GCs in the
  /// given isolate and re-try the allocation.
  ///
  /// If `on_failure` is [`BackingStoreOnFailureMode::OutOfMemory`] and GCs do
  /// not help, then the process crashes with an out-of-memory error. If it is
  /// [`BackingStoreOnFailureMode::ReturnNull`], `None` is returned instead.
  #[inline(always)]
  pub fn new_backing_store_with_mode(
    isolate: &mut Isolate,
    byte_length: usize,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    unsafe {
      Self::new_backing_store_with_modes(
        isolate,
        byte_length,
        BackingStoreInitializationMode::ZeroInitialized,
        on_failure,
      )
    }
  }

  /// Like [`ArrayBuffer::new_backing_store_with_mode`], but the allocated
  /// memory is left uninitialized, which is faster but leaves whatever the
  /// allocator handed out visible in the backing store.
  ///
  /// # Safety
  ///
  /// The returned backing store contains uninitialized memory, which its safe
  /// `Deref<Target = [Cell<u8>]>` would otherwise let anyone read. The caller
  /// must overwrite every byte before the backing store is read, either from
  /// Rust or through an ArrayBuffer handed to JavaScript. Reading it
  /// beforehand is undefined behaviour and may disclose unrelated process
  /// memory.
  #[inline(always)]
  pub unsafe fn new_backing_store_uninitialized(
    isolate: &mut Isolate,
    byte_length: usize,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    unsafe {
      Self::new_backing_store_with_modes(
        isolate,
        byte_length,
        BackingStoreInitializationMode::Uninitialized,
        on_failure,
      )
    }
  }

  /// # Safety
  ///
  /// See [`ArrayBuffer::new_backing_store_uninitialized`]: passing
  /// [`BackingStoreInitializationMode::Uninitialized`] yields a backing store
  /// whose contents must be fully written before they are read or exposed.
  #[inline(always)]
  unsafe fn new_backing_store_with_modes(
    isolate: &mut Isolate,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    let ptr = unsafe {
      v8__ArrayBuffer__NewBackingStore__with_mode(
        isolate.as_real_ptr(),
        byte_length,
        initialization_mode,
        on_failure,
      )
    };
    if ptr.is_null() {
      None
    } else {
      Some(unsafe { UniqueRef::from_raw(ptr) })
    }
  }

  /// Returns a new resizable standalone BackingStore that is allocated using
  /// the array buffer allocator of the isolate. The result can be later passed
  /// to [`ArrayBuffer::with_backing_store`].
  ///
  /// `byte_length` must be `<= max_byte_length`.
  ///
  /// This function is usable without an isolate. Unlike the isolate-scoped
  /// constructors, GCs cannot be triggered and there are no retries;
  /// allocation failure crashes the process with an out-of-memory error.
  #[inline(always)]
  pub fn new_resizable_backing_store(
    byte_length: usize,
    max_byte_length: usize,
  ) -> UniqueRef<BackingStore> {
    unsafe {
      UniqueRef::from_raw(v8__ArrayBuffer__NewResizableBackingStore(
        byte_length,
        max_byte_length,
      ))
    }
  }

  /// Returns a new standalone BackingStore that takes over the ownership of
  /// the given buffer.
  ///
  /// The destructor of the BackingStore frees owned buffer memory.
  ///
  /// The result can be later passed to ArrayBuffer::New. The raw pointer
  /// to the buffer must not be passed again to any V8 API function.
  ///
  /// Not available in Sandbox Mode, see new_backing_store_from_bytes for a potential alternative
  #[inline(always)]
  #[cfg(not(feature = "v8_enable_sandbox"))]
  pub fn new_backing_store_from_boxed_slice(
    data: Box<[u8]>,
  ) -> UniqueRef<BackingStore> {
    Self::new_backing_store_from_bytes(data)
  }

  /// Returns a new standalone BackingStore that takes over the ownership of
  /// the given buffer.
  ///
  /// The destructor of the BackingStore frees owned buffer memory.
  ///
  /// The result can be later passed to ArrayBuffer::New. The raw pointer
  /// to the buffer must not be passed again to any V8 API function.
  ///
  /// Not available in Sandbox Mode, see new_backing_store_from_bytes for a potential alternative
  #[inline(always)]
  #[cfg(not(feature = "v8_enable_sandbox"))]
  pub fn new_backing_store_from_vec(data: Vec<u8>) -> UniqueRef<BackingStore> {
    Self::new_backing_store_from_bytes(data)
  }

  /// Returns a new standalone BackingStore backed by a container that dereferences
  /// to a mutable slice of bytes. The object is dereferenced once, and the resulting slice's
  /// memory is used for the lifetime of the buffer.
  ///
  /// This method may be called with most single-ownership containers that implement `AsMut<[u8]>`, including
  /// `Box<[u8]>`, and `Vec<u8>`. This will also support most other mutable bytes containers (including `bytes::BytesMut`),
  /// though these buffers will need to be boxed to manage ownership of memory.
  ///
  /// Not available in sandbox mode. Sandbox mode requires data to be allocated
  /// within the sandbox's address space. Within sandbox mode, consider the below alternatives
  ///
  /// 1. consider using new_backing_store and BackingStore::data() followed by doing a std::ptr::copy to copy the data into a BackingStore.
  /// 2. If you truly do have data that is allocated inside the sandbox address space, consider using the unsafe new_backing_store_from_ptr API
  ///
  /// ```
  /// // Vector of bytes
  /// let backing_store = v8::ArrayBuffer::new_backing_store_from_bytes(vec![1, 2, 3]);
  /// // Boxes slice of bytes
  /// let boxed_slice: Box<[u8]> = vec![1, 2, 3].into_boxed_slice();
  /// let backing_store = v8::ArrayBuffer::new_backing_store_from_bytes(boxed_slice);
  /// // BytesMut from bytes crate
  /// let backing_store = v8::ArrayBuffer::new_backing_store_from_bytes(Box::new(bytes::BytesMut::new()));
  /// ```
  #[inline(always)]
  #[cfg(not(feature = "v8_enable_sandbox"))]
  pub fn new_backing_store_from_bytes<T>(
    mut bytes: T,
  ) -> UniqueRef<BackingStore>
  where
    T: sealed::Rawable,
  {
    let len = bytes.byte_len();

    let (ptr, slice) = T::into_raw(bytes);

    unsafe extern "C" fn drop_rawable<T: sealed::Rawable>(
      _ptr: *mut c_void,
      len: usize,
      data: *mut c_void,
    ) {
      // SAFETY: We know that data is a raw T from above
      unsafe { T::drop_raw(data as _, len) }
    }

    // SAFETY: We are extending the lifetime of a slice, but we're locking away the box that we
    // derefed from so there's no way to get another mutable reference.
    unsafe {
      Self::new_backing_store_from_ptr(
        slice as _,
        len,
        drop_rawable::<T>,
        ptr as _,
      )
    }
  }

  /// Returns a new standalone BackingStore backed by given ptr.
  ///
  /// SAFETY: This API consumes raw pointers so is inherently
  /// unsafe. Usually you should use new_backing_store_from_boxed_slice.
  ///
  /// WARNING: Using sandbox mode has extra limitations that may cause crashes
  /// or memory safety violations if this API is used incorrectly:
  ///
  /// 1. Sandbox mode requires data to be allocated within the sandbox's address space.
  /// 2. It is very easy to cause memory safety errors when using this API with sandbox mode
  #[inline(always)]
  pub unsafe fn new_backing_store_from_ptr(
    data_ptr: *mut c_void,
    byte_length: usize,
    deleter_callback: BackingStoreDeleterCallback,
    deleter_data: *mut c_void,
  ) -> UniqueRef<BackingStore> {
    unsafe {
      UniqueRef::from_raw(v8__ArrayBuffer__NewBackingStore__with_data(
        data_ptr,
        byte_length,
        deleter_callback,
        deleter_data,
      ))
    }
  }
}

impl DataView {
  /// Returns a new DataView.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    arraybuffer: Local<'s, ArrayBuffer>,
    byte_offset: usize,
    length: usize,
  ) -> Local<'s, DataView> {
    unsafe {
      scope
        .cast_local(|_| v8__DataView__New(&*arraybuffer, byte_offset, length))
    }
    .unwrap()
  }

  /// Returns a new DataView over a SharedArrayBuffer.
  #[inline(always)]
  pub fn new_with_shared_buffer<'s>(
    scope: &PinScope<'s, '_, ()>,
    shared_array_buffer: Local<'s, SharedArrayBuffer>,
    byte_offset: usize,
    length: usize,
  ) -> Local<'s, DataView> {
    unsafe {
      scope.cast_local(|_| {
        v8__DataView__New__with_shared_buffer(
          &*shared_array_buffer,
          byte_offset,
          length,
        )
      })
    }
    .unwrap()
  }
}
