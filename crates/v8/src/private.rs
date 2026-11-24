use crate::Local;
use crate::Private;
use crate::String;
use crate::Value;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;

unsafe extern "C" {
  fn v8__Private__New(
    isolate: *mut RealIsolate,
    name: *const String,
  ) -> *const Private;
  fn v8__Private__ForApi(
    isolate: *mut RealIsolate,
    name: *const String,
  ) -> *const Private;
  fn v8__Private__Name(this: *const Private) -> *const Value;
}

impl Private {
  /// Create a private symbol. If name is not empty, it will be the description.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    name: Option<Local<String>>,
  ) -> Local<'s, Private> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Private__New(
          sd.get_isolate_ptr(),
          name.map_or_else(std::ptr::null, |v| &*v),
        )
      })
    }
    .unwrap()
  }

  /// Retrieve a global private symbol. If a symbol with this name has not
  /// been retrieved in the same isolate before, it is created.
  /// Note that private symbols created this way are never collected, so
  /// they should only be used for statically fixed properties.
  /// Also, there is only one global name space for the names used as keys.
  /// To minimize the potential for clashes, use qualified names as keys,
  /// e.g., "Class#property".
  #[inline(always)]
  pub fn for_api<'s>(
    scope: &PinScope<'s, '_, ()>,
    name: Option<Local<String>>,
  ) -> Local<'s, Private> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Private__ForApi(
          sd.get_isolate_ptr(),
          name.map_or_else(std::ptr::null, |v| &*v),
        )
      })
    }
    .unwrap()
  }

  /// Returns the print name string of the private symbol, or undefined if none.
  #[inline(always)]
  pub fn name<'s>(&self, scope: &PinScope<'s, '_, ()>) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__Private__Name(self)) }.unwrap()
  }
}
