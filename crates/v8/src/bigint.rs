use crate::BigInt;
use crate::Context;
use crate::Local;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;
use crate::support::int;

use std::mem::MaybeUninit;

unsafe extern "C" {
  fn v8__BigInt__New(isolate: *mut RealIsolate, value: i64) -> *const BigInt;
  fn v8__BigInt__NewFromUnsigned(
    isolate: *mut RealIsolate,
    value: u64,
  ) -> *const BigInt;
  fn v8__BigInt__NewFromWords(
    context: *const Context,
    sign_bit: int,
    word_count: int,
    words: *const u64,
  ) -> *const BigInt;
  fn v8__BigInt__Uint64Value(this: *const BigInt, lossless: *mut bool) -> u64;
  fn v8__BigInt__Int64Value(this: *const BigInt, lossless: *mut bool) -> i64;
  fn v8__BigInt__WordCount(this: *const BigInt) -> int;
  fn v8__BigInt__ToWordsArray(
    this: *const BigInt,
    sign_bit: *mut int,
    word_count: *mut int,
    words: *mut u64,
  );
}

impl BigInt {
  #[inline(always)]
  pub fn new_from_i64<'s>(
    scope: &PinScope<'s, '_, ()>,
    value: i64,
  ) -> Local<'s, BigInt> {
    unsafe {
      scope.cast_local(|sd| v8__BigInt__New(sd.get_isolate_ptr(), value))
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn new_from_u64<'s>(
    scope: &PinScope<'s, '_, ()>,
    value: u64,
  ) -> Local<'s, BigInt> {
    unsafe {
      scope.cast_local(|sd| {
        v8__BigInt__NewFromUnsigned(sd.get_isolate_ptr(), value)
      })
    }
    .unwrap()
  }

  /// Creates a new BigInt object using a specified sign bit and a
  /// specified list of digits/words.
  /// The resulting number is calculated as:
  ///
  /// (-1)^sign_bit * (words[0] * (2^64)^0 + words[1] * (2^64)^1 + ...)
  #[inline(always)]
  pub fn new_from_words<'s>(
    scope: &PinScope<'s, '_, ()>,
    sign_bit: bool,
    words: &[u64],
  ) -> Option<Local<'s, BigInt>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__BigInt__NewFromWords(
          sd.get_current_context(),
          sign_bit as int,
          words.len() as int,
          words.as_ptr(),
        )
      })
    }
  }

  /// Returns the value of this BigInt as an unsigned 64-bit integer, and a
  /// `bool` indicating that the conversion was lossless when `true`.
  /// The boolean value will be `false` if the return value was truncated or wrapped around,
  /// in particular if the BigInt is negative.
  #[inline(always)]
  pub fn u64_value(&self) -> (u64, bool) {
    let mut lossless = MaybeUninit::uninit();
    let v = unsafe { v8__BigInt__Uint64Value(self, lossless.as_mut_ptr()) };
    let lossless = unsafe { lossless.assume_init() };
    (v, lossless)
  }

  /// Returns the value of this BigInt as a signed 64-bit integer, and a `bool`
  /// indicating that the conversion was lossless when `true`.
  /// The boolean value will be `false` if the return value was truncated or wrapped around.
  #[inline(always)]
  pub fn i64_value(&self) -> (i64, bool) {
    let mut lossless = MaybeUninit::uninit();
    let v = unsafe { v8__BigInt__Int64Value(self, lossless.as_mut_ptr()) };
    let lossless = unsafe { lossless.assume_init() };
    (v, lossless)
  }

  /// Returns the number of 64-bit words needed to store the result of
  /// `to_words_array`.
  #[inline(always)]
  pub fn word_count(&self) -> usize {
    unsafe { v8__BigInt__WordCount(self) as usize }
  }

  /// Converts this BigInt to a (sign_bit, words) pair. `sign_bit` will be true
  /// if this BigInt is negative. If `words` has too few elements, the result will
  /// be truncated to fit.
  #[inline]
  pub fn to_words_array<'a>(
    &self,
    words: &'a mut [u64],
  ) -> (bool, &'a mut [u64]) {
    let mut sign_bit = MaybeUninit::uninit();
    let mut word_count = words.len() as int;
    unsafe {
      v8__BigInt__ToWordsArray(
        self,
        sign_bit.as_mut_ptr(),
        &mut word_count,
        words.as_mut_ptr(),
      );
    }

    let sign_bit = unsafe { sign_bit.assume_init() };
    debug_assert!(sign_bit == 0 || sign_bit == 1);
    let word_count = word_count as usize;

    (
      sign_bit == 1,
      if word_count < words.len() {
        &mut words[..word_count]
      } else {
        words
      },
    )
  }
}
