use crate::Boolean;
use crate::Local;
use crate::Primitive;
use crate::isolate::Isolate;

unsafe extern "C" {
  fn v8__Null(isolate: *mut Isolate) -> *const Primitive;
  fn v8__Undefined(isolate: *mut Isolate) -> *const Primitive;

  fn v8__Boolean__New(isolate: *mut Isolate, value: bool) -> *const Boolean;
}

#[inline(always)]
pub fn null<'a, R>(scope: &mut R) -> Local<'a, Primitive>
where
  R: AsMut<Isolate>,
{
  unsafe { Local::from_raw_unchecked(v8__Null(scope.as_mut())) }
}

#[inline(always)]
pub fn undefined<'a, R>(scope: &mut R) -> Local<'a, Primitive>
where
  R: AsMut<Isolate>,
{
  unsafe { Local::from_raw_unchecked(v8__Undefined(scope.as_mut())) }
}

impl Boolean {
  #[inline(always)]
  pub fn new<'a, R>(scope: &mut R, value: bool) -> Local<'a, Boolean>
  where
    R: AsMut<Isolate>,
  {
    unsafe {
      Local::from_raw_unchecked(v8__Boolean__New(scope.as_mut(), value))
    }
  }
}
