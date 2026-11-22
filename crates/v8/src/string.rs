use crate::Isolate;
use crate::Local;
use crate::String;
use crate::binding::v8__String__kMaxLength;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;
use crate::support::Opaque;
use crate::support::char;
use crate::support::int;
use crate::support::size_t;
use std::borrow::Cow;
use std::convert::TryInto;
use std::default::Default;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::slice;

/// Converts Latin-1 encoded bytes to UTF-8, writing into the output buffer.
///
/// The output buffer must have at least `2 * input_length` bytes of capacity,
/// since each Latin-1 byte can expand to at most 2 UTF-8 bytes.
///
/// Returns the number of bytes written to the output buffer.
///
/// # Safety
///
/// - `inbuf` must point to at least `input_length` readable bytes.
/// - `outbuf` must point to at least `2 * input_length` writable bytes.
#[inline(always)]
pub unsafe fn latin1_to_utf8(
  input_length: usize,
  inbuf: *const u8,
  outbuf: *mut u8,
) -> usize {
  unsafe {
    let mut output = 0;
    let mut input = 0;

    // Process 8 bytes at a time: check if all are ASCII with a single AND
    while input + 8 <= input_length {
      let chunk = (inbuf.add(input) as *const u64).read_unaligned();
      if chunk & 0x8080_8080_8080_8080 == 0 {
        // All 8 bytes are ASCII, copy in bulk
        (outbuf.add(output) as *mut u64).write_unaligned(chunk);
        input += 8;
        output += 8;
      } else {
        // At least one non-ASCII byte, process individually
        let end = input + 8;
        while input < end {
          let byte = *(inbuf.add(input));
          if byte < 0x80 {
            *(outbuf.add(output)) = byte;
            output += 1;
          } else {
            // Latin-1 byte to two-byte UTF-8 sequence
            *(outbuf.add(output)) = (byte >> 6) | 0b1100_0000;
            *(outbuf.add(output + 1)) = (byte & 0b0011_1111) | 0b1000_0000;
            output += 2;
          }
          input += 1;
        }
      }
    }

    // Handle remaining bytes
    while input < input_length {
      let byte = *(inbuf.add(input));
      if byte < 0x80 {
        *(outbuf.add(output)) = byte;
        output += 1;
      } else {
        *(outbuf.add(output)) = (byte >> 6) | 0b1100_0000;
        *(outbuf.add(output + 1)) = (byte & 0b0011_1111) | 0b1000_0000;
        output += 2;
      }
      input += 1;
    }
    output
  }
}

/// Minimum non-ASCII UTF-8 byte length before `new_from_utf8` decodes with
/// simdutf instead of V8's decoder. Below this the two potential simdutf FFI
/// calls cost more than V8 handling the tiny string itself.
const NONASCII_ENCODE_SIMD_THRESHOLD: usize = 16;

