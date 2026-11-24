// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Context;
use crate::Date;
use crate::Local;
use crate::String;
use crate::scope::PinScope;

unsafe extern "C" {
  fn v8__Date__New(context: *const Context, value: f64) -> *const Date;
  fn v8__Date__Parse(
    context: *const Context,
    date_string: *const String,
  ) -> *const Date;
  fn v8__Date__ValueOf(this: *const Date) -> f64;
  fn v8__Date__ToISOString(this: *const Date) -> *const String;
  fn v8__Date__ToUTCString(this: *const Date) -> *const String;
}

/// An instance of the built-in Date constructor (ECMA-262, 15.9).
impl Date {
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_>,
    value: f64,
  ) -> Option<Local<'s, Date>> {
    unsafe {
      scope.cast_local(|sd| v8__Date__New(sd.get_current_context(), value))
    }
  }

  /// Parses the given string as a date, as `Date.parse` does, and returns the
  /// resulting Date object.
  ///
  /// Returns `None` if an exception was thrown; note that a string which is
  /// not a valid date does not throw, but instead yields a Date whose
  /// [`value_of()`] is `NaN`.
  ///
  /// [`value_of()`]: Date::value_of
  #[inline(always)]
  pub fn parse<'s>(
    scope: &PinScope<'s, '_>,
    date_string: Local<String>,
  ) -> Option<Local<'s, Date>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Date__Parse(sd.get_current_context(), &*date_string)
      })
    }
  }

  /// A specialization of Value::NumberValue that is more efficient
  /// because we know the structure of this object.
  #[inline(always)]
  pub fn value_of(&self) -> f64 {
    unsafe { v8__Date__ValueOf(self) }
  }

  /// Generates ISO string representation.
  ///
  /// Unlike JavaScript's `Date.prototype.toISOString`, this does not throw
  /// for a Date whose [`value_of()`] is `NaN`; it returns `"Invalid Date"`.
  ///
  /// [`value_of()`]: Date::value_of
  #[inline(always)]
  pub fn to_iso_string<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__Date__ToISOString(self)) }.unwrap()
  }

  /// Generates UTC string representation.
  ///
  /// Returns `"Invalid Date"` for a Date whose [`value_of()`] is `NaN`.
  ///
  /// [`value_of()`]: Date::value_of
  #[inline(always)]
  pub fn to_utc_string<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__Date__ToUTCString(self)) }.unwrap()
  }
}
