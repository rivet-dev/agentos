use crate::HandleScope;
use crate::Isolate;
use crate::Local;
use crate::String;
use crate::binding::v8__String__kMaxLength;
use crate::support::Opaque;
use crate::support::char;
use crate::support::int;
use crate::support::size_t;
use std::borrow::Cow;
use std::convert::TryInto;
use std::default::Default;
use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::slice;

unsafe extern "C" {
  fn v8__String__Empty(isolate: *mut Isolate) -> *const String;

  fn v8__String__NewFromUtf8(
    isolate: *mut Isolate,
    data: *const char,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__NewFromOneByte(
    isolate: *mut Isolate,
    data: *const u8,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__NewFromTwoByte(
    isolate: *mut Isolate,
    data: *const u16,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__Length(this: *const String) -> int;

  fn v8__String__Utf8Length(this: *const String, isolate: *mut Isolate) -> int;

  fn v8__String__Write(
    this: *const String,
    isolate: *mut Isolate,
    buffer: *mut u16,
    start: int,
    length: int,
    options: WriteOptions,
  ) -> int;

  fn v8__String__Write_v2(
    this: *const String,
    isolate: *mut Isolate,
    offset: u32,
    length: u32,
    buffer: *mut u16,
    flags: int,
  );

  fn v8__String__WriteOneByte(
    this: *const String,
    isolate: *mut Isolate,
    buffer: *mut u8,
    start: int,
    length: int,
    options: WriteOptions,
  ) -> int;

  fn v8__String__WriteOneByte_v2(
    this: *const String,
    isolate: *mut Isolate,
    offset: u32,
    length: u32,
    buffer: *mut u8,
    flags: int,
  );

  fn v8__String__WriteUtf8(
    this: *const String,
    isolate: *mut Isolate,
    buffer: *mut char,
    length: int,
    nchars_ref: *mut int,
    options: WriteOptions,
  ) -> int;

  fn v8__String__WriteUtf8_v2(
    this: *const String,
    isolate: *mut Isolate,
    buffer: *mut char,
    capacity: size_t,
    flags: int,
    processed_characters_return: *mut size_t,
  ) -> int;

  fn v8__String__GetExternalStringResource(
    this: *const String,
  ) -> *mut ExternalStringResource;
  fn v8__String__GetExternalStringResourceBase(
    this: *const String,
    encoding: *mut Encoding,
  ) -> *mut ExternalStringResourceBase;

  fn v8__String__NewExternalOneByteConst(
    isolate: *mut Isolate,
    onebyte_const: *const OneByteConst,
  ) -> *const String;

  fn v8__String__NewExternalOneByteStatic(
    isolate: *mut Isolate,
    buffer: *const char,
    length: int,
  ) -> *const String;

  fn v8__String__NewExternalOneByte(
    isolate: *mut Isolate,
    buffer: *mut char,
    length: size_t,
    free: unsafe extern "C" fn(*mut char, size_t),
  ) -> *const String;

  fn v8__String__NewExternalTwoByteStatic(
    isolate: *mut Isolate,
    buffer: *const u16,
    length: int,
  ) -> *const String;

  #[allow(dead_code)]
  fn v8__String__IsExternal(this: *const String) -> bool;
  fn v8__String__IsExternalOneByte(this: *const String) -> bool;
  fn v8__String__IsExternalTwoByte(this: *const String) -> bool;
  #[allow(dead_code)]
  fn v8__String__IsOneByte(this: *const String) -> bool;
  fn v8__String__ContainsOnlyOneByte(this: *const String) -> bool;
  fn v8__ExternalOneByteStringResource__data(
    this: *const ExternalOneByteStringResource,
  ) -> *const char;
  fn v8__ExternalOneByteStringResource__length(
    this: *const ExternalOneByteStringResource,
  ) -> size_t;

  fn v8__String__ValueView__CONSTRUCT(
    buf: *mut ValueView,
    isolate: *mut Isolate,
    string: *const String,
  );
  fn v8__String__ValueView__DESTRUCT(this: *mut ValueView);
  fn v8__String__ValueView__is_one_byte(this: *const ValueView) -> bool;
  fn v8__String__ValueView__data(this: *const ValueView) -> *const c_void;
  fn v8__String__ValueView__length(this: *const ValueView) -> int;
}

#[derive(PartialEq, Debug)]
#[repr(C)]
pub enum Encoding {
  Unknown = 0x1,
  TwoByte = 0x2,
  OneByte = 0x8,
}

#[repr(C)]
pub struct ExternalStringResource(Opaque);

#[repr(C)]
pub struct ExternalStringResourceBase(Opaque);

#[repr(C)]
/// An external, one-byte string resource.
/// This corresponds with `v8::String::ExternalOneByteStringResource`.
///
/// Note: The data contained in a one-byte string resource is guaranteed to be
/// Latin-1 data. It is not safe to assume that it is valid UTF-8, as Latin-1
/// only has commonality with UTF-8 in the ASCII range and differs beyond that.
pub struct ExternalOneByteStringResource(Opaque);

impl ExternalOneByteStringResource {
  /// Returns a pointer to the data owned by this resource.
  /// This pointer is valid as long as the resource is alive.
  /// The data is guaranteed to be Latin-1.
  pub fn data(&self) -> *const char {
    unsafe { v8__ExternalOneByteStringResource__data(self) }
  }

  /// Returns the length of the data owned by this resource.
  pub fn length(&self) -> usize {
    unsafe { v8__ExternalOneByteStringResource__length(self) }
  }

  /// Returns the data owned by this resource as a string slice.
  /// The data is guaranteed to be Latin-1.
  pub fn as_bytes(&self) -> &[u8] {
    let len = self.length();
    if len == 0 {
      &[]
    } else {
      // SAFETY: We know this is Latin-1
      unsafe { std::slice::from_raw_parts(self.data().cast(), len) }
    }
  }
}

/// A static ASCII string resource for usage in V8, created at build time.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct OneByteConst {
  vtable: *const OneByteConstNoOp,
  cached_data: *const char,
  length: usize,
}

impl OneByteConst {
  /// `const` function that returns this string as a string reference.
  #[inline(always)]
  pub const fn as_str(&self) -> &str {
    if self.length == 0 {
      ""
    } else {
      // SAFETY: We know this is ASCII and length > 0
      unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(
          self.cached_data as _,
          self.length,
        ))
      }
    }
  }
}

