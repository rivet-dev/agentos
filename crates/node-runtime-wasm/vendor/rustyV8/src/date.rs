// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Context;
use crate::Date;
use crate::HandleScope;
use crate::Local;

unsafe extern "C" {
  fn v8__Date__New(context: *const Context, value: f64) -> *const Date;
  fn v8__Date__ValueOf(this: *const Date) -> f64;
}

/// An instance of the built-in Date constructor (ECMA-262, 15.9).
impl Date {
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s>,
    value: f64,
  ) -> Option<Local<'s, Date>> {
    unsafe {
      scope.cast_local(|sd| v8__Date__New(sd.get_current_context(), value))
    }
  }

  /// A specialization of Value::NumberValue that is more efficient
  /// because we know the structure of this object.
  #[inline(always)]
  pub fn value_of(&self) -> f64 {
    unsafe { v8__Date__ValueOf(self) }
  }
}
