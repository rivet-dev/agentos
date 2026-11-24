// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use std::ffi::c_void;

use crate::External;
use crate::Local;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;

unsafe extern "C" {
  fn v8__External__New(
    isolate: *mut RealIsolate,
    value: *mut c_void,
  ) -> *const External;
  fn v8__External__Value(this: *const External) -> *mut c_void;
}

impl External {
  #[inline(always)]
  #[allow(clippy::not_unsafe_ptr_arg_deref)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    value: *mut c_void,
  ) -> Local<'s, Self> {
    unsafe {
      scope.cast_local(|sd| v8__External__New(sd.get_isolate_ptr(), value))
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn value(&self) -> *mut c_void {
    unsafe { v8__External__Value(self) }
  }
}