impl AsRef<str> for OneByteConst {
  #[inline(always)]
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl AsRef<[u8]> for OneByteConst {
  #[inline(always)]
  fn as_ref(&self) -> &[u8] {
    self.as_str().as_bytes()
  }
}

impl std::ops::Deref for OneByteConst {
  type Target = str;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    self.as_ref()
  }
}

// SAFETY: The vtable for OneByteConst is an immutable static and all
// of the included functions are thread-safe, the cached_data pointer
// is never changed and points to a static ASCII string, and the
// length is likewise never changed. Thus, it is safe to share the
// OneByteConst across threads. This means that multiple isolates
// can use the same OneByteConst statics simultaneously.
unsafe impl Sync for OneByteConst {}

unsafe extern "C" fn one_byte_const_no_op(_this: *const OneByteConst) {}
unsafe extern "C" fn one_byte_const_is_cacheable(
  _this: *const OneByteConst,
) -> bool {
  true
}
unsafe extern "C" fn one_byte_const_data(
  this: *const OneByteConst,
) -> *const char {
  // SAFETY: Only called from C++ with a valid OneByteConst pointer.
  unsafe { (*this).cached_data }
}
unsafe extern "C" fn one_byte_const_length(this: *const OneByteConst) -> usize {
  // SAFETY: Only called from C++ with a valid OneByteConst pointer.
  unsafe { (*this).length }
}
unsafe extern "C" fn one_byte_const_unaccount(
  _this: *const OneByteConst,
  _isolate: *mut Isolate,
) {
}
unsafe extern "C" fn one_byte_const_estimate_memory_usage(
  _this: *const OneByteConst,
) -> size_t {
  usize::MAX // ExternalStringResource::kDefaultMemoryEstimate
}
unsafe extern "C" fn one_byte_const_estimate_shared_memory_usage(
  _this: *const OneByteConst,
  _recorder: *mut (),
) {
}

type OneByteConstNoOp = unsafe extern "C" fn(*const OneByteConst);
type OneByteConstIsCacheable =
  unsafe extern "C" fn(*const OneByteConst) -> bool;
type OneByteConstData =
  unsafe extern "C" fn(*const OneByteConst) -> *const char;
type OneByteConstLength = unsafe extern "C" fn(*const OneByteConst) -> usize;
type OneByteConstUnaccount =
  unsafe extern "C" fn(*const OneByteConst, *mut Isolate);
type OneByteConstEstimateMemoryUsage =
  unsafe extern "C" fn(*const OneByteConst) -> size_t;
type OneByteConstEstimateSharedMemoryUsage =
  unsafe extern "C" fn(*const OneByteConst, *mut ());