unsafe extern "C" {
  fn v8__String__Empty(isolate: *mut RealIsolate) -> *const String;

  fn v8__String__NewFromUtf8(
    isolate: *mut RealIsolate,
    data: *const char,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__NewFromOneByte(
    isolate: *mut RealIsolate,
    data: *const u8,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__NewFromTwoByte(
    isolate: *mut RealIsolate,
    data: *const u16,
    new_type: NewStringType,
    length: int,
  ) -> *const String;

  fn v8__String__Concat(
    isolate: *mut RealIsolate,
    left: *const String,
    right: *const String,
  ) -> *const String;

  fn v8__String__Length(this: *const String) -> int;

  fn v8__String__Utf8Length(
    this: *const String,
    isolate: *mut RealIsolate,
  ) -> int;

  fn v8__String__Write_v2(
    this: *const String,
    isolate: *mut RealIsolate,
    offset: u32,
    length: u32,
    buffer: *mut u16,
    flags: int,
  );

  fn v8__String__WriteOneByte_v2(
    this: *const String,
    isolate: *mut RealIsolate,
    offset: u32,
    length: u32,
    buffer: *mut u8,
    flags: int,
  );

  fn v8__String__WriteUtf8_v2(
    this: *const String,
    isolate: *mut RealIsolate,
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
    isolate: *mut RealIsolate,
    onebyte_const: *const OneByteConst,
  ) -> *const String;

  fn v8__String__NewExternalOneByteStatic(
    isolate: *mut RealIsolate,
    buffer: *const char,
    length: int,
  ) -> *const String;

  fn v8__String__NewExternalOneByte(
    isolate: *mut RealIsolate,
    buffer: *mut char,
    length: size_t,
    free: unsafe extern "C" fn(*mut char, size_t),
  ) -> *const String;

  fn v8__String__NewExternalTwoByteStatic(
    isolate: *mut RealIsolate,
    buffer: *const u16,
    length: int,
  ) -> *const String;

  fn v8__String__NewExternalTwoByte(
    isolate: *mut RealIsolate,
    buffer: *mut u16,
    length: size_t,
    free: unsafe extern "C" fn(*mut u16, size_t),
  ) -> *const String;

  #[allow(dead_code)]
  fn v8__String__IsExternal(this: *const String) -> bool;
  fn v8__String__IsExternalOneByte(this: *const String) -> bool;
  fn v8__String__IsExternalTwoByte(this: *const String) -> bool;
  #[allow(dead_code)]
  fn v8__String__IsOneByte(this: *const String) -> bool;
  fn v8__String__ContainsOnlyOneByte(this: *const String) -> bool;
  fn v8__String__StringEquals(this: *const String, that: *const String)
  -> bool;
  fn v8__String__InternalizeString(
    this: *const String,
    isolate: *mut RealIsolate,
  ) -> *const String;
  fn v8__ExternalOneByteStringResource__data(
    this: *const ExternalOneByteStringResource,
  ) -> *const char;
  fn v8__ExternalOneByteStringResource__length(
    this: *const ExternalOneByteStringResource,
  ) -> size_t;

  fn v8__String__ValueView__CONSTRUCT(
    buf: *mut ValueView,
    isolate: *mut RealIsolate,
    string: *const String,
  );
  fn v8__String__ValueView__DESTRUCT(this: *mut ValueView);
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
  #[inline]
  pub fn data(&self) -> *const char {
    unsafe { v8__ExternalOneByteStringResource__data(self) }
  }

  /// Returns the length of the data owned by this resource.
  #[inline]
  pub fn length(&self) -> usize {
    unsafe { v8__ExternalOneByteStringResource__length(self) }
  }

  /// Returns the data owned by this resource as a string slice.
  /// The data is guaranteed to be Latin-1.
  #[inline]
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
  _isolate: *mut RealIsolate,
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
  unsafe extern "C" fn(*const OneByteConst, *mut RealIsolate);
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
    const kNullTerminate =
      crate::binding::v8__String__WriteFlags__kNullTerminate as _;
    const kReplaceInvalidUtf8 =
      crate::binding::v8__String__WriteFlags__kReplaceInvalidUtf8 as _;
  }
}

impl String {
  /// The maximum length (in bytes) of a buffer that a v8::String can be built
  /// from. Attempting to create a v8::String from a larger buffer will result
  /// in None being returned.
  pub const MAX_LENGTH: usize = v8__String__kMaxLength as _;

  #[inline(always)]
  pub fn empty<'s>(scope: &PinScope<'s, '_, ()>) -> Local<'s, String> {
    // FIXME(bnoordhuis) v8__String__Empty() is infallible so there
    // is no need to box up the result, only to unwrap it again.
    unsafe { scope.cast_local(|sd| v8__String__Empty(sd.get_isolate_ptr())) }
      .unwrap()
  }

  /// Allocates a new string from UTF-8 data. Only returns an empty value when
  /// length > kMaxLength
  #[inline(always)]
  pub fn new_from_utf8<'s>(
    scope: &PinScope<'s, '_, ()>,
    buffer: &[u8],
    new_type: NewStringType,
  ) -> Option<Local<'s, String>> {
    if buffer.is_empty() {
      return Some(Self::empty(scope));
    }
    // V8's `NewFromUtf8` runs a scalar UTF-8 decoder (twice: once to compute
    // the width/length, once to write), which is very slow for non-ASCII. When
    // simdutf is available we decode the input ourselves and hand V8 a
    // pre-decoded one-byte (Latin-1) or two-byte (UTF-16) buffer — which it can
    // just memcpy.
    // `NewFromUtf8` rejects inputs whose *byte* length exceeds the maximum
    // string length (conservatively, before decoding). Our decode paths would
    // otherwise accept some of those (the decoded string is shorter), which
    // would change behavior, so only take them when the byte length is in
    // range and let V8 reject the rest.
    if buffer.len() <= Self::MAX_LENGTH {
      // Pure ASCII (the common case): the bytes are already valid one-byte
      // (Latin-1) data. `onebyte_is_ascii` uses a wide simdutf scan for long
      // inputs (where it beats std's SWAR `is_ascii`) and the inline scan for
      // short ones — matching what the read paths already do.
      if onebyte_is_ascii(buffer) {
        return Self::new_from_one_byte(scope, buffer, new_type);
      }
      // Non-ASCII: transcode with simdutf only above a small threshold. For
      // tiny strings the two potential simdutf FFI calls (Latin-1 attempt then
      // UTF-16) cost more than V8's decoder, which is only slow at scale.
      if buffer.len() >= NONASCII_ENCODE_SIMD_THRESHOLD {
        return Self::new_from_utf8_transcode(scope, buffer, new_type);
      }
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

  /// Decodes non-ASCII, non-empty valid UTF-8 into one-byte (Latin-1) or
  /// two-byte (UTF-16) data with simdutf and hands it to V8. Falls back to
  /// V8's lossy `NewFromUtf8` when the input isn't valid UTF-8.
  fn new_from_utf8_transcode<'s>(
    scope: &PinScope<'s, '_, ()>,
    buffer: &[u8],
    new_type: NewStringType,
  ) -> Option<Local<'s, String>> {
    {
      // Try Latin-1 first (more compact). The conversion errors if any code
      // point exceeds U+00FF or the input isn't valid UTF-8; a Latin-1 result
      // is never longer than the UTF-8 input.
      let mut latin1: Vec<u8> = Vec::with_capacity(buffer.len());
      // SAFETY: `latin1` has `buffer.len()` bytes of spare capacity, an upper
      // bound on the Latin-1 length; simdutf only writes, never reads it.
      let r = unsafe {
        let out =
          std::slice::from_raw_parts_mut(latin1.as_mut_ptr(), buffer.len());
        crate::simdutf::convert_utf8_to_latin1_with_errors(buffer, out)
      };
      if r.is_ok() {
        // SAFETY: simdutf wrote `r.count` valid Latin-1 bytes.
        unsafe { latin1.set_len(r.count) };
        return Self::new_from_one_byte(scope, &latin1, new_type);
      }
      // Not Latin-1 representable (or invalid UTF-8): try UTF-16. A UTF-16
      // result is never more code units than the UTF-8 input has bytes.
      let mut utf16: Vec<u16> = Vec::with_capacity(buffer.len());
      // SAFETY: `utf16` has `buffer.len()` units of spare capacity, an upper
      // bound on the UTF-16 length.
      let r = unsafe {
        let out =
          std::slice::from_raw_parts_mut(utf16.as_mut_ptr(), buffer.len());
        crate::simdutf::convert_utf8_to_utf16le_with_errors(buffer, out)
      };
      if r.is_ok() {
        // SAFETY: simdutf wrote `r.count` valid UTF-16 code units.
        unsafe { utf16.set_len(r.count) };
        return Self::new_from_two_byte(scope, &utf16, new_type);
      }
      // Invalid UTF-8: fall through to V8's lossy `NewFromUtf8`.
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
    scope: &PinScope<'s, '_, ()>,
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
    scope: &PinScope<'s, '_, ()>,
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
  pub fn utf8_length(&self, scope: &Isolate) -> usize {
    crate::handle::assert_locals_in_isolate(scope);
    unsafe { v8__String__Utf8Length(self, scope.as_real_ptr()) as usize }
  }

  /// Writes the contents of the string to an external buffer, as 16-bit
  /// (UTF-16) character codes.
  #[inline(always)]
  pub fn write_v2(
    &self,
    scope: &Isolate,
    offset: u32,
    buffer: &mut [u16],
    flags: WriteFlags,
  ) {
    crate::handle::assert_locals_in_isolate(scope);
    unsafe {
      v8__String__Write_v2(
        self,
        scope.as_real_ptr(),
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
  pub fn write_one_byte_v2(
    &self,
    scope: &Isolate,
    offset: u32,
    buffer: &mut [u8],
    flags: WriteFlags,
  ) {
    crate::handle::assert_locals_in_isolate(scope);
    unsafe {
      v8__String__WriteOneByte_v2(
        self,
        scope.as_real_ptr(),
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
  pub fn write_one_byte_uninit_v2(
    &self,
    scope: &Isolate,
    offset: u32,
    buffer: &mut [MaybeUninit<u8>],
    flags: WriteFlags,
  ) {
    crate::handle::assert_locals_in_isolate(scope);
    unsafe {
      v8__String__WriteOneByte_v2(
        self,
        scope.as_real_ptr(),
        offset,
        self.length().min(buffer.len()) as _,
        buffer.as_mut_ptr() as _,
        flags.bits(),
      )
    }
  }

  /// Writes the contents of the string to an external buffer, as UTF-8.
  #[inline(always)]
  pub fn write_utf8_v2(
    &self,
    scope: &Isolate,
    buffer: &mut [u8],
    flags: WriteFlags,
    processed_characters_return: Option<&mut usize>,
  ) -> usize {
    crate::handle::assert_locals_in_isolate(scope);
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
  pub fn write_utf8_uninit_v2(
    &self,
    scope: &Isolate,
    buffer: &mut [MaybeUninit<u8>],
    flags: WriteFlags,
    processed_characters_return: Option<&mut usize>,
  ) -> usize {
    crate::handle::assert_locals_in_isolate(scope);
    let bytes = unsafe {
      v8__String__WriteUtf8_v2(
        self,
        scope.as_real_ptr(),
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
    scope: &PinScope<'s, '_, ()>,
    value: &str,
  ) -> Option<Local<'s, String>> {
    Self::new_from_utf8(scope, value.as_ref(), NewStringType::Normal)
  }

  /// Creates a new string by concatenating `left` and `right`.
  /// Returns `None` if the resulting string would exceed
  /// `v8::String::kMaxLength`.
  #[inline(always)]
  pub fn concat<'s>(
    scope: &PinScope<'s, '_, ()>,
    left: Local<String>,
    right: Local<String>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__String__Concat(sd.get_isolate_ptr(), &*left, &*right)
      })
    }
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
    scope: &PinScope<'s, '_, ()>,
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
    scope: &PinScope<'s, '_, ()>,
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
    scope: &PinScope<'s, '_, ()>,
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
    scope: &PinScope<'s, '_, ()>,
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
    scope: &PinScope<'s, '_, ()>,
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

  /// Creates a `v8::String` from owned two-byte (UTF-16) data.
  /// V8 will take ownership of the buffer and free it when the string
  /// is garbage collected.
  #[inline(always)]
  pub fn new_external_twobyte<'s>(
    scope: &PinScope<'s, '_, ()>,
    buffer: Box<[u16]>,
  ) -> Option<Local<'s, String>> {
    let buffer_len = buffer.len();
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalTwoByte(
          sd.get_isolate_ptr(),
          Box::into_raw(buffer).cast::<u16>(),
          buffer_len,
          free_rust_external_twobyte,
        )
      })
    }
  }

  /// Creates a `v8::String` from owned two-byte (UTF-16) data, length,
  /// and a custom destructor.
  /// V8 will take ownership of the buffer and call the destructor when
  /// the string is garbage collected.
  ///
  /// # Safety
  ///
  /// `buffer` must be owned (valid for the lifetime of the string), and
  /// `destructor` must be a valid function pointer that can free the
  /// buffer. The destructor will be called with the buffer and length
  /// when the string is garbage collected.
  #[inline(always)]
  pub unsafe fn new_external_twobyte_raw<'s>(
    scope: &PinScope<'s, '_, ()>,
    buffer: *mut u16,
    buffer_len: usize,
    destructor: unsafe extern "C" fn(*mut u16, usize),
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__String__NewExternalTwoByte(
          sd.get_isolate_ptr(),
          buffer,
          buffer_len,
          destructor,
        )
      })
    }
  }

  /// Get the ExternalStringResource for an external string.
  ///
  /// Returns None if is_external() doesn't return true.
  #[inline]
  pub fn get_external_string_resource(
    &self,
  ) -> Option<NonNull<ExternalStringResource>> {
    NonNull::new(unsafe { v8__String__GetExternalStringResource(self) })
  }

  /// Get the ExternalOneByteStringResource for an external one-byte string.
  ///
  /// Returns None if is_external_onebyte() doesn't return true.
  #[inline]
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

  /// Returns true if the string is identical to `other`, comparing by
  /// contents rather than by identity.
  #[inline(always)]
  pub fn string_equals(&self, other: Local<String>) -> bool {
    unsafe { v8__String__StringEquals(self, &*other) }
  }

  /// Converts this string to an internalized string, which is guaranteed to
  /// be unique for its contents within the isolate: two internalized strings
  /// with equal contents are the same object.
  #[inline(always)]
  pub fn internalize_string<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, String> {
    unsafe {
      scope.cast_local(|sd| {
        v8__String__InternalizeString(self, sd.get_isolate_ptr())
      })
    }
    .unwrap()
  }

  /// Creates a copy of a [`crate::String`] in a [`std::string::String`].
  /// Convenience function not present in the original V8 API.
  ///
  /// Uses [`ValueView`] internally for single-pass access to the string
  /// data, with SIMD-accelerated transcoding for Latin-1 and two-byte
  /// strings.
  pub fn to_rust_string_lossy(&self, scope: &Isolate) -> std::string::String {
    crate::handle::assert_locals_in_isolate(scope);
    // No preliminary `self.length()` FFI call: the `ValueView` reports the
    // length, and `data()` yields an empty slice for empty strings, which the
    // ASCII arm below turns into an empty `String`.
    // SAFETY: `self` is a valid V8 string reachable from a handle scope.
    let view = unsafe { ValueView::new_from_ref(scope, self) };

    match view.data() {
      ValueViewData::OneByte(bytes) => onebyte_to_string(bytes),
      ValueViewData::TwoByte(units) => wtf16_to_string(units),
    }
  }

  /// Writes the UTF-8 representation of this string into an existing
  /// [`std::string::String`], reusing its allocation.
  ///
  /// The buffer is cleared first, then filled with the string's UTF-8
  /// contents. This avoids repeated heap allocation when converting
  /// many V8 strings — callers can keep a single `String` and reuse it.
  ///
  /// Uses [`ValueView`] internally for single-pass access, avoiding
  /// the extra `utf8_length` FFI call.
  pub fn write_utf8_into(
    &self,
    scope: &mut Isolate,
    buf: &mut std::string::String,
  ) {
    crate::handle::assert_locals_in_isolate(scope);
    buf.clear();
    // No preliminary `self.length()` FFI call; an empty string yields an empty
    // `data()` slice and leaves `buf` cleared.
    // SAFETY: `self` is a valid V8 string reachable from a handle scope.
    // The ValueView is dropped before we return.
    let view = unsafe { ValueView::new_from_ref(scope, self) };

    match view.data() {
      ValueViewData::OneByte(bytes) => {
        if onebyte_is_ascii(bytes) {
          // ASCII: direct copy, already valid UTF-8.
          buf.reserve(bytes.len());
          unsafe {
            let vec = buf.as_mut_vec();
            std::ptr::copy_nonoverlapping(
              bytes.as_ptr(),
              vec.as_mut_ptr(),
              bytes.len(),
            );
            vec.set_len(bytes.len());
          }
        } else {
          // Latin-1: each byte can expand to at most 2 UTF-8 bytes.
          let max_utf8_len = bytes.len() * 2;
          buf.reserve(max_utf8_len);
          unsafe {
            let vec = buf.as_mut_vec();
            let written =
              latin1_to_utf8(bytes.len(), bytes.as_ptr(), vec.as_mut_ptr());
            vec.set_len(written);
          }
        }
      }
      ValueViewData::TwoByte(units) => {
        wtf16_into_string(units, buf);
      }
    }
  }

  /// Converts a [`crate::String`] to either an owned [`std::string::String`],
  /// or a borrowed [`str`], depending on whether it fits into the provided
  /// buffer.
  ///
  /// Uses [`ValueView`] internally for direct access to the string's
  /// contents, eliminating the `utf8_length` pre-scan that the previous
  /// implementation required.
  pub fn to_rust_cow_lossy<'a, const N: usize>(
    &self,
    scope: &mut Isolate,
    buffer: &'a mut [MaybeUninit<u8>; N],
  ) -> Cow<'a, str> {
    crate::handle::assert_locals_in_isolate(scope);
    // No preliminary `self.length()` FFI call; an empty string yields an empty
    // `data()` slice, which the ASCII arm borrows as an empty `&str`.
    // SAFETY: `self` is a valid V8 string reachable from a handle scope.
    // The ValueView is dropped before we return, so the
    // DisallowGarbageCollection scope it holds is properly scoped.
    let view = unsafe { ValueView::new_from_ref(scope, self) };

    match view.data() {
      ValueViewData::OneByte(bytes) => {
        // Fused single pass: `convert_latin1_to_utf8` transcodes Latin-1 and,
        // for the common pure-ASCII case, is just a copy (`written == len`).
        // This removes the separate `onebyte_is_ascii` pre-scan the previous
        // code ran before the memcpy. Only taken when the worst-case 2x
        // expansion fits the borrow buffer (so the convert can't overflow) and
        // the string is long enough for the simdutf FFI call to pay off.
        if bytes.len() >= ONEBYTE_SIMD_THRESHOLD
          && bytes.len().saturating_mul(2) <= N
        {
          // SAFETY: `buffer` is valid for `N` writes and `N >= bytes.len() * 2`
          // (guarded above), so it fits the full UTF-8 expansion.
          let written = unsafe {
            transcode_latin1_to_utf8(bytes, buffer.as_mut_ptr() as *mut u8, N)
          };
          // SAFETY: simdutf wrote `written` valid UTF-8 bytes into `buffer`.
          return unsafe {
            let buf = &mut buffer[..written];
            let buf = &mut *(buf as *mut [_] as *mut [u8]);
            Cow::Borrowed(std::str::from_utf8_unchecked(buf))
          };
        }
        if onebyte_is_ascii(bytes) {
          // ASCII: direct memcpy, no transcoding needed.
          if bytes.len() <= N {
            unsafe {
              std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                buffer.as_mut_ptr() as *mut u8,
                bytes.len(),
              );
              let buf = &mut buffer[..bytes.len()];
              let buf = &mut *(buf as *mut [_] as *mut [u8]);
              Cow::Borrowed(std::str::from_utf8_unchecked(buf))
            }
          } else {
            // SAFETY: ASCII bytes are valid UTF-8.
            unsafe {
              Cow::Owned(std::string::String::from_utf8_unchecked(
                bytes.to_vec(),
              ))
            }
          }
        } else {
          latin1_to_cow_str(bytes, buffer)
        }
      }
      ValueViewData::TwoByte(units) => wtf16_to_cow_str(units, buffer),
    }
  }
}

