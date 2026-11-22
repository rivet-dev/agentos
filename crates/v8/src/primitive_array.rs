// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Local;
use crate::Primitive;
use crate::PrimitiveArray;
use crate::isolate::RealIsolate;
use crate::scope::GetIsolate;
use crate::scope::PinScope;
use crate::support::int;

unsafe extern "C" {
  fn v8__PrimitiveArray__New(
    isolate: *mut RealIsolate,
    length: int,
  ) -> *const PrimitiveArray;

  fn v8__PrimitiveArray__Length(this: *const PrimitiveArray) -> int;

  fn v8__PrimitiveArray__Set(
    this: *const PrimitiveArray,
    isolate: *mut RealIsolate,
    index: int,
    item: *const Primitive,
  );

  fn v8__PrimitiveArray__Get(
    this: *const PrimitiveArray,
    isolate: *mut RealIsolate,
    index: int,
  ) -> *const Primitive;
}

impl PrimitiveArray {
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_>,
    length: usize,
  ) -> Local<'s, PrimitiveArray> {
    unsafe {
      scope.cast_local(|sd| {
        v8__PrimitiveArray__New(sd.get_isolate_ptr(), length as int)
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn length(&self) -> usize {
    unsafe { v8__PrimitiveArray__Length(self) as usize }
  }

  #[inline(always)]
  pub fn set(
    &self,
    scope: &PinScope<'_, '_>,
    index: usize,
    item: Local<'_, Primitive>,
  ) {
    unsafe {
      v8__PrimitiveArray__Set(
        self,
        scope.get_isolate_ptr(),
        index as int,
        &*item,
      );
    }
  }

  #[inline(always)]
  pub fn get<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    index: usize,
  ) -> Local<'s, Primitive> {
    unsafe {
      scope.cast_local(|sd| {
        v8__PrimitiveArray__Get(self, sd.get_isolate_ptr(), index as int)
      })
    }
    .unwrap()
  }
}