#[repr(C)]
struct OneByteConstVtable {
  #[cfg(target_family = "windows")]
  // In SysV / Itanium ABI -0x10 offset of the vtable
  // tells how many bytes the vtable pointer pointing to
  // this vtable is offset from the base class. For
  // single inheritance this is always 0.
  _offset_to_top: usize,
  // In Itanium ABI the -0x08 offset contains the type_info
  // pointer, and in MSVC it contains the RTTI Complete Object
  // Locator pointer. V8 is normally compiled with `-fno-rtti`
  // meaning that this pointer is a nullptr on both
  // Itanium and MSVC.
  _typeinfo: *const (),
  // After the metadata fields come the virtual function
  // pointers. The vtable pointer in a class instance points
  // to the first virtual function pointer, making this
  // the 0x00 offset of the table.
  // The order of the virtual function pointers is determined
  // by their order of declaration in the classes.
  delete1: OneByteConstNoOp,
  // In SysV / Itanium ABI, a class vtable includes the
  // deleting destructor and the compete object destructor.
  // In MSVC, it only includes the deleting destructor.
  #[cfg(not(target_family = "windows"))]
  delete2: OneByteConstNoOp,
  is_cacheable: OneByteConstIsCacheable,
  unaccount: OneByteConstUnaccount,
  estimate_memory_usage: OneByteConstEstimateMemoryUsage,
  estimate_shared_memory_usage: OneByteConstEstimateSharedMemoryUsage,
  dispose: OneByteConstNoOp,
  lock: OneByteConstNoOp,
  unlock: OneByteConstNoOp,
  data: OneByteConstData,
  length: OneByteConstLength,
}

const ONE_BYTE_CONST_VTABLE: OneByteConstVtable = OneByteConstVtable {
  #[cfg(target_family = "windows")]
  _offset_to_top: 0,
  _typeinfo: std::ptr::null(),
  delete1: one_byte_const_no_op,
  #[cfg(not(target_family = "windows"))]
  delete2: one_byte_const_no_op,
  is_cacheable: one_byte_const_is_cacheable,
  unaccount: one_byte_const_unaccount,
  estimate_memory_usage: one_byte_const_estimate_memory_usage,
  estimate_shared_memory_usage: one_byte_const_estimate_shared_memory_usage,
  dispose: one_byte_const_no_op,
  lock: one_byte_const_no_op,
  unlock: one_byte_const_no_op,
  data: one_byte_const_data,
  length: one_byte_const_length,
};

#[repr(C)]
#[derive(Debug, Default)]
pub enum NewStringType {
  #[default]
  Normal,
  Internalized,
}

bitflags! {
  #[derive(Clone, Copy, Default)]
  #[repr(transparent)]
  pub struct WriteOptions: int {
    const NO_OPTIONS = 0;
    const HINT_MANY_WRITES_EXPECTED = 1;
    const NO_NULL_TERMINATION = 2;
    const PRESERVE_ONE_BYTE_NULL = 4;
    // Used by WriteUtf8 to replace orphan surrogate code units with the
    // unicode replacement character. Needs to be set to guarantee valid UTF-8
    // output.
    const REPLACE_INVALID_UTF8 = 8;
  }
}

bitflags! {
  #[derive(Clone, Copy, Default)]
  #[repr(transparent)]
  pub struct WriteFlags: int {
    const kNullTerminate = crate::binding::v8_String_WriteFlags_kNullTerminate as _;
    const kReplaceInvalidUtf8 = crate::binding::v8_String_WriteFlags_kReplaceInvalidUtf8 as _;
  }
}

impl String {
  /// The maximum length (in bytes) of a buffer that a v8::String can be built
  /// from. Attempting to create a v8::String from a larger buffer will result
  /// in None being returned.
  pub const MAX_LENGTH: usize = v8__String__kMaxLength as _;