#[inline]
pub unsafe extern "C" fn free_rust_external_onebyte(s: *mut char, len: usize) {
  unsafe {
    let slice = std::slice::from_raw_parts_mut(s, len);

    // Drop the slice
    drop(Box::from_raw(slice));
  }
}

#[inline]
pub unsafe extern "C" fn free_rust_external_twobyte(s: *mut u16, len: usize) {
  unsafe {
    let slice = std::slice::from_raw_parts_mut(s, len);
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
    crate::handle::assert_locals_in_isolate(isolate);
    // SAFETY: Local<'s, String> guarantees the V8 string is rooted in a
    // HandleScope that lives for at least 's.  Deref on Local erases the
    // scope lifetime, so we recover it via pointer cast.
    let string_ref: &'s String = unsafe { &*((&*string) as *const String) };
    unsafe { Self::new_from_ref(isolate, string_ref) }
  }

  /// Constructs a `ValueView` from a raw string reference.
  ///
  /// # Safety
  ///
  /// The caller must ensure that `string` is a valid V8 string that
  /// remains alive for at least `'s`. In practice this means the
  /// string must be reachable from a handle scope that outlives the
  /// returned `ValueView`.
  #[inline(always)]
  pub(crate) unsafe fn new_from_ref(
    isolate: &Isolate,
    string: &'s String,
  ) -> Self {
    let mut v = std::mem::MaybeUninit::uninit();
    unsafe {
      v8__String__ValueView__CONSTRUCT(
        v.as_mut_ptr(),
        isolate.as_real_ptr(),
        string,
      );
      v.assume_init()
    }
  }

  #[inline(always)]
  pub fn data(&self) -> ValueViewData<'_> {
    // Read the `v8::String::ValueView` fields directly out of the byte buffer
    // that `CONSTRUCT` filled, instead of crossing FFI for each one-line
    // accessor thunk. The layout is fixed by the public header
    // (v8/include/v8-primitive.h):
    //   offset 0:                    Local<v8::String> flat_str_  (1 pointer)
    //   offset size_of::<*>():       union { data8_; data16_ }    (1 pointer)
    //   + size_of::<*>():            uint32_t length_
    //   + size_of::<u32>():          bool is_one_byte_
    // The offsets are verified at runtime against the FFI accessors in
    // `tests/test_api.rs` (`value_view_field_layout`).
    const PTR: usize = std::mem::size_of::<*const u8>();
    const DATA_OFFSET: usize = PTR;
    const LENGTH_OFFSET: usize = PTR + PTR;
    const IS_ONE_BYTE_OFFSET: usize = PTR + PTR + std::mem::size_of::<u32>();
    unsafe {
      let base = self.0.as_ptr();
      let length =
        base.add(LENGTH_OFFSET).cast::<u32>().read_unaligned() as usize;
      let is_one_byte = *base.add(IS_ONE_BYTE_OFFSET) != 0;
      if length == 0 {
        // Empty strings may carry a null `data8_`/`data16_` pointer, so return
        // an empty slice with a valid (dangling) pointer rather than passing a
        // possibly-null pointer to `from_raw_parts`. Still report the actual
        // encoding so `data()`'s contract holds for empty two-byte strings.
        return if is_one_byte {
          ValueViewData::OneByte(&[])
        } else {
          ValueViewData::TwoByte(&[])
        };
      }
      let data = base.add(DATA_OFFSET).cast::<*const u8>().read_unaligned();
      if is_one_byte {
        ValueViewData::OneByte(std::slice::from_raw_parts(data, length))
      } else {
        ValueViewData::TwoByte(std::slice::from_raw_parts(
          data.cast::<u16>(),
          length,
        ))
      }
    }
  }

  /// Returns a zero-copy `&str` if the string is one-byte and pure ASCII.
  ///
  /// This is the fastest way to access a V8 string's contents as a Rust
  /// `&str` — no allocation, no copy, no transcoding. Returns `None` for
  /// strings that contain non-ASCII Latin-1 bytes or are two-byte encoded.
  ///
  /// The returned reference is valid as long as this `ValueView` is alive.
  #[inline(always)]
  pub fn as_str(&self) -> Option<&str> {
    match self.data() {
      ValueViewData::OneByte(bytes) => {
        if bytes.is_ascii() {
          // SAFETY: ASCII bytes are valid UTF-8.
          Some(unsafe { std::str::from_utf8_unchecked(bytes) })
        } else {
          None
        }
      }
      ValueViewData::TwoByte(_) => None,
    }
  }

  /// Returns the string contents as a `Cow<str>`.
  ///
  /// - **One-byte ASCII**: returns `Cow::Borrowed(&str)` — true zero-copy.
  /// - **One-byte Latin-1** (non-ASCII): transcodes to UTF-8, returns
  ///   `Cow::Owned`.
  /// - **Two-byte** (UTF-16/WTF-16): transcodes to UTF-8, returns
  ///   `Cow::Owned`, using SIMD-accelerated conversion for valid UTF-16
  ///   strings above a threshold size.
  #[inline(always)]
  pub fn to_cow_lossy(&self) -> Cow<'_, str> {
    match self.data() {
      ValueViewData::OneByte(bytes) => {
        if bytes.is_ascii() {
          // SAFETY: ASCII bytes are valid UTF-8.
          Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(bytes) })
        } else {
          Cow::Owned(latin1_to_string(bytes))
        }
      }
      ValueViewData::TwoByte(units) => Cow::Owned(wtf16_to_string(units)),
    }
  }
}

