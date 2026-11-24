use crate::Context;
use crate::Local;
use crate::Object;
use crate::Proxy;
use crate::Value;
use crate::scope::PinScope;

unsafe extern "C" {
  fn v8__Proxy__New(
    context: *const Context,
    target: *const Object,
    handler: *const Object,
  ) -> *const Proxy;
  fn v8__Proxy__GetHandler(this: *const Proxy) -> *const Value;
  fn v8__Proxy__GetTarget(this: *const Proxy) -> *const Value;
  fn v8__Proxy__IsRevoked(this: *const Proxy) -> bool;
  fn v8__Proxy__Revoke(this: *const Proxy);
}

impl Proxy {
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_>,
    target: Local<Object>,
    handler: Local<Object>,
  ) -> Option<Local<'s, Proxy>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Proxy__New(sd.get_current_context(), &*target, &*handler)
      })
    }
  }

  #[inline(always)]
  pub fn get_handler<'s>(&self, scope: &PinScope<'s, '_>) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__Proxy__GetHandler(self)) }.unwrap()
  }

  #[inline(always)]
  pub fn get_target<'s>(&self, scope: &PinScope<'s, '_>) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__Proxy__GetTarget(self)) }.unwrap()
  }

  #[inline(always)]
  pub fn is_revoked(&self) -> bool {
    unsafe { v8__Proxy__IsRevoked(self) }
  }

  #[inline(always)]
  pub fn revoke(&self) {
    unsafe { v8__Proxy__Revoke(self) };
  }
}
