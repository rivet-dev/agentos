//! SIMD-accelerated Unicode validation, transcoding, and base64.
//!
//! These bindings expose the [simdutf](https://github.com/simdutf/simdutf)
//! library that is already bundled with V8, avoiding C++ symbol clashes
//! that occur when linking a separate simdutf crate.

// ---------------------------------------------------------------------------
// FFI declarations
// ---------------------------------------------------------------------------

#[repr(C)]
struct FfiResult {
  error: i32,
  count: usize,
}

unsafe extern "C" {
  // Validation
  fn simdutf__validate_utf8(buf: *const u8, len: usize) -> bool;
  fn simdutf__validate_utf8_with_errors(
    buf: *const u8,
    len: usize,
  ) -> FfiResult;
  fn simdutf__validate_ascii(buf: *const u8, len: usize) -> bool;
  fn simdutf__validate_ascii_with_errors(
    buf: *const u8,
    len: usize,
  ) -> FfiResult;
  fn simdutf__validate_utf16le(buf: *const u16, len: usize) -> bool;
  fn simdutf__validate_utf16le_with_errors(
    buf: *const u16,
    len: usize,
  ) -> FfiResult;
  fn simdutf__validate_utf16be(buf: *const u16, len: usize) -> bool;
  fn simdutf__validate_utf16be_with_errors(
    buf: *const u16,
    len: usize,
  ) -> FfiResult;
  fn simdutf__validate_utf32(buf: *const u32, len: usize) -> bool;
  fn simdutf__validate_utf32_with_errors(
    buf: *const u32,
    len: usize,
  ) -> FfiResult;

  // Conversion: UTF-8 <-> UTF-16LE
  fn simdutf__convert_utf8_to_utf16le(
    input: *const u8,
    length: usize,
    output: *mut u16,
  ) -> usize;
  fn simdutf__convert_utf8_to_utf16le_with_errors(
    input: *const u8,
    length: usize,
    output: *mut u16,
  ) -> FfiResult;
  fn simdutf__convert_valid_utf8_to_utf16le(
    input: *const u8,
    length: usize,
    output: *mut u16,
  ) -> usize;
  fn simdutf__convert_utf16le_to_utf8(
    input: *const u16,
    length: usize,
    output: *mut u8,
  ) -> usize;
  fn simdutf__convert_utf16le_to_utf8_with_errors(
    input: *const u16,
    length: usize,
    output: *mut u8,
  ) -> FfiResult;
  fn simdutf__convert_valid_utf16le_to_utf8(
    input: *const u16,
    length: usize,
    output: *mut u8,
  ) -> usize;

  // Conversion: UTF-8 <-> UTF-16BE
  fn simdutf__convert_utf8_to_utf16be(
    input: *const u8,
    length: usize,
    output: *mut u16,
  ) -> usize;
  fn simdutf__convert_utf16be_to_utf8(
    input: *const u16,
    length: usize,
    output: *mut u8,
  ) -> usize;

  // Conversion: UTF-8 <-> Latin-1
  fn simdutf__convert_utf8_to_latin1(
    input: *const u8,
    length: usize,
    output: *mut u8,
  ) -> usize;
  fn simdutf__convert_utf8_to_latin1_with_errors(
    input: *const u8,
    length: usize,
    output: *mut u8,
  ) -> FfiResult;
  fn simdutf__convert_valid_utf8_to_latin1(
    input: *const u8,
    length: usize,
    output: *mut u8,
  ) -> usize;
  fn simdutf__convert_latin1_to_utf8(
    input: *const u8,
    length: usize,
    output: *mut u8,
  ) -> usize;

  // Conversion: Latin-1 <-> UTF-16LE
  fn simdutf__convert_latin1_to_utf16le(
    input: *const u8,
    length: usize,
    output: *mut u16,
  ) -> usize;
  fn simdutf__convert_utf16le_to_latin1(
    input: *const u16,
    length: usize,
    output: *mut u8,
  ) -> usize;

  // Conversion: UTF-8 <-> UTF-32
  fn simdutf__convert_utf8_to_utf32(
    input: *const u8,
    length: usize,
    output: *mut u32,
  ) -> usize;
  fn simdutf__convert_utf32_to_utf8(
    input: *const u32,
    length: usize,
    output: *mut u8,
  ) -> usize;

  // Length calculation
  fn simdutf__utf8_length_from_utf16le(
    input: *const u16,
    length: usize,
  ) -> usize;
  fn simdutf__utf8_length_from_utf16be(
    input: *const u16,
    length: usize,
  ) -> usize;
  fn simdutf__utf16_length_from_utf8(input: *const u8, length: usize) -> usize;
  fn simdutf__utf8_length_from_latin1(input: *const u8, length: usize)
  -> usize;
  fn simdutf__latin1_length_from_utf8(input: *const u8, length: usize)
  -> usize;
  fn simdutf__utf32_length_from_utf8(input: *const u8, length: usize) -> usize;
  fn simdutf__utf8_length_from_utf32(input: *const u32, length: usize)
  -> usize;
  fn simdutf__utf16_length_from_utf32(
    input: *const u32,
    length: usize,
  ) -> usize;
  fn simdutf__utf32_length_from_utf16le(
    input: *const u16,
    length: usize,
  ) -> usize;

  // Counting
  fn simdutf__count_utf8(input: *const u8, length: usize) -> usize;
  fn simdutf__count_utf16le(input: *const u16, length: usize) -> usize;
  fn simdutf__count_utf16be(input: *const u16, length: usize) -> usize;

  // Encoding detection
  fn simdutf__detect_encodings(input: *const u8, length: usize) -> i32;

  // Base64
  fn simdutf__maximal_binary_length_from_base64(
    input: *const u8,
    length: usize,
  ) -> usize;
  fn simdutf__base64_to_binary(
    input: *const u8,
    length: usize,
    output: *mut u8,
    options: u64,
    last_chunk_options: u64,
  ) -> FfiResult;
  fn simdutf__base64_length_from_binary(length: usize, options: u64) -> usize;
  fn simdutf__binary_to_base64(
    input: *const u8,
    length: usize,
    output: *mut u8,
    options: u64,
  ) -> usize;
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Error codes returned by simdutf operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ErrorCode {
  Success = 0,
  HeaderBits = 1,
  TooShort = 2,
  TooLong = 3,
  Overlong = 4,
  TooLarge = 5,
  Surrogate = 6,
  InvalidBase64Character = 7,
  Base64InputRemainder = 8,
  Base64ExtraBits = 9,
  OutputBufferTooSmall = 10,
  Other = 11,
}

impl ErrorCode {
  fn from_i32(v: i32) -> Self {
    match v {
      0 => Self::Success,
      1 => Self::HeaderBits,
      2 => Self::TooShort,
      3 => Self::TooLong,
      4 => Self::Overlong,
      5 => Self::TooLarge,
      6 => Self::Surrogate,
      7 => Self::InvalidBase64Character,
      8 => Self::Base64InputRemainder,
      9 => Self::Base64ExtraBits,
      10 => Self::OutputBufferTooSmall,
      _ => Self::Other,
    }
  }
}

/// Result of a simdutf operation.
///
/// On success, `count` is the number of code units written/validated.
/// On error, `count` is the position of the error in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimdUtfResult {
  pub error: ErrorCode,
  pub count: usize,
}