// ---------------------------------------------------------------------------
// String conversion helpers.
// Hot paths dispatch to SIMD-accelerated routines in `crate::simdutf`.
// ---------------------------------------------------------------------------

/// The minimum number of UTF-16 code units before we try the SIMD path.
/// With the single-pass `convert_utf16le_to_utf8_with_errors` conversion the
/// crossover against the scalar `decode_utf16` loop is low; measured wins start
/// around 16 units.
const WTF16_SIMD_THRESHOLD: usize = 16;

/// Minimum one-byte string length before the simdutf `utf8_length_from_latin1`
/// path beats std's inline `is_ascii` SWAR scan (the simdutf FFI call has fixed
/// overhead that only pays off once the scan is long enough).
const ONEBYTE_SIMD_THRESHOLD: usize = 128;

/// Transcodes Latin-1 `bytes` into the caller-provided output region, returning
/// the number of UTF-8 bytes written (the UTF-8 length of `bytes`).
///
/// Callers write into uninitialized memory — a fresh `Vec`'s spare capacity or
/// a `MaybeUninit` borrow buffer — so the destination is passed as a raw
/// pointer + length rather than an already-initialized `&mut [u8]`. Centralizes
/// the one unsafe `simdutf` FFI call shared by the one-byte read paths.
///
/// # Safety
/// `out_ptr` must be valid for writes of `out_len` bytes, and `out_len` must be
/// at least the UTF-8 length of `bytes` (which never exceeds `bytes.len() * 2`).
#[inline(always)]
unsafe fn transcode_latin1_to_utf8(
  bytes: &[u8],
  out_ptr: *mut u8,
  out_len: usize,
) -> usize {
  // SAFETY: the caller guarantees `out_ptr` is valid for `out_len` writes.
  let out = unsafe { std::slice::from_raw_parts_mut(out_ptr, out_len) };
  // SAFETY: `out` covers the full UTF-8 expansion, so simdutf's write stays in
  // bounds; it always produces valid UTF-8 from Latin-1 input.
  unsafe { crate::simdutf::convert_latin1_to_utf8(bytes, out) }
}

