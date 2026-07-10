use crate::Context;
use crate::HandleScope;
use crate::Local;
use crate::Object;
use crate::RegExp;
use crate::String;
use crate::support::int;

bitflags! {
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  #[repr(transparent)]
  pub struct RegExpCreationFlags: int {
    const GLOBAL = 1 << 0;
    const IGNORE_CASE = 1 << 1;
    const MULTILINE = 1 << 2;
    const STICKY = 1 << 3;
    const UNICODE = 1 << 4;
    const DOT_ALL = 1 << 5;
    const LINEAR = 1 << 6;
    const HAS_INDICES = 1 << 7;
    const UNICODE_SETS = 1 << 8;
  }
}

unsafe extern "C" {
  fn v8__RegExp__New(
    context: *const Context,
    pattern: *const String,
    flags: RegExpCreationFlags,
  ) -> *const RegExp;
  fn v8__RegExp__Exec(
    this: *const RegExp,
    context: *const Context,
    subject: *const String,
  ) -> *const Object;
  fn v8__RegExp__GetSource(this: *const RegExp) -> *const String;
}

impl RegExp {
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s>,
    pattern: Local<String>,
    flags: RegExpCreationFlags,
  ) -> Option<Local<'s, RegExp>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__RegExp__New(sd.get_current_context(), &*pattern, flags)
      })
    }
  }

  #[inline(always)]
  pub fn exec<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    subject: Local<String>,
  ) -> Option<Local<'s, Object>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__RegExp__Exec(self, sd.get_current_context(), &*subject)
      })
    }
  }

  #[inline(always)]
  pub fn get_source<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__RegExp__GetSource(self)) }.unwrap()
  }
}
