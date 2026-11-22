// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::BackingStore;
use crate::BackingStoreDeleterCallback;
use crate::BackingStoreInitializationMode;
use crate::BackingStoreOnFailureMode;
use crate::Local;
use crate::SharedArrayBuffer;
use crate::isolate::RealIsolate;
use crate::scope::GetIsolate;
use crate::scope::PinScope;
use crate::support::SharedRef;
use crate::support::UniqueRef;

unsafe extern "C" {
  fn v8__SharedArrayBuffer__New__with_byte_length(
    isolate: *mut RealIsolate,
    byte_length: usize,
  ) -> *const SharedArrayBuffer;
  fn v8__SharedArrayBuffer__New__with_backing_store(
    isolate: *mut RealIsolate,
    backing_store: *const SharedRef<BackingStore>,
  ) -> *const SharedArrayBuffer;
  fn v8__SharedArrayBuffer__ByteLength(this: *const SharedArrayBuffer)
  -> usize;
  fn v8__SharedArrayBuffer__GetBackingStore(
    this: *const SharedArrayBuffer,
  ) -> SharedRef<BackingStore>;
  fn v8__SharedArrayBuffer__NewBackingStore__with_byte_length(
    isolate: *mut RealIsolate,
    byte_length: usize,
  ) -> *mut BackingStore;
  fn v8__SharedArrayBuffer__NewBackingStore__with_mode(
    isolate: *mut RealIsolate,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
    on_failure: BackingStoreOnFailureMode,
  ) -> *mut BackingStore;
  fn v8__SharedArrayBuffer__Data(this: *const SharedArrayBuffer)
  -> *mut c_void;
  fn v8__SharedArrayBuffer__NewBackingStore__with_data(
    data: *mut c_void,
    byte_length: usize,
    deleter: BackingStoreDeleterCallback,
    deleter_data: *mut c_void,
  ) -> *mut BackingStore;
}

impl SharedArrayBuffer {
  /// Create a new SharedArrayBuffer. Allocate |byte_length| bytes.
  /// Allocated memory will be owned by a created SharedArrayBuffer and
  /// will be deallocated when it is garbage-collected,
  /// unless the object is externalized.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_>,
    byte_length: usize,
  ) -> Option<Local<'s, SharedArrayBuffer>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__SharedArrayBuffer__New__with_byte_length(
          sd.get_isolate_ptr(),
          byte_length,
        )
      })
    }
  }

  #[inline(always)]
  pub fn with_backing_store<'s>(
    scope: &PinScope<'s, '_>,
    backing_store: &SharedRef<BackingStore>,
  ) -> Local<'s, SharedArrayBuffer> {
    unsafe {
      scope.cast_local(|sd| {
        v8__SharedArrayBuffer__New__with_backing_store(
          sd.get_isolate_ptr(),
          backing_store,
        )
      })
    }
    .unwrap()
  }

  /// Data length in bytes.
  #[inline(always)]
  pub fn byte_length(&self) -> usize {
    unsafe { v8__SharedArrayBuffer__ByteLength(self) }
  }

  /// More efficient shortcut for `get_backing_store().data()`.
  /// The returned pointer is valid as long as the SharedArrayBuffer is alive.
  #[inline(always)]
  pub fn data(&self) -> Option<NonNull<c_void>> {
    let raw_ptr = unsafe { v8__SharedArrayBuffer__Data(self) };
    NonNull::new(raw_ptr)
  }

  /// Get a shared pointer to the backing store of this array buffer. This
  /// pointer coordinates the lifetime management of the internal storage
  /// with any live ArrayBuffers on the heap, even across isolates. The embedder
  /// should not attempt to manage lifetime of the storage through other means.
  #[inline(always)]
  pub fn get_backing_store(&self) -> SharedRef<BackingStore> {
    unsafe { v8__SharedArrayBuffer__GetBackingStore(self) }
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
    scope: &PinScope<'_, '_>,
    byte_length: usize,
  ) -> UniqueRef<BackingStore> {
    unsafe {
      UniqueRef::from_raw(
        v8__SharedArrayBuffer__NewBackingStore__with_byte_length(
          scope.get_isolate_ptr(),
          byte_length,
        ),
      )
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
    scope: &PinScope<'_, '_>,
    byte_length: usize,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    unsafe {
      Self::new_backing_store_with_modes(
        scope,
        byte_length,
        BackingStoreInitializationMode::ZeroInitialized,
        on_failure,
      )
    }
  }

  /// Like [`SharedArrayBuffer::new_backing_store_with_mode`], but the
  /// allocated memory is left uninitialized, which is faster but leaves
  /// whatever the allocator handed out visible in the backing store.
  ///
  /// # Safety
  ///
  /// The returned backing store contains uninitialized memory, which its safe
  /// `Deref<Target = [Cell<u8>]>` would otherwise let anyone read. The caller
  /// must overwrite every byte before the backing store is read, either from
  /// Rust or through a SharedArrayBuffer handed to JavaScript. Reading it
  /// beforehand is undefined behaviour and may disclose unrelated process
  /// memory.
  #[inline(always)]
  pub unsafe fn new_backing_store_uninitialized(
    scope: &PinScope<'_, '_>,
    byte_length: usize,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    unsafe {
      Self::new_backing_store_with_modes(
        scope,
        byte_length,
        BackingStoreInitializationMode::Uninitialized,
        on_failure,
      )
    }
  }

  /// # Safety
  ///
  /// See [`SharedArrayBuffer::new_backing_store_uninitialized`]: passing
  /// [`BackingStoreInitializationMode::Uninitialized`] yields a backing store
  /// whose contents must be fully written before they are read or exposed.
  #[inline(always)]
  unsafe fn new_backing_store_with_modes(
    scope: &PinScope<'_, '_>,
    byte_length: usize,
    initialization_mode: BackingStoreInitializationMode,
    on_failure: BackingStoreOnFailureMode,
  ) -> Option<UniqueRef<BackingStore>> {
    let ptr = unsafe {
      v8__SharedArrayBuffer__NewBackingStore__with_mode(
        scope.get_isolate_ptr(),
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

  /// Returns a new standalone BackingStore that takes over the ownership of
  /// the given buffer.
  ///
  /// The destructor of the BackingStore frees owned buffer memory.
  ///
  /// The result can be later passed to SharedArrayBuffer::New. The raw pointer
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
  /// The result can be later passed to SharedArrayBuffer::New. The raw pointer
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
  /// within the sandbox's address space. Within sandbox mode, consider the below alternatives:
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
  #[cfg(not(feature = "v8_enable_sandbox"))]
  #[inline(always)]
  pub fn new_backing_store_from_bytes<T>(
    mut bytes: T,
  ) -> UniqueRef<BackingStore>
  where
    T: crate::array_buffer::sealed::Rawable,
  {
    let len = bytes.byte_len();

    let (ptr, slice) = T::into_raw(bytes);

    unsafe extern "C" fn drop_rawable<
      T: crate::array_buffer::sealed::Rawable,
    >(
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
      UniqueRef::from_raw(v8__SharedArrayBuffer__NewBackingStore__with_data(
        data_ptr,
        byte_length,
        deleter_callback,
        deleter_data,
      ))
    }
  }
}