/// Whether one-byte string data is pure ASCII. Uses simdutf's wide SIMD scan
/// for long strings (where it beats std's SWAR `is_ascii`) and the inline
/// `is_ascii` for short ones (avoiding the simdutf FFI-call overhead). Shared
/// by the one-byte read paths that only need the ASCII/Latin-1 decision.
#[inline(always)]
fn onebyte_is_ascii(bytes: &[u8]) -> bool {
  if bytes.len() >= ONEBYTE_SIMD_THRESHOLD {
    // simdutf's `validate_ascii` scans the *whole* buffer even when the very
    // first byte is non-ASCII, whereas std's `is_ascii` short-circuits. Do a
    // cheap inline early-reject on the head first so Latin-1 / non-ASCII text
    // (which typically has a high byte early) doesn't pay for a full SIMD scan
    // just to be rejected. Pure ASCII passes the head and then gets simdutf's
    // fast wide scan over the rest.
    let head = bytes.len().min(32);
    if !bytes[..head].is_ascii() {
      return false;
    }
    // The head is already confirmed ASCII; scan only the remainder (ASCII-ness
    // is per-byte, so this is equivalent to validating the whole buffer).
    return crate::simdutf::validate_ascii(&bytes[head..]);
  }
  bytes.is_ascii()
}