  #[inline(always)]
  pub fn empty<'s>(scope: &mut HandleScope<'s, ()>) -> Local<'s, String> {
    // FIXME(bnoordhuis) v8__String__Empty() is infallible so there
    // is no need to box up the result, only to unwrap it again.
    unsafe { scope.cast_local(|sd| v8__String__Empty(sd.get_isolate_ptr())) }
      .unwrap()
  }

  /// Allocates a new string from UTF-8 data. Only returns an empty value when
  /// length > kMaxLength
  #[inline(always)]
  pub fn new_from_utf8<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: &[u8],
    new_type: NewStringType,
  ) -> Option<Local<'s, String>> {
    if buffer.is_empty() {
      return Some(Self::empty(scope));
    }
    let buffer_len = buffer.len().try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewFromUtf8(
          sd.get_isolate_ptr(),
          buffer.as_ptr() as *const char,
          new_type,
          buffer_len,
        )
      })
    }
  }

  /// Allocates a new string from Latin-1 data.  Only returns an empty value when
  /// length > kMaxLength.
  #[inline(always)]
  pub fn new_from_one_byte<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: &[u8],
    new_type: NewStringType,
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len().try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewFromOneByte(
          sd.get_isolate_ptr(),
          buffer.as_ptr(),
          new_type,
          buffer_len,
        )
      })
    }
  }

  /// Allocates a new string from UTF-16 data. Only returns an empty value when
  /// length > kMaxLength.
  #[inline(always)]
  pub fn new_from_two_byte<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: &[u16],
    new_type: NewStringType,
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len().try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewFromTwoByte(
          sd.get_isolate_ptr(),
          buffer.as_ptr(),
          new_type,
          buffer_len,
        )
      })
    }
  }

  /// Returns the number of characters (UTF-16 code units) in this string.
  #[inline(always)]
  pub fn length(&self) -> usize {
    unsafe { v8__String__Length(self) as usize }
  }

  /// Returns the number of bytes in the UTF-8 encoded representation of this
  /// string.
  #[inline(always)]
  pub fn utf8_length(&self, scope: &mut Isolate) -> usize {
    unsafe { v8__String__Utf8Length(self, scope) as usize }
  }

  /// Writes the contents of the string to an external buffer, as 16-bit
  /// (UTF-16) character codes.
  #[inline(always)]
  #[deprecated = "Use `v8::String::write_v2` instead"]
  pub fn write(
    &self,
    scope: &mut Isolate,
    buffer: &mut [u16],
    start: usize,
    options: WriteOptions,
  ) -> usize {
    unsafe {
      v8__String__Write(
        self,
        scope,
        buffer.as_mut_ptr(),
        start.try_into().unwrap_or(int::MAX),
        buffer.len().try_into().unwrap_or(int::MAX),
        options,
      ) as usize
    }
  }

  /// Writes the contents of the string to an external buffer, as 16-bit
  /// (UTF-16) character codes.
  #[inline(always)]
  pub fn write_v2(
    &self,
    scope: &mut Isolate,
    offset: u32,
    buffer: &mut [u16],
    flags: WriteFlags,
  ) {
    unsafe {
      v8__String__Write_v2(
        self,
        scope,
        offset,
        self.length().min(buffer.len()) as _,
        buffer.as_mut_ptr(),
        flags.bits(),
      )
    }
  }

  /// Writes the contents of the string to an external buffer, as one-byte
  /// (Latin-1) characters.
  #[inline(always)]
  #[deprecated = "Use `v8::String::write_one_byte_v2` instead."]
  pub fn write_one_byte(
    &self,
    scope: &mut Isolate,
    buffer: &mut [u8],
    start: usize,
    options: WriteOptions,
  ) -> usize {
    unsafe {
      v8__String__WriteOneByte(
        self,
        scope,
        buffer.as_mut_ptr(),
        start.try_into().unwrap_or(int::MAX),
        buffer.len().try_into().unwrap_or(int::MAX),
        options,
      ) as usize
    }
  }

  /// Writes the contents of the string to an external buffer, as one-byte
  /// (Latin-1) characters.
  #[inline(always)]
  pub fn write_one_byte_v2(
    &self,
    scope: &mut Isolate,
    offset: u32,
    buffer: &mut [u8],
    flags: WriteFlags,
  ) {
    unsafe {
      v8__String__WriteOneByte_v2(
        self,
        scope,
        offset,
        self.length().min(buffer.len()) as _,
        buffer.as_mut_ptr(),
        flags.bits(),
      )
    }
  }

  /// Writes the contents of the string to an external [`MaybeUninit`] buffer, as one-byte
  /// (Latin-1) characters.
  #[inline(always)]
  #[deprecated = "Use `v8::String::write_one_byte_uninit_v2` instead."]
  pub fn write_one_byte_uninit(
    &self,
    scope: &mut Isolate,
    buffer: &mut [MaybeUninit<u8>],
    start: usize,
    options: WriteOptions,
  ) -> usize {
    unsafe {
      v8__String__WriteOneByte(
        self,
        scope,
        buffer.as_mut_ptr() as *mut u8,
        start.try_into().unwrap_or(int::MAX),
        buffer.len().try_into().unwrap_or(int::MAX),
        options,
      ) as usize
    }
  }

  /// Writes the contents of the string to an external [`MaybeUninit`] buffer, as one-byte
  /// (Latin-1) characters.
  #[inline(always)]
  pub fn write_one_byte_uninit_v2(
    &self,
    scope: &mut Isolate,
    offset: u32,
    buffer: &mut [MaybeUninit<u8>],
    flags: WriteFlags,
  ) {
    unsafe {
      v8__String__WriteOneByte_v2(
        self,
        scope,
        offset,
        self.length().min(buffer.len()) as _,
        buffer.as_mut_ptr() as _,
        flags.bits(),
      )
    }
  }

  /// Writes the contents of the string to an external buffer, as UTF-8.
  #[inline(always)]
  #[deprecated = "Use `v8::String::write_utf8_v2` instead."]
  pub fn write_utf8(
    &self,
    scope: &mut Isolate,
    buffer: &mut [u8],
    nchars_ref: Option<&mut usize>,
    options: WriteOptions,
  ) -> usize {
    unsafe {
      // SAFETY:
      // We assume that v8 will overwrite the buffer without de-initializing any byte in it.
      // So the type casting of the buffer is safe.
      let buffer = {
        let len = buffer.len();
        let data = buffer.as_mut_ptr().cast();
        slice::from_raw_parts_mut(data, len)
      };
      #[allow(deprecated)]
      self.write_utf8_uninit(scope, buffer, nchars_ref, options)
    }
  }

  /// Writes the contents of the string to an external buffer, as UTF-8.
  #[inline(always)]
  pub fn write_utf8_v2(
    &self,
    scope: &mut Isolate,
    buffer: &mut [u8],
    flags: WriteFlags,
    processed_characters_return: Option<&mut usize>,
  ) -> usize {
    unsafe {
      // SAFETY:
      // We assume that v8 will overwrite the buffer without de-initializing any byte in it.
      // So the type casting of the buffer is safe.

      let buffer = {
        let len = buffer.len();
        let data = buffer.as_mut_ptr().cast();
        slice::from_raw_parts_mut(data, len)
      };
      self.write_utf8_uninit_v2(
        scope,
        buffer,
        flags,
        processed_characters_return,
      )
    }
  }

  /// Writes the contents of the string to an external [`MaybeUninit`] buffer, as UTF-8.
  #[deprecated = "Use `v8::String::write_utf8_uninit_v2` instead."]
  pub fn write_utf8_uninit(
    &self,
    scope: &mut Isolate,
    buffer: &mut [MaybeUninit<u8>],
    nchars_ref: Option<&mut usize>,
    options: WriteOptions,
  ) -> usize {
    let mut nchars_ref_int: int = 0;
    let bytes = unsafe {
      v8__String__WriteUtf8(
        self,
        scope,
        buffer.as_mut_ptr() as *mut char,
        buffer.len().try_into().unwrap_or(int::MAX),
        &mut nchars_ref_int,
        options,
      )
    };
    if let Some(r) = nchars_ref {
      *r = nchars_ref_int as usize;
    }
    bytes as usize
  }

  /// Writes the contents of the string to an external [`MaybeUninit`] buffer, as UTF-8.
  pub fn write_utf8_uninit_v2(
    &self,
    scope: &mut Isolate,
    buffer: &mut [MaybeUninit<u8>],
    flags: WriteFlags,
    processed_characters_return: Option<&mut usize>,
  ) -> usize {
    let bytes = unsafe {
      v8__String__WriteUtf8_v2(
        self,
        scope,
        buffer.as_mut_ptr() as _,
        buffer.len(),
        flags.bits(),
        processed_characters_return
          .map(|p| p as *mut _)
          .unwrap_or(std::ptr::null_mut()),
      )
    };
    bytes as usize
  }

  // Convenience function not present in the original V8 API.
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s, ()>,
    value: &str,
  ) -> Option<Local<'s, String>> {
    Self::new_from_utf8(scope, value.as_ref(), NewStringType::Normal)
  }

  /// Compile-time function to create an external string resource.
  /// The buffer is checked to contain only ASCII characters.
  #[inline(always)]
  pub const fn create_external_onebyte_const(
    buffer: &'static [u8],
  ) -> OneByteConst {
    // Assert that the buffer contains only ASCII, and that the
    // length is less or equal to (64-bit) v8::String::kMaxLength.
    assert!(buffer.is_ascii() && buffer.len() <= ((1 << 29) - 24));
    OneByteConst {
      vtable: &ONE_BYTE_CONST_VTABLE.delete1,
      cached_data: buffer.as_ptr() as *const char,
      length: buffer.len(),
    }
  }

  /// Compile-time function to create an external string resource which
  /// skips the ASCII and length checks.
  ///
  /// ## Safety
  ///
  /// The passed in buffer must contain only ASCII data. Note that while V8
  /// allows OneByte string resources to contain Latin-1 data, the OneByteConst
  /// struct does not allow it.
  #[inline(always)]
  pub const unsafe fn create_external_onebyte_const_unchecked(
    buffer: &'static [u8],
  ) -> OneByteConst {
    OneByteConst {
      vtable: &ONE_BYTE_CONST_VTABLE.delete1,
      cached_data: buffer.as_ptr() as *const char,
      length: buffer.len(),
    }
  }

  /// Creates a v8::String from a `&'static OneByteConst`
  /// which is guaranteed to be ASCII.
  ///
  /// Note that OneByteConst guarantees ASCII even though V8 would allow
  /// OneByte string resources to contain Latin-1.
  #[inline(always)]
  pub fn new_from_onebyte_const<'s>(
    scope: &mut HandleScope<'s, ()>,
    onebyte_const: &'static OneByteConst,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalOneByteConst(sd.get_isolate_ptr(), onebyte_const)
      })
    }
  }

  /// Creates a v8::String from a `&'static [u8]`,
  /// must be Latin-1 or ASCII, not UTF-8!
  #[inline(always)]
  pub fn new_external_onebyte_static<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: &'static [u8],
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len().try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalOneByteStatic(
          sd.get_isolate_ptr(),
          buffer.as_ptr() as *const char,
          buffer_len,
        )
      })
    }
  }

  /// Creates a `v8::String` from owned bytes.
  /// The bytes must be Latin-1 or ASCII.
  /// V8 will take ownership of the buffer and free it when the string is garbage collected.
  #[inline(always)]
  pub fn new_external_onebyte<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: Box<[u8]>,
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len();
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalOneByte(
          sd.get_isolate_ptr(),
          Box::into_raw(buffer).cast::<char>(),
          buffer_len,
          free_rust_external_onebyte,
        )
      })
    }
  }

  /// Creates a `v8::String` from owned bytes, length, and a custom destructor.
  /// The bytes must be Latin-1 or ASCII.
  /// V8 will take ownership of the buffer and free it when the string is garbage collected.
  ///
  /// SAFETY: `buffer` must be owned (valid for the lifetime of the string), and
  /// `destructor` must be a valid function pointer that can free the buffer.
  /// The destructor will be called with the buffer and length when the string is garbage collected.
  #[inline(always)]
  pub unsafe fn new_external_onebyte_raw<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: *mut char,
    buffer_len: usize,
    destructor: unsafe extern "C" fn(*mut char, usize),
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalOneByte(
          sd.get_isolate_ptr(),
          buffer,
          buffer_len,
          destructor,
        )
      })
    }
  }

  /// Creates a v8::String from a `&'static [u16]`.
  #[inline(always)]
  pub fn new_external_twobyte_static<'s>(
    scope: &mut HandleScope<'s, ()>,
    buffer: &'static [u16],
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len().try_into().ok()?;
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalTwoByteStatic(
          sd.get_isolate_ptr(),
          buffer.as_ptr(),
          buffer_len,
        )
      })
    }
  }

  /// Get the ExternalStringResource for an external string.
  ///
  /// Returns None if is_external() doesn't return true.
  pub fn get_external_string_resource(
    &self,
  ) -> Option<NonNull<ExternalStringResource>> {
    NonNull::new(unsafe { v8__String__GetExternalStringResource(self) })
  }

  /// Get the ExternalOneByteStringResource for an external one-byte string.
  ///
  /// Returns None if is_external_onebyte() doesn't return true.
  pub fn get_external_onebyte_string_resource(
    &self,
  ) -> Option<NonNull<ExternalOneByteStringResource>> {
    let (base, encoding) = self.get_external_string_resource_base();
    let base = base?;
    if encoding != Encoding::OneByte {
      return None;
    }

    Some(base.cast())
  }

  /// Get the ExternalStringResourceBase for an external string.
  /// Note this is just the base class, and isn't very useful on its own.
  /// You'll want to downcast to one of its subclasses, for instance
  /// with `get_external_onebyte_string_resource`.
  pub fn get_external_string_resource_base(
    &self,
  ) -> (Option<NonNull<ExternalStringResourceBase>>, Encoding) {
    let mut encoding = Encoding::Unknown;
    (
      NonNull::new(unsafe {
        v8__String__GetExternalStringResourceBase(self, &mut encoding)
      }),
      encoding,
    )
  }

  /// True if string is external
  #[inline(always)]
  pub fn is_external(&self) -> bool {
    // TODO: re-enable on next v8-release
    // Right now it fallbacks to Value::IsExternal, which is incorrect
    // See: https://source.chromium.org/chromium/_/chromium/v8/v8.git/+/1dd8624b524d14076160c1743f7da0b20fbe68e0
    // unsafe { v8__String__IsExternal(self) }

    // Fallback for now (though functionally identical)
    self.is_external_onebyte() || self.is_external_twobyte()
  }

  /// True if string is external & one-byte
  /// (e.g: created with new_external_onebyte_static)
  #[inline(always)]
  pub fn is_external_onebyte(&self) -> bool {
    unsafe { v8__String__IsExternalOneByte(self) }
  }

  /// True if string is external & two-byte
  /// (e.g: created with new_external_twobyte_static)
  #[inline(always)]
  pub fn is_external_twobyte(&self) -> bool {
    unsafe { v8__String__IsExternalTwoByte(self) }
  }

  /// Will return true if and only if string is known for certain to contain only one-byte data,
  /// ie: Latin-1, a.k.a. ISO-8859-1 code points. Doesn't read the string so can return false
  /// negatives, and a return value of false does not mean this string is not one-byte data.
  ///
  /// For a method that will not return false negatives at the cost of
  /// potentially reading the entire string, use [`contains_only_onebyte()`].
  ///
  /// [`contains_only_onebyte()`]: String::contains_only_onebyte
  #[inline(always)]
  pub fn is_onebyte(&self) -> bool {
    unsafe { v8__String__IsOneByte(self) }
  }

  /// True if the string contains only one-byte data.
  /// Will read the entire string in some cases.
  #[inline(always)]
  pub fn contains_only_onebyte(&self) -> bool {
    unsafe { v8__String__ContainsOnlyOneByte(self) }
  }

  /// Creates a copy of a [`crate::String`] in a [`std::string::String`].
  /// Convenience function not present in the original V8 API.
  pub fn to_rust_string_lossy(
    &self,
    scope: &mut Isolate,
  ) -> std::string::String {
    let len_utf16 = self.length();

    // No need to allocate or do any work for zero-length strings
    if len_utf16 == 0 {
      return std::string::String::new();
    }

    let len_utf8 = self.utf8_length(scope);

    // If len_utf8 == len_utf16 and the string is one-byte, we can take the fast memcpy path. This is true iff the
    // string is 100% 7-bit ASCII.
    if self.is_onebyte() && len_utf8 == len_utf16 {
      unsafe {
        // Create an uninitialized buffer of `capacity` bytes. We need to be careful here to avoid
        // accidentally creating a slice of u8 which would be invalid.
        let layout = std::alloc::Layout::from_size_align(len_utf16, 1).unwrap();
        let data = std::alloc::alloc(layout) as *mut MaybeUninit<u8>;
        let buffer = std::ptr::slice_from_raw_parts_mut(data, len_utf16);

        // Write to this MaybeUninit buffer, assuming we're going to fill this entire buffer
        self.write_one_byte_uninit_v2(
          scope,
          0,
          &mut *buffer,
          WriteFlags::kReplaceInvalidUtf8,
        );

        // Return an owned string from this guaranteed now-initialized data
        let buffer = data as *mut u8;
        return std::string::String::from_raw_parts(
          buffer, len_utf16, len_utf16,
        );
      }
    }

    // SAFETY: This allocates a buffer manually using the default allocator using the string's capacity.
    // We have a large number of invariants to uphold, so please check changes to this code carefully
    unsafe {
      // Create an uninitialized buffer of `capacity` bytes. We need to be careful here to avoid
      // accidentally creating a slice of u8 which would be invalid.
      let layout = std::alloc::Layout::from_size_align(len_utf8, 1).unwrap();
      let data = std::alloc::alloc(layout) as *mut MaybeUninit<u8>;
      let buffer = std::ptr::slice_from_raw_parts_mut(data, len_utf8);

      // Write to this MaybeUninit buffer, assuming we're going to fill this entire buffer
      let length = self.write_utf8_uninit_v2(
        scope,
        &mut *buffer,
        WriteFlags::kReplaceInvalidUtf8,
        None,
      );
      debug_assert!(length == len_utf8);

      // Return an owned string from this guaranteed now-initialized data
      let buffer = data as *mut u8;
      std::string::String::from_raw_parts(buffer, length, len_utf8)
    }
  }

  /// Converts a [`crate::String`] to either an owned [`std::string::String`], or a borrowed [`str`], depending on whether it fits into the
  /// provided buffer.
  pub fn to_rust_cow_lossy<'a, const N: usize>(
    &self,
    scope: &mut Isolate,
    buffer: &'a mut [MaybeUninit<u8>; N],
  ) -> Cow<'a, str> {
    let len_utf16 = self.length();

    // No need to allocate or do any work for zero-length strings
    if len_utf16 == 0 {
      return "".into();
    }

    // TODO(mmastrac): Ideally we should be able to access the string's internal representation
    let len_utf8 = self.utf8_length(scope);

    // If len_utf8 == len_utf16 and the string is one-byte, we can take the fast memcpy path. This is true iff the
    // string is 100% 7-bit ASCII.
    if self.is_onebyte() && len_utf8 == len_utf16 {
      if len_utf16 <= N {
        self.write_one_byte_uninit_v2(scope, 0, buffer, WriteFlags::empty());
        unsafe {
          // Get a slice of &[u8] of what we know is initialized now
          let buffer = &mut buffer[..len_utf16];
          let buffer = &mut *(buffer as *mut [_] as *mut [u8]);

          // We know it's valid UTF-8, so make a string
          return Cow::Borrowed(std::str::from_utf8_unchecked(buffer));
        }
      }

      unsafe {
        // Create an uninitialized buffer of `capacity` bytes. We need to be careful here to avoid
        // accidentally creating a slice of u8 which would be invalid.
        let layout = std::alloc::Layout::from_size_align(len_utf16, 1).unwrap();
        let data = std::alloc::alloc(layout) as *mut MaybeUninit<u8>;
        let buffer = std::ptr::slice_from_raw_parts_mut(data, len_utf16);

        // Write to this MaybeUninit buffer, assuming we're going to fill this entire buffer
        self.write_one_byte_uninit_v2(
          scope,
          0,
          &mut *buffer,
          WriteFlags::kReplaceInvalidUtf8,
        );

        // Return an owned string from this guaranteed now-initialized data
        let buffer = data as *mut u8;
        return Cow::Owned(std::string::String::from_raw_parts(
          buffer, len_utf16, len_utf16,
        ));
      }
    }

    if len_utf8 <= N {
      // No malloc path
      let length = self.write_utf8_uninit_v2(
        scope,
        buffer,
        WriteFlags::kReplaceInvalidUtf8,
        None,
      );
      debug_assert!(length == len_utf8);

      // SAFETY: We know that we wrote `length` UTF-8 bytes. See `slice_assume_init_mut` for additional guarantee information.
      unsafe {
        // Get a slice of &[u8] of what we know is initialized now
        let buffer = &mut buffer[..length];
        let buffer = &mut *(buffer as *mut [_] as *mut [u8]);

        // We know it's valid UTF-8, so make a string
        return Cow::Borrowed(std::str::from_utf8_unchecked(buffer));
      }
    }

    // SAFETY: This allocates a buffer manually using the default allocator using the string's capacity.
    // We have a large number of invariants to uphold, so please check changes to this code carefully
    unsafe {
      // Create an uninitialized buffer of `capacity` bytes. We need to be careful here to avoid
      // accidentally creating a slice of u8 which would be invalid.
      let layout = std::alloc::Layout::from_size_align(len_utf8, 1).unwrap();
      let data = std::alloc::alloc(layout) as *mut MaybeUninit<u8>;
      let buffer = std::ptr::slice_from_raw_parts_mut(data, len_utf8);

      // Write to this MaybeUninit buffer, assuming we're going to fill this entire buffer
      let length = self.write_utf8_uninit_v2(
        scope,
        &mut *buffer,
        WriteFlags::kReplaceInvalidUtf8,
        None,
      );
      debug_assert!(length == len_utf8);

      // Return an owned string from this guaranteed now-initialized data
      let buffer = data as *mut u8;
      Cow::Owned(std::string::String::from_raw_parts(
        buffer, length, len_utf8,
      ))
    }
  }
}