impl SimdUtfResult {
  fn from_ffi(r: FfiResult) -> Self {
    Self {
      error: ErrorCode::from_i32(r.error),
      count: r.count,
    }
  }

  /// Returns `true` if the operation succeeded.
  #[inline]
  pub fn is_ok(&self) -> bool {
    self.error == ErrorCode::Success
  }
}

/// Encoding types detected by [`detect_encodings`].
///
/// The returned value is a bitmask — multiple encodings may be possible.
pub mod encoding {
  pub const UTF8: i32 = 1;
  pub const UTF16_LE: i32 = 2;
  pub const UTF16_BE: i32 = 4;
  pub const UTF32_LE: i32 = 8;
  pub const UTF32_BE: i32 = 16;
  pub const LATIN1: i32 = 32;
}

/// Base64 encoding/decoding options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum Base64Options {
  /// Standard base64 with padding.
  Default = 0,
  /// URL-safe base64 without padding.
  Url = 1,
  /// Standard base64 without padding.
  DefaultNoPadding = 2,
  /// URL-safe base64 with padding.
  UrlWithPadding = 3,
}

/// Last chunk handling for base64 decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum LastChunkHandling {
  /// Forgiving: decode partial final chunk.
  Loose = 0,
  /// Error on partial/unpadded last chunk.
  Strict = 1,
  /// Ignore partial last chunk (no error).
  StopBeforePartial = 2,
  /// Only decode full 4-character blocks.
  OnlyFullChunks = 3,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validates that the input is valid UTF-8.