/// Converts one-byte (Latin-1) string data to an owned
/// [`std::string::String`].
///
/// With `simdutf`, a single `utf8_length_from_latin1` SIMD pass both detects
/// pure ASCII (result == input length) and yields the exact UTF-8 length for
/// the Latin-1 case, so an ASCII string is one SIMD scan + a memcpy and a
/// Latin-1 string is one SIMD scan + one SIMD transcode (down from the previous
/// `is_ascii` scan + separate length scan + transcode).
#[inline(always)]
fn onebyte_to_string(bytes: &[u8]) -> std::string::String {
  {
    // For long strings, one `utf8_length_from_latin1` SIMD pass both detects
    // ASCII and sizes the Latin-1 transcode. For short strings the simdutf FFI
    // call costs more than std's inline `is_ascii` SWAR loop, so keep the
    // inline path there (crossover measured near ~128 bytes).
    // Large strings: fuse detect+transcode into a single `convert_latin1_to_utf8`
    // pass, over-allocating the 2x worst case up front. Dropping the separate
    // `utf8_length_from_latin1` pre-scan is a net win only once the input is
    // large enough to amortize the extra allocation; below this the exact-length
    // path is cheaper (measured: fusing at ~256 bytes regresses from the 2x
    // alloc, but wins clearly by a few KB).
    const ONEBYTE_FUSE_THRESHOLD: usize = 4096;
    if bytes.len() >= ONEBYTE_FUSE_THRESHOLD {
      // `saturating_mul` mirrors the `to_rust_cow_lossy` guard; the product is
      // the max UTF-8 length of Latin-1 input (2 bytes/code point).
      let cap = bytes.len().saturating_mul(2);
      let mut buf: Vec<u8> = Vec::with_capacity(cap);
      // SAFETY: `buf` reserved `cap` bytes == max UTF-8 length of Latin-1 input;
      // the transcode writes `written` <= `cap` valid UTF-8 bytes.
      unsafe {
        let written = transcode_latin1_to_utf8(bytes, buf.as_mut_ptr(), cap);
        buf.set_len(written);
      }
      // TRADEOFF: the returned `String` keeps `capacity == cap == 2 * len` for
      // its lifetime even though `written` can be as low as `len` (pure ASCII,
      // the common case). We deliberately do NOT `shrink_to_fit` here: the
      // realloc + full memcpy it would cost outweighs the single
      // `utf8_length_from_latin1` pre-scan pass this fused path exists to
      // avoid, erasing the win. So large one-byte strings trade up to 2x
      // retained heap for the throughput gain (measured +18% ASCII at >=4 KB).
      // SAFETY: simdutf produced valid UTF-8.
      return unsafe { std::string::String::from_utf8_unchecked(buf) };
    }
    if bytes.len() >= ONEBYTE_SIMD_THRESHOLD {
      let utf8_len = crate::simdutf::utf8_length_from_latin1(bytes);
      if utf8_len == bytes.len() {
        // Pure ASCII: already valid UTF-8. SAFETY: ASCII is valid UTF-8.
        return unsafe { std::str::from_utf8_unchecked(bytes) }.to_owned();
      }
      let mut buf: Vec<u8> = Vec::with_capacity(utf8_len);
      // SAFETY: `buf` has capacity `utf8_len`, exactly what the transcode writes.
      unsafe {
        let written =
          transcode_latin1_to_utf8(bytes, buf.as_mut_ptr(), utf8_len);
        debug_assert_eq!(written, utf8_len);
        buf.set_len(written);
        return std::string::String::from_utf8_unchecked(buf);
      }
    }
  }
  if bytes.is_ascii() {
    // SAFETY: ASCII is valid UTF-8.
    unsafe { std::str::from_utf8_unchecked(bytes) }.to_owned()
  } else {
    latin1_to_string(bytes)
  }
}

