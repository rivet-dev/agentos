// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Data;
use crate::FixedArray;
use crate::Local;
use crate::scope::PinScope;
use crate::support::int;

unsafe extern "C" {
  fn v8__FixedArray__Length(this: *const FixedArray) -> int;

  fn v8__FixedArray__Get(this: *const FixedArray, index: int) -> *const Data;
}

impl FixedArray {
  #[inline(always)]
  pub fn length(&self) -> usize {
    unsafe { v8__FixedArray__Length(self) as usize }
  }

  #[inline(always)]
  pub fn get<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    index: usize,
  ) -> Option<Local<'s, Data>> {
    if index >= self.length() {
      return None;
    }

    unsafe { scope.cast_local(|_| v8__FixedArray__Get(self, index as int)) }
  }
}