#[inline]
pub fn validate_utf8(input: &[u8]) -> bool {
  unsafe { simdutf__validate_utf8(input.as_ptr(), input.len()) }
}

/// Validates UTF-8 and returns error position on failure.
#[inline]
pub fn validate_utf8_with_errors(input: &[u8]) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__validate_utf8_with_errors(input.as_ptr(), input.len())
  })
}

/// Validates that the input is valid ASCII.
#[inline]
pub fn validate_ascii(input: &[u8]) -> bool {
  unsafe { simdutf__validate_ascii(input.as_ptr(), input.len()) }
}

/// Validates ASCII and returns error position on failure.
#[inline]
pub fn validate_ascii_with_errors(input: &[u8]) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__validate_ascii_with_errors(input.as_ptr(), input.len())
  })
}

/// Validates that the input is valid UTF-16LE.
#[inline]
pub fn validate_utf16le(input: &[u16]) -> bool {
  unsafe { simdutf__validate_utf16le(input.as_ptr(), input.len()) }
}

/// Validates UTF-16LE and returns error position on failure.
#[inline]
pub fn validate_utf16le_with_errors(input: &[u16]) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__validate_utf16le_with_errors(input.as_ptr(), input.len())
  })
}

/// Validates that the input is valid UTF-16BE.
#[inline]
pub fn validate_utf16be(input: &[u16]) -> bool {
  unsafe { simdutf__validate_utf16be(input.as_ptr(), input.len()) }
}

/// Validates UTF-16BE and returns error position on failure.
#[inline]
pub fn validate_utf16be_with_errors(input: &[u16]) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__validate_utf16be_with_errors(input.as_ptr(), input.len())
  })
}

/// Validates that the input is valid UTF-32 (as native-endian `u32`).
#[inline]
pub fn validate_utf32(input: &[u32]) -> bool {
  unsafe { simdutf__validate_utf32(input.as_ptr(), input.len()) }
}

/// Validates UTF-32 and returns error position on failure.
#[inline]
pub fn validate_utf32_with_errors(input: &[u32]) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__validate_utf32_with_errors(input.as_ptr(), input.len())
  })
}

// ---------------------------------------------------------------------------
// Conversion: UTF-8 <-> UTF-16LE
// ---------------------------------------------------------------------------

