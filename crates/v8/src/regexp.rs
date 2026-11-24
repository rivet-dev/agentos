use crate::Context;
use crate::Local;
use crate::Object;
use crate::RegExp;
use crate::String;
use crate::scope::PinScope;
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
  fn v8__RegExp__NewWithBacktrackLimit(
    context: *const Context,
    pattern: *const String,
    flags: RegExpCreationFlags,
    backtrack_limit: u32,
  ) -> *const RegExp;
  fn v8__RegExp__MAX_BACKTRACK_LIMIT() -> u32;
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
    scope: &PinScope<'s, '_>,
    pattern: Local<String>,
    flags: RegExpCreationFlags,
  ) -> Option<Local<'s, RegExp>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__RegExp__New(sd.get_current_context(), &*pattern, flags)
      })
    }
  }

  /// The largest backtrack limit [`new_with_backtrack_limit`] accepts.
  ///
  /// V8 requires the limit to be a valid Smi, so this is `2^30 - 1` on
  /// pointer-compressed builds and `2^31 - 1` otherwise.
  ///
  /// [`new_with_backtrack_limit`]: RegExp::new_with_backtrack_limit
  #[inline(always)]
  pub fn max_backtrack_limit() -> u32 {
    unsafe { v8__RegExp__MAX_BACKTRACK_LIMIT() }
  }

  /// Like [`new`], but additionally specifies a backtrack limit. If the number
  /// of backtracks done in one [`exec`] call hits the limit, a match failure is
  /// immediately returned.
  ///
  /// # Panics
  ///
  /// Panics if `backtrack_limit` is zero, which V8 reserves to mean "no
  /// limit", or if it exceeds [`max_backtrack_limit()`]. V8 itself responds to
  /// both by aborting the process even in release builds, so these are checked
  /// here to fail with a usable message instead.
  ///
  /// [`new`]: RegExp::new
  /// [`exec`]: RegExp::exec
  /// [`max_backtrack_limit()`]: RegExp::max_backtrack_limit
  #[inline(always)]
  pub fn new_with_backtrack_limit<'s>(
    scope: &PinScope<'s, '_>,
    pattern: Local<String>,
    flags: RegExpCreationFlags,
    backtrack_limit: u32,
  ) -> Option<Local<'s, RegExp>> {
    assert!(
      backtrack_limit != 0,
      "backtrack_limit must not be zero; zero is V8's \"no limit\" sentinel, \
       use RegExp::new() instead"
    );
    let max = Self::max_backtrack_limit();
    assert!(
      backtrack_limit <= max,
      "backtrack_limit {backtrack_limit} exceeds the maximum of {max}"
    );
    unsafe {
      scope.cast_local(|sd| {
        v8__RegExp__NewWithBacktrackLimit(
          sd.get_current_context(),
          &*pattern,
          flags,
          backtrack_limit,
        )
      })
    }
  }

  #[inline(always)]
  pub fn exec<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    subject: Local<String>,
  ) -> Option<Local<'s, Object>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__RegExp__Exec(self, sd.get_current_context(), &*subject)
      })
    }
  }

  #[inline(always)]
  pub fn get_source<'s>(&self, scope: &PinScope<'s, '_>) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__RegExp__GetSource(self)) }.unwrap()
  }
}
