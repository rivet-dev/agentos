use std::mem::MaybeUninit;
use std::mem::size_of;

use crate::Local;
use crate::Value;

unsafe extern "C" {
  fn v8__PropertyDescriptor__CONSTRUCT(out: *mut PropertyDescriptor);
  fn v8__PropertyDescriptor__CONSTRUCT__Value(
    this: *const PropertyDescriptor,
    value: *const Value,
  );
  fn v8__PropertyDescriptor__CONSTRUCT__Value_Writable(
    this: *const PropertyDescriptor,
    value: *const Value,
    writable: bool,
  );
  fn v8__PropertyDescriptor__CONSTRUCT__Get_Set(
    this: *const PropertyDescriptor,
    get: *const Value,
    set: *const Value,
  );
  fn v8__PropertyDescriptor__DESTRUCT(this: *mut PropertyDescriptor);
  fn v8__PropertyDescriptor__configurable(
    this: *const PropertyDescriptor,
  ) -> bool;
  fn v8__PropertyDescriptor__enumerable(
    this: *const PropertyDescriptor,
  ) -> bool;
  fn v8__PropertyDescriptor__writable(this: *const PropertyDescriptor) -> bool;
  fn v8__PropertyDescriptor__value(
    this: *const PropertyDescriptor,
  ) -> *const Value;
  fn v8__PropertyDescriptor__get(
    this: *const PropertyDescriptor,
  ) -> *const Value;
  fn v8__PropertyDescriptor__set(
    this: *const PropertyDescriptor,
  ) -> *const Value;
  fn v8__PropertyDescriptor__has_configurable(
    this: *const PropertyDescriptor,
  ) -> bool;
  fn v8__PropertyDescriptor__has_enumerable(
    this: *const PropertyDescriptor,
  ) -> bool;
  fn v8__PropertyDescriptor__has_writable(
    this: *const PropertyDescriptor,
  ) -> bool;
  fn v8__PropertyDescriptor__has_value(this: *const PropertyDescriptor)
  -> bool;
  fn v8__PropertyDescriptor__has_get(this: *const PropertyDescriptor) -> bool;
  fn v8__PropertyDescriptor__has_set(this: *const PropertyDescriptor) -> bool;
  fn v8__PropertyDescriptor__set_enumerable(
    this: *mut PropertyDescriptor,
    enumerable: bool,
  );
  fn v8__PropertyDescriptor__set_configurable(
    this: *mut PropertyDescriptor,
    configurable: bool,
  );
}

#[repr(transparent)]
pub struct PropertyDescriptor([usize; 1]);

const _: () = {
  assert!(
    size_of::<PropertyDescriptor>() == size_of::<usize>(),
    "PropertyDescriptor size is not 1 usize"
  );
};

impl Default for PropertyDescriptor {
  fn default() -> Self {
    Self::new()
  }
}

impl PropertyDescriptor {
  pub fn new() -> Self {
    let mut this = MaybeUninit::<Self>::uninit();
    unsafe {
      v8__PropertyDescriptor__CONSTRUCT(this.as_mut_ptr());
      this.assume_init()
    }
  }

  pub fn new_from_value(value: Local<Value>) -> Self {
    let mut this = MaybeUninit::<Self>::uninit();
    unsafe {
      v8__PropertyDescriptor__CONSTRUCT__Value(this.as_mut_ptr(), &*value);
      this.assume_init()
    }
  }

  pub fn new_from_value_writable(value: Local<Value>, writable: bool) -> Self {
    let mut this = MaybeUninit::<Self>::uninit();
    unsafe {
      v8__PropertyDescriptor__CONSTRUCT__Value_Writable(
        this.as_mut_ptr(),
        &*value,
        writable,
      );
      this.assume_init()
    }
  }

  pub fn new_from_get_set(get: Local<Value>, set: Local<Value>) -> Self {
    let mut this = MaybeUninit::<Self>::uninit();
    unsafe {
      v8__PropertyDescriptor__CONSTRUCT__Get_Set(
        this.as_mut_ptr(),
        &*get,
        &*set,
      );
      this.assume_init()
    }
  }

  pub fn configurable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__configurable(self) }
  }

  pub fn enumerable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__enumerable(self) }
  }

  pub fn writable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__writable(self) }
  }

  pub fn value(&self) -> Local<Value> {
    unsafe { Local::from_raw(v8__PropertyDescriptor__value(self)) }.unwrap()
  }

  pub fn get(&self) -> Local<Value> {
    unsafe { Local::from_raw(v8__PropertyDescriptor__get(self)) }.unwrap()
  }

  pub fn set(&self) -> Local<Value> {
    unsafe { Local::from_raw(v8__PropertyDescriptor__set(self)) }.unwrap()
  }

  pub fn has_configurable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_configurable(self) }
  }

  pub fn has_enumerable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_enumerable(self) }
  }

  pub fn has_writable(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_writable(self) }
  }

  pub fn has_value(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_value(self) }
  }

  pub fn has_get(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_get(self) }
  }

  pub fn has_set(&self) -> bool {
    unsafe { v8__PropertyDescriptor__has_set(self) }
  }

  pub fn set_enumerable(&mut self, enumerable: bool) {
    unsafe { v8__PropertyDescriptor__set_enumerable(self, enumerable) }
  }

  pub fn set_configurable(&mut self, configurable: bool) {
    unsafe { v8__PropertyDescriptor__set_configurable(self, configurable) }
  }
}

impl Drop for PropertyDescriptor {
  fn drop(&mut self) {
    unsafe { v8__PropertyDescriptor__DESTRUCT(self) }
  }
}
