use std::alloc::Layout;
use std::ptr::NonNull;

use crate::HandleScope;
use crate::Int32;
use crate::Integer;
use crate::Isolate;
use crate::Local;
use crate::Number;
use crate::Uint32;

unsafe extern "C" {
  fn v8__Number__New(isolate: *mut Isolate, value: f64) -> *const Number;
  fn v8__Number__Value(this: *const Number) -> f64;
  fn v8__Integer__New(isolate: *mut Isolate, value: i32) -> *const Integer;
  fn v8__Integer__NewFromUnsigned(
    isolate: *mut Isolate,
    value: u32,
  ) -> *const Integer;
  fn v8__Integer__Value(this: *const Integer) -> i64;
  fn v8__Uint32__Value(this: *const Uint32) -> u32;
  fn v8__Int32__Value(this: *const Int32) -> i32;
}

impl Number {
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s, ()>,
    value: f64,
  ) -> Local<'s, Number> {
    unsafe {
      scope.cast_local(|sd| v8__Number__New(sd.get_isolate_ptr(), value))
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn value(&self) -> f64 {
    unsafe { v8__Number__Value(self) }
  }
}

impl Integer {
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s, ()>,
    value: i32,
  ) -> Local<'s, Integer> {
    unsafe {
      scope.cast_local(|sd| v8__Integer__New(sd.get_isolate_ptr(), value))
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn new_from_unsigned<'s>(
    scope: &mut HandleScope<'s, ()>,
    value: u32,
  ) -> Local<'s, Integer> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Integer__NewFromUnsigned(sd.get_isolate_ptr(), value)
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn value(&self) -> i64 {
    unsafe { v8__Integer__Value(self) }
  }

  /// Internal helper function to produce a handle containing a SMI zero value,
  /// without the need for the caller to provide (or have entered) a
  /// `HandleScope`.
  #[inline(always)]
  pub(crate) fn zero<'s>() -> Local<'s, Integer> {
    // The SMI representation of zero is also zero. In debug builds, double
    // check this, so in the unlikely event that V8 changes its internal
    // representation of SMIs such that this invariant no longer holds, we'd
    // catch it.
    static ZERO_SMI: usize = 0;
    let zero_raw = &ZERO_SMI as *const _ as *mut Self;
    let zero_nn = unsafe { NonNull::new_unchecked(zero_raw) };
    let zero_local = unsafe { Local::from_non_null(zero_nn) };
    debug_assert_eq!(Layout::new::<usize>(), Layout::new::<Local<Self>>());
    debug_assert_eq!(zero_local.value(), 0);
    zero_local
  }
}

impl Uint32 {
  #[inline(always)]
  pub fn value(&self) -> u32 {
    unsafe { v8__Uint32__Value(self) }
  }
}

impl Int32 {
  #[inline(always)]
  pub fn value(&self) -> i32 {
    unsafe { v8__Int32__Value(self) }
  }
}