pub unsafe extern "C" fn free_rust_external_onebyte(s: *mut char, len: usize) {
  unsafe {
    let slice = std::slice::from_raw_parts_mut(s, len);

    // Drop the slice
    drop(Box::from_raw(slice));
  }
}

#[derive(Debug, PartialEq)]
pub enum ValueViewData<'s> {
  OneByte(&'s [u8]),
  TwoByte(&'s [u16]),
}

/// Returns a view onto a string's contents.
///
/// WARNING: This does not copy the string's contents, and will therefore be
/// invalidated if the GC can move the string while the ValueView is alive. It
/// is therefore required that no GC or allocation can happen while there is an
/// active ValueView. This requirement may be relaxed in the future.
///
/// V8 strings are either encoded as one-byte or two-bytes per character.
#[repr(C)]
pub struct ValueView<'s>(
  [u8; crate::binding::v8__String__ValueView_SIZE],
  PhantomData<&'s ()>,
);

impl<'s> ValueView<'s> {
  #[inline(always)]
  pub fn new(isolate: &mut Isolate, string: Local<'s, String>) -> Self {
    let mut v = std::mem::MaybeUninit::uninit();
    unsafe {
      v8__String__ValueView__CONSTRUCT(v.as_mut_ptr(), isolate, &*string);
      v.assume_init()
    }
  }

  #[inline(always)]
  pub fn data(&self) -> ValueViewData<'_> {
    unsafe {
      let data = v8__String__ValueView__data(self);
      let length = v8__String__ValueView__length(self) as usize;
      if v8__String__ValueView__is_one_byte(self) {
        ValueViewData::OneByte(std::slice::from_raw_parts(data as _, length))
      } else {
        ValueViewData::TwoByte(std::slice::from_raw_parts(data as _, length))
      }
    }
  }
}

impl Drop for ValueView<'_> {
  fn drop(&mut self) {
    unsafe { v8__String__ValueView__DESTRUCT(self) }
  }
}