/// Converts Latin-1 bytes to an owned [`std::string::String`].
#[inline(always)]
fn latin1_to_string(bytes: &[u8]) -> std::string::String {
  debug_assert!(!bytes.is_ascii());
  let utf8_len = crate::simdutf::utf8_length_from_latin1(bytes);
  let mut buf: Vec<u8> = Vec::with_capacity(utf8_len);
  // SAFETY: `buf` has capacity `utf8_len`, exactly what the transcode writes.
  unsafe {
    let written = transcode_latin1_to_utf8(bytes, buf.as_mut_ptr(), utf8_len);
    debug_assert_eq!(written, utf8_len);
    buf.set_len(written);
    std::string::String::from_utf8_unchecked(buf)
  }
}

/// Converts (potentially ill-formed) UTF-16LE / WTF-16 code units to an
/// owned [`std::string::String`], replacing unpaired surrogates with U+FFFD.
#[inline(always)]
fn wtf16_to_string(units: &[u16]) -> std::string::String {
  {
    // Single simdutf pass that validates *and* converts. Each UTF-16 code unit
    // yields at most 3 UTF-8 bytes (surrogate pairs are 2 units -> 4 bytes), so
    // `len * 3` is a safe upper bound. On a lone-surrogate error we fall
    // through to the scalar WTF-16 loop below.
    if units.len() >= WTF16_SIMD_THRESHOLD {
      let cap = units.len() * 3;
      let mut buf: Vec<u8> = Vec::with_capacity(cap);
      // SAFETY: `buf` has `cap` bytes of spare capacity.
      let result = unsafe {
        let out = std::slice::from_raw_parts_mut(buf.as_mut_ptr(), cap);
        crate::simdutf::convert_utf16le_to_utf8_with_errors(units, out)
      };
      if result.is_ok() {
        // SAFETY: simdutf wrote `result.count` valid UTF-8 bytes.
        unsafe {
          buf.set_len(result.count);
          return std::string::String::from_utf8_unchecked(buf);
        }
      }
    }
  }
  // Scalar fallback: handles short strings and strings with unpaired
  // surrogates (WTF-16).
  let mut buf = std::string::String::with_capacity(units.len() * 3);
  for result in std::char::decode_utf16(units.iter().copied()) {
    buf.push(result.unwrap_or('\u{FFFD}'));
  }
  buf
}

