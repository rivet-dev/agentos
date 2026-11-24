use crate::ArrayBuffer;
use crate::ArrayBufferView;
use crate::BackingStore;
use crate::Local;
use crate::SharedRef;
use crate::binding::memory_span_t;
use crate::scope::PinScope;
use crate::support::int;
use std::convert::TryInto;
use std::ffi::c_void;
use std::mem::MaybeUninit;

unsafe extern "C" {
  fn v8__ArrayBufferView__Buffer(
    this: *const ArrayBufferView,
  ) -> *const ArrayBuffer;
  fn v8__ArrayBufferView__Buffer__Data(
    this: *const ArrayBufferView,
  ) -> *mut c_void;
  fn v8__ArrayBufferView__ByteLength(this: *const ArrayBufferView) -> usize;
  fn v8__ArrayBufferView__ByteOffset(this: *const ArrayBufferView) -> usize;
  fn v8__ArrayBufferView__HasBuffer(this: *const ArrayBufferView) -> bool;
  fn v8__ArrayBufferView__CopyContents(
    this: *const ArrayBufferView,
    dest: *mut c_void,
    byte_length: int,
  ) -> usize;
  fn v8__ArrayBufferView__GetContents(
    this: *const ArrayBufferView,
    storage: memory_span_t,
  ) -> memory_span_t;
}

impl ArrayBufferView {
  /// Returns underlying ArrayBuffer.
  #[inline(always)]
  pub fn buffer<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Option<Local<'s, ArrayBuffer>> {
    unsafe { scope.cast_local(|_| v8__ArrayBufferView__Buffer(self)) }
  }

  /// Returns true if ArrayBufferView's backing ArrayBuffer has already been allocated.
  pub fn has_buffer(&self) -> bool {
    unsafe { v8__ArrayBufferView__HasBuffer(self) }
  }

  /// Get a shared pointer to the backing store of this array buffer. This
  /// pointer coordinates the lifetime management of the internal storage
  /// with any live ArrayBuffers on the heap, even across isolates. The embedder
  /// should not attempt to manage lifetime of the storage through other means.
  #[inline(always)]
  pub fn get_backing_store(&self) -> Option<SharedRef<BackingStore>> {
    let buffer = unsafe { v8__ArrayBufferView__Buffer(self) };
    unsafe { buffer.as_ref().map(|buffer| buffer.get_backing_store()) }
  }

  /// Returns the underlying storage for this `ArrayBufferView`, including the built-in `byte_offset`.
  /// This is a more efficient way of calling `buffer(scope)->data()`, and may be called without a
  /// scope.
  #[inline(always)]
  pub fn data(&self) -> *mut c_void {
    unsafe {
      v8__ArrayBufferView__Buffer__Data(self)
        .add(v8__ArrayBufferView__ByteOffset(self))
    }
  }

  /// Size of a view in bytes.
  #[inline(always)]
  pub fn byte_length(&self) -> usize {
    unsafe { v8__ArrayBufferView__ByteLength(self) }
  }

  /// Byte offset in |Buffer|.
  #[inline(always)]
  pub fn byte_offset(&self) -> usize {
    unsafe { v8__ArrayBufferView__ByteOffset(self) }
  }

  /// Copy the contents of the ArrayBufferView's buffer to an embedder defined
  /// memory without additional overhead that calling ArrayBufferView::Buffer
  /// might incur.
  /// Returns the number of bytes actually written.
  #[inline(always)]
  pub fn copy_contents(&self, dest: &mut [u8]) -> usize {
    unsafe {
      v8__ArrayBufferView__CopyContents(
        self,
        dest.as_mut_ptr() as *mut c_void,
        dest.len().try_into().unwrap(),
      )
    }
  }

  /// Copy the contents of the ArrayBufferView's buffer to an embedder defined
  /// memory without additional overhead that calling ArrayBufferView::Buffer
  /// might incur.
  /// Returns the number of bytes actually written.
  #[inline(always)]
  pub fn copy_contents_uninit(&self, dest: &mut [MaybeUninit<u8>]) -> usize {
    unsafe {
      v8__ArrayBufferView__CopyContents(
        self,
        dest.as_mut_ptr() as *mut c_void,
        dest.len().try_into().unwrap(),
      )
    }
  }

  /// Returns the contents of the ArrayBufferView's buffer as a MemorySpan. If
  /// the contents are on the V8 heap, they get copied into `storage`. Otherwise
  /// a view into the off-heap backing store is returned. The provided storage
  /// should be at least as large as the maximum on-heap size of a TypedArray,
  /// which is available as `v8::TYPED_ARRAY_MAX_SIZE_IN_HEAP`.
  #[inline(always)]
  pub unsafe fn get_contents_raw_parts(
    &self,
    storage: &mut [u8],
  ) -> (*mut u8, usize) {
    unsafe {
      let span = v8__ArrayBufferView__GetContents(
        self,
        memory_span_t {
          data: storage.as_mut_ptr(),
          size: storage.len(),
        },
      );
      (span.data, span.size)
    }
  }

  /// Returns the contents of the ArrayBufferView's buffer as a MemorySpan. If
  /// the contents are on the V8 heap, they get copied into `storage`. Otherwise
  /// a view into the off-heap backing store is returned. The provided storage
  /// should be at least as large as the maximum on-heap size of a TypedArray,
  /// which is available as `v8::TYPED_ARRAY_MAX_SIZE_IN_HEAP`.
  #[inline(always)]
  pub fn get_contents<'s, 'a>(&'s self, storage: &'a mut [u8]) -> &'a [u8]
  where
    's: 'a,
  {
    unsafe {
      let (data, size) = self.get_contents_raw_parts(storage);
      if data.is_null() {
        debug_assert_eq!(size, 0);
        std::slice::from_raw_parts(std::ptr::dangling(), size)
      } else {
        std::slice::from_raw_parts(data, size)
      }
    }
  }
}
