use crate::Boolean;
use crate::Local;
use crate::Primitive;
use crate::isolate::Isolate;
use crate::isolate::RealIsolate;

unsafe extern "C" {
  fn v8__Null(isolate: *mut RealIsolate) -> *const Primitive;
  fn v8__Undefined(isolate: *mut RealIsolate) -> *const Primitive;

  fn v8__Boolean__New(isolate: *mut RealIsolate, value: bool)
  -> *const Boolean;
  fn v8__Boolean__Value(this: *const Boolean) -> bool;
}

#[inline(always)]
pub fn null<'s, R>(scope: &R) -> Local<'s, Primitive>
where
  R: AsRef<Isolate>,
{
  unsafe { Local::from_raw_unchecked(v8__Null(scope.as_ref().as_real_ptr())) }
}

#[inline(always)]
pub fn undefined<'s, R>(scope: &R) -> Local<'s, Primitive>
where
  R: AsRef<Isolate>,
{
  unsafe {
    Local::from_raw_unchecked(v8__Undefined(scope.as_ref().as_real_ptr()))
  }
}

impl Boolean {
  #[inline(always)]
  pub fn new<'s, R>(scope: &R, value: bool) -> Local<'s, Boolean>
  where
    R: AsRef<Isolate>,
  {
    unsafe {
      Local::from_raw_unchecked(v8__Boolean__New(
        scope.as_ref().as_real_ptr(),
        value,
      ))
    }
  }

  /// Returns the value of this boolean.
  #[inline(always)]
  pub fn value(&self) -> bool {
    unsafe { v8__Boolean__Value(self) }
  }
}