/// Appends WTF-16 code units as UTF-8 into an existing string buffer.
#[inline(always)]
fn wtf16_into_string(units: &[u16], buf: &mut std::string::String) {
  {
    if units.len() >= WTF16_SIMD_THRESHOLD {
      let cap = units.len() * 3;
      buf.reserve(cap);
      // SAFETY: appended bytes are valid UTF-8 (or we roll back on error).
      let vec = unsafe { buf.as_mut_vec() };
      let start = vec.len();
      let result = unsafe {
        let out =
          std::slice::from_raw_parts_mut(vec.as_mut_ptr().add(start), cap);
        crate::simdutf::convert_utf16le_to_utf8_with_errors(units, out)
      };
      if result.is_ok() {
        // SAFETY: simdutf wrote `result.count` valid UTF-8 bytes at `start`.
        unsafe { vec.set_len(start + result.count) };
        return;
      }
      // Lone surrogate: `vec` len is unchanged (`start`); fall through to
      // the scalar loop, which appends over the untouched spare capacity.
    }
  }
  // Scalar fallback.
  buf.reserve(units.len() * 3);
  for result in std::char::decode_utf16(units.iter().copied()) {
    buf.push(result.unwrap_or('\u{FFFD}'));
  }
}

/// Converts Latin-1 bytes to a `Cow<str>`, borrowing from `buffer` when
/// the transcoded result fits.
#[inline(always)]
fn latin1_to_cow_str<'a, const N: usize>(
  bytes: &[u8],
  buffer: &'a mut [MaybeUninit<u8>; N],
) -> Cow<'a, str> {
  let utf8_len = crate::simdutf::utf8_length_from_latin1(bytes);

  if utf8_len <= N {
    // SAFETY: `buffer` is valid for `N >= utf8_len` writes (guarded above).
    let written = unsafe {
      transcode_latin1_to_utf8(bytes, buffer.as_mut_ptr() as *mut u8, utf8_len)
    };

    unsafe {
      let buf = &mut buffer[..written];
      let buf = &mut *(buf as *mut [_] as *mut [u8]);
      Cow::Borrowed(std::str::from_utf8_unchecked(buf))
    }
  } else {
    Cow::Owned(latin1_to_string(bytes))
  }
}

/// Converts WTF-16 code units to a `Cow<str>`, borrowing from `buffer`
/// when the transcoded result fits.
#[inline(always)]
fn wtf16_to_cow_str<'a, const N: usize>(
  units: &[u16],
  buffer: &'a mut [MaybeUninit<u8>; N],
) -> Cow<'a, str> {
  {
    if units.len() >= WTF16_SIMD_THRESHOLD {
      // Each unit is at most 3 UTF-8 bytes, so if `len * 3` fits the stack
      // buffer the single-pass conversion is guaranteed to fit; borrow it.
      if units.len() * 3 <= N {
        let result = unsafe {
          let out =
            std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, N);
          crate::simdutf::convert_utf16le_to_utf8_with_errors(units, out)
        };
        if result.is_ok() {
          return unsafe {
            let buf = &mut buffer[..result.count];
            let buf = &mut *(buf as *mut [_] as *mut [u8]);
            Cow::Borrowed(std::str::from_utf8_unchecked(buf))
          };
        }
        // Lone surrogate: fall through to the scalar path below.
      } else {
        // The worst case may not fit the stack buffer — allocate.
        return Cow::Owned(wtf16_to_string(units));
      }
    }
  }

  // Scalar fallback: try to fit into the buffer, otherwise allocate.
  let mut pos = 0;
  let mut tmp = [0u8; 4];
  let mut all_fit = true;
  for result in std::char::decode_utf16(units.iter().copied()) {
    let c = result.unwrap_or('\u{FFFD}');
    let encoded = c.encode_utf8(&mut tmp);
    if pos + encoded.len() > N {
      all_fit = false;
      break;
    }
    unsafe {
      std::ptr::copy_nonoverlapping(
        encoded.as_ptr(),
        (buffer.as_mut_ptr() as *mut u8).add(pos),
        encoded.len(),
      );
    }
    pos += encoded.len();
  }
  if all_fit {
    unsafe {
      let buf = &mut buffer[..pos];
      let buf = &mut *(buf as *mut [_] as *mut [u8]);
      Cow::Borrowed(std::str::from_utf8_unchecked(buf))
    }
  } else {
    Cow::Owned(std::string::String::from_utf16_lossy(units))
  }
}

impl Drop for ValueView<'_> {
  fn drop(&mut self) {
    unsafe { v8__String__ValueView__DESTRUCT(self) }
  }
}