/// Converts UTF-8 to UTF-16LE. Returns 0 if the input is not valid UTF-8.
///
/// # Safety
///
/// `output` must have at least `input.len()` elements of capacity.
/// Use [`utf16_length_from_utf8`] for an exact count.
#[inline]
pub unsafe fn convert_utf8_to_utf16le(
  input: &[u8],
  output: &mut [u16],
) -> usize {
  unsafe {
    simdutf__convert_utf8_to_utf16le(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-8 to UTF-16LE with error reporting.
///
/// # Safety
///
/// `output` must have at least `input.len()` elements of capacity.
#[inline]
pub unsafe fn convert_utf8_to_utf16le_with_errors(
  input: &[u8],
  output: &mut [u16],
) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__convert_utf8_to_utf16le_with_errors(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  })
}

/// Converts valid UTF-8 to UTF-16LE. Input MUST be valid UTF-8.
///
/// # Safety
///
/// - The input must be valid UTF-8.
/// - `output` must have at least `input.len()` elements of capacity.
#[inline]
pub unsafe fn convert_valid_utf8_to_utf16le(
  input: &[u8],
  output: &mut [u16],
) -> usize {
  unsafe {
    simdutf__convert_valid_utf8_to_utf16le(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-16LE to UTF-8. Returns 0 if the input is not valid UTF-16LE.
///
/// # Safety
///
/// `output` must have at least `input.len() * 3` bytes of capacity.
/// Use [`utf8_length_from_utf16le`] for an exact count.
#[inline]
pub unsafe fn convert_utf16le_to_utf8(
  input: &[u16],
  output: &mut [u8],
) -> usize {
  unsafe {
    simdutf__convert_utf16le_to_utf8(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-16LE to UTF-8 with error reporting.
///
/// # Safety
///
/// `output` must have at least `input.len() * 3` bytes of capacity.
#[inline]
pub unsafe fn convert_utf16le_to_utf8_with_errors(
  input: &[u16],
  output: &mut [u8],
) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__convert_utf16le_to_utf8_with_errors(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  })
}

/// Converts valid UTF-16LE to UTF-8. Input MUST be valid UTF-16LE.
///
/// # Safety
///
/// - The input must be valid UTF-16LE.
/// - `output` must have at least `input.len() * 3` bytes of capacity.
#[inline]
pub unsafe fn convert_valid_utf16le_to_utf8(
  input: &[u16],
  output: &mut [u8],
) -> usize {
  unsafe {
    simdutf__convert_valid_utf16le_to_utf8(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

// ---------------------------------------------------------------------------
// Conversion: UTF-8 <-> UTF-16BE
// ---------------------------------------------------------------------------

/// Converts UTF-8 to UTF-16BE. Returns 0 on invalid input.
///
/// # Safety
///
/// `output` must have at least `input.len()` elements of capacity.
#[inline]
pub unsafe fn convert_utf8_to_utf16be(
  input: &[u8],
  output: &mut [u16],
) -> usize {
  unsafe {
    simdutf__convert_utf8_to_utf16be(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-16BE to UTF-8. Returns 0 on invalid input.
///
/// # Safety
///
/// `output` must have at least `input.len() * 3` bytes of capacity.
#[inline]
pub unsafe fn convert_utf16be_to_utf8(
  input: &[u16],
  output: &mut [u8],
) -> usize {
  unsafe {
    simdutf__convert_utf16be_to_utf8(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

// ---------------------------------------------------------------------------
// Conversion: UTF-8 <-> Latin-1
// ---------------------------------------------------------------------------

/// Converts UTF-8 to Latin-1. Returns 0 on invalid input or if
/// characters are outside the Latin-1 range.
///
/// # Safety
///
/// `output` must have at least `input.len()` bytes of capacity.
/// Use [`latin1_length_from_utf8`] for an exact count.
#[inline]
pub unsafe fn convert_utf8_to_latin1(input: &[u8], output: &mut [u8]) -> usize {
  unsafe {
    simdutf__convert_utf8_to_latin1(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-8 to Latin-1 with error reporting.
///
/// # Safety
///
/// `output` must have at least `input.len()` bytes of capacity.
#[inline]
pub unsafe fn convert_utf8_to_latin1_with_errors(
  input: &[u8],
  output: &mut [u8],
) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__convert_utf8_to_latin1_with_errors(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  })
}

/// Converts valid UTF-8 to Latin-1. Input MUST be valid UTF-8 containing
/// only codepoints in the Latin-1 range (U+0000..U+00FF).
///
/// # Safety
///
/// - The input must be valid UTF-8 with only Latin-1 codepoints.
/// - `output` must have at least `input.len()` bytes of capacity.
#[inline]
pub unsafe fn convert_valid_utf8_to_latin1(
  input: &[u8],
  output: &mut [u8],
) -> usize {
  unsafe {
    simdutf__convert_valid_utf8_to_latin1(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts Latin-1 to UTF-8.
///
/// # Safety
///
/// `output` must have at least `input.len() * 2` bytes of capacity.
/// Use [`utf8_length_from_latin1`] for an exact count.
#[inline]
pub unsafe fn convert_latin1_to_utf8(input: &[u8], output: &mut [u8]) -> usize {
  unsafe {
    simdutf__convert_latin1_to_utf8(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

// ---------------------------------------------------------------------------
// Conversion: Latin-1 <-> UTF-16LE
// ---------------------------------------------------------------------------

/// Converts Latin-1 to UTF-16LE.
///
/// # Safety
///
/// `output` must have at least `input.len()` elements of capacity.
#[inline]
pub unsafe fn convert_latin1_to_utf16le(
  input: &[u8],
  output: &mut [u16],
) -> usize {
  unsafe {
    simdutf__convert_latin1_to_utf16le(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-16LE to Latin-1. The input must only contain codepoints
/// in the Latin-1 range.
///
/// # Safety
///
/// `output` must have at least `input.len()` bytes of capacity.
#[inline]
pub unsafe fn convert_utf16le_to_latin1(
  input: &[u16],
  output: &mut [u8],
) -> usize {
  unsafe {
    simdutf__convert_utf16le_to_latin1(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

// ---------------------------------------------------------------------------
// Conversion: UTF-8 <-> UTF-32
// ---------------------------------------------------------------------------

/// Converts UTF-8 to UTF-32. Returns 0 on invalid input.
///
/// # Safety
///
/// `output` must have at least `input.len()` elements of capacity.
#[inline]
pub unsafe fn convert_utf8_to_utf32(input: &[u8], output: &mut [u32]) -> usize {
  unsafe {
    simdutf__convert_utf8_to_utf32(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

/// Converts UTF-32 to UTF-8. Returns 0 on invalid input.
///
/// # Safety
///
/// `output` must have at least `input.len() * 4` bytes of capacity.
#[inline]
pub unsafe fn convert_utf32_to_utf8(input: &[u32], output: &mut [u8]) -> usize {
  unsafe {
    simdutf__convert_utf32_to_utf8(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
    )
  }
}

// ---------------------------------------------------------------------------
// Length calculation
// ---------------------------------------------------------------------------

/// Returns the number of UTF-8 bytes needed to encode the given UTF-16LE
/// input.
#[inline]
pub fn utf8_length_from_utf16le(input: &[u16]) -> usize {
  unsafe { simdutf__utf8_length_from_utf16le(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-8 bytes needed to encode the given UTF-16BE
/// input.
#[inline]
pub fn utf8_length_from_utf16be(input: &[u16]) -> usize {
  unsafe { simdutf__utf8_length_from_utf16be(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-16 code units needed to encode the given
/// UTF-8 input.
#[inline]
pub fn utf16_length_from_utf8(input: &[u8]) -> usize {
  unsafe { simdutf__utf16_length_from_utf8(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-8 bytes needed to encode the given Latin-1 data.
#[inline]
pub fn utf8_length_from_latin1(input: &[u8]) -> usize {
  unsafe { simdutf__utf8_length_from_latin1(input.as_ptr(), input.len()) }
}

/// Returns the number of Latin-1 bytes that the given UTF-8 input would
/// produce. The input must contain only codepoints in the Latin-1 range.
#[inline]
pub fn latin1_length_from_utf8(input: &[u8]) -> usize {
  unsafe { simdutf__latin1_length_from_utf8(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-32 code units needed for the given UTF-8
/// input.
#[inline]
pub fn utf32_length_from_utf8(input: &[u8]) -> usize {
  unsafe { simdutf__utf32_length_from_utf8(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-8 bytes needed for the given UTF-32 input.
#[inline]
pub fn utf8_length_from_utf32(input: &[u32]) -> usize {
  unsafe { simdutf__utf8_length_from_utf32(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-16 code units needed for the given UTF-32
/// input.
#[inline]
pub fn utf16_length_from_utf32(input: &[u32]) -> usize {
  unsafe { simdutf__utf16_length_from_utf32(input.as_ptr(), input.len()) }
}

/// Returns the number of UTF-32 code units needed for the given
/// UTF-16LE input.
#[inline]
pub fn utf32_length_from_utf16le(input: &[u16]) -> usize {
  unsafe { simdutf__utf32_length_from_utf16le(input.as_ptr(), input.len()) }
}

// ---------------------------------------------------------------------------
// Counting
// ---------------------------------------------------------------------------

/// Counts the number of Unicode codepoints in the UTF-8 input.
/// The input must be valid UTF-8.
#[inline]
pub fn count_utf8(input: &[u8]) -> usize {
  unsafe { simdutf__count_utf8(input.as_ptr(), input.len()) }
}

/// Counts the number of Unicode codepoints in the UTF-16LE input.
/// The input must be valid UTF-16LE.
#[inline]
pub fn count_utf16le(input: &[u16]) -> usize {
  unsafe { simdutf__count_utf16le(input.as_ptr(), input.len()) }
}

/// Counts the number of Unicode codepoints in the UTF-16BE input.
/// The input must be valid UTF-16BE.
#[inline]
pub fn count_utf16be(input: &[u16]) -> usize {
  unsafe { simdutf__count_utf16be(input.as_ptr(), input.len()) }
}

// ---------------------------------------------------------------------------
// Encoding detection
// ---------------------------------------------------------------------------

/// Detects which encodings the input could be.
///
/// Returns a bitmask of possible encodings. Use the constants in
/// [`encoding`] to test the result:
///
/// ```ignore
/// let mask = v8::simdutf::detect_encodings(data);
/// if mask & v8::simdutf::encoding::UTF8 != 0 {
///     // Could be UTF-8
/// }
/// ```
#[inline]
pub fn detect_encodings(input: &[u8]) -> i32 {
  unsafe { simdutf__detect_encodings(input.as_ptr(), input.len()) }
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

/// Returns the maximum number of binary bytes that could result from
/// decoding the given base64 input.
#[inline]
pub fn maximal_binary_length_from_base64(input: &[u8]) -> usize {
  unsafe {
    simdutf__maximal_binary_length_from_base64(input.as_ptr(), input.len())
  }
}

/// Decodes base64 input to binary.
///
/// # Safety
///
/// `output` must have at least
/// [`maximal_binary_length_from_base64`]`(input)` bytes of capacity.
#[inline]
pub unsafe fn base64_to_binary(
  input: &[u8],
  output: &mut [u8],
  options: Base64Options,
  last_chunk: LastChunkHandling,
) -> SimdUtfResult {
  SimdUtfResult::from_ffi(unsafe {
    simdutf__base64_to_binary(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
      options as u64,
      last_chunk as u64,
    )
  })
}

/// Returns the number of base64 characters needed to encode the given
/// number of binary bytes.
#[inline]
pub fn base64_length_from_binary(
  length: usize,
  options: Base64Options,
) -> usize {
  unsafe { simdutf__base64_length_from_binary(length, options as u64) }
}

/// Encodes binary data to base64.
///
/// # Safety
///
/// `output` must have at least
/// [`base64_length_from_binary`]`(input.len(), options)` bytes of capacity.
#[inline]
pub unsafe fn binary_to_base64(
  input: &[u8],
  output: &mut [u8],
  options: Base64Options,
) -> usize {
  unsafe {
    simdutf__binary_to_base64(
      input.as_ptr(),
      input.len(),
      output.as_mut_ptr(),
      options as u64,
    )
  }
}
