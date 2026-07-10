// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::HandleScope;
use crate::Isolate;
use crate::Local;
use crate::Primitive;
use crate::PrimitiveArray;
use crate::support::int;

unsafe extern "C" {
  fn v8__PrimitiveArray__New(
    isolate: *mut Isolate,
    length: int,
  ) -> *const PrimitiveArray;

  fn v8__PrimitiveArray__Length(this: *const PrimitiveArray) -> int;

  fn v8__PrimitiveArray__Set(
    this: *const PrimitiveArray,
    isolate: *mut Isolate,
    index: int,
    item: *const Primitive,
  );

  fn v8__PrimitiveArray__Get(
    this: *const PrimitiveArray,
    isolate: *mut Isolate,
    index: int,
  ) -> *const Primitive;
}

impl PrimitiveArray {
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s>,
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
    scope: &mut HandleScope,
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
    scope: &mut HandleScope<'s>,
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
