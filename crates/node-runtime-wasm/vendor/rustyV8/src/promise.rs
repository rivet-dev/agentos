use std::marker::PhantomData;

use crate::Context;
use crate::Function;
use crate::HandleScope;
use crate::Local;
use crate::Promise;
use crate::PromiseResolver;
use crate::Value;
use crate::support::MaybeBool;

unsafe extern "C" {
  fn v8__Promise__Resolver__New(
    context: *const Context,
  ) -> *const PromiseResolver;
  fn v8__Promise__Resolver__GetPromise(
    this: *const PromiseResolver,
  ) -> *const Promise;
  fn v8__Promise__Resolver__Resolve(
    this: *const PromiseResolver,
    context: *const Context,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Promise__Resolver__Reject(
    this: *const PromiseResolver,
    context: *const Context,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Promise__State(this: *const Promise) -> PromiseState;
  fn v8__Promise__HasHandler(this: *const Promise) -> bool;
  fn v8__Promise__Result(this: *const Promise) -> *const Value;
  fn v8__Promise__Catch(
    this: *const Promise,
    context: *const Context,
    handler: *const Function,
  ) -> *const Promise;
  fn v8__Promise__Then(
    this: *const Promise,
    context: *const Context,
    handler: *const Function,
  ) -> *const Promise;
  fn v8__Promise__Then2(
    this: *const Promise,
    context: *const Context,
    on_fulfilled: *const Function,
    on_rejected: *const Function,
  ) -> *const Promise;

  fn v8__PromiseRejectMessage__GetPromise(
    this: *const PromiseRejectMessage,
  ) -> *const Promise;
  fn v8__PromiseRejectMessage__GetValue(
    this: *const PromiseRejectMessage,
  ) -> *const Value;
  fn v8__PromiseRejectMessage__GetEvent(
    this: *const PromiseRejectMessage,
  ) -> PromiseRejectEvent;
}

#[derive(Debug, PartialEq, Eq)]
#[repr(C)]
pub enum PromiseState {
  Pending,
  Fulfilled,
  Rejected,
}

impl Promise {
  /// Returns the value of the [[PromiseState]] field.
  #[inline(always)]
  pub fn state(&self) -> PromiseState {
    unsafe { v8__Promise__State(self) }
  }

  /// Returns true if the promise has at least one derived promise, and
  /// therefore resolve/reject handlers (including default handler).
  #[inline(always)]
  pub fn has_handler(&self) -> bool {
    unsafe { v8__Promise__HasHandler(self) }
  }

  /// Returns the content of the [[PromiseResult]] field. The Promise must not
  /// be pending.
  #[inline(always)]
  pub fn result<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__Promise__Result(self)) }.unwrap()
  }

  /// Register a rejection handler with a promise.
  ///
  /// See `Self::then2`.
  #[inline(always)]
  pub fn catch<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    handler: Local<Function>,
  ) -> Option<Local<'s, Promise>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Promise__Catch(self, sd.get_current_context(), &*handler)
      })
    }
  }

  /// Register a resolution handler with a promise.
  ///
  /// See `Self::then2`.
  #[inline(always)]
  pub fn then<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    handler: Local<Function>,
  ) -> Option<Local<'s, Promise>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Promise__Then(self, sd.get_current_context(), &*handler)
      })
    }
  }

  /// Register a resolution/rejection handler with a promise.
  /// The handler is given the respective resolution/rejection value as
  /// an argument. If the promise is already resolved/rejected, the handler is
  /// invoked at the end of turn.
  #[inline(always)]
  pub fn then2<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    on_fulfilled: Local<Function>,
    on_rejected: Local<Function>,
  ) -> Option<Local<'s, Promise>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Promise__Then2(
          self,
          sd.get_current_context(),
          &*on_fulfilled,
          &*on_rejected,
        )
      })
    }
  }
}

impl PromiseResolver {
  /// Create a new resolver, along with an associated promise in pending state.
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, PromiseResolver>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Promise__Resolver__New(sd.get_current_context()))
    }
  }

  /// Extract the associated promise.
  #[inline(always)]
  pub fn get_promise<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Local<'s, Promise> {
    unsafe { scope.cast_local(|_| v8__Promise__Resolver__GetPromise(self)) }
      .unwrap()
  }

  /// Resolve the associated promise with a given value.
  /// Ignored if the promise is no longer pending.
  #[inline(always)]
  pub fn resolve(
    &self,
    scope: &mut HandleScope,
    value: Local<'_, Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Promise__Resolver__Resolve(
        self,
        &*scope.get_current_context(),
        &*value,
      )
      .into()
    }
  }

  /// Reject the associated promise with a given value.
  /// Ignored if the promise is no longer pending.
  #[inline(always)]
  pub fn reject(
    &self,
    scope: &mut HandleScope,
    value: Local<'_, Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Promise__Resolver__Reject(
        self,
        &*scope.get_current_context(),
        &*value,
      )
      .into()
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum PromiseRejectEvent {
  PromiseRejectWithNoHandler,
  PromiseHandlerAddedAfterReject,
  PromiseRejectAfterResolved,
  PromiseResolveAfterResolved,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PromiseRejectMessage<'msg>([usize; 3], PhantomData<&'msg ()>);

impl<'msg> PromiseRejectMessage<'msg> {
  #[inline(always)]
  pub fn get_promise(&self) -> Local<'msg, Promise> {
    unsafe { Local::from_raw(v8__PromiseRejectMessage__GetPromise(self)) }
      .unwrap()
  }

  #[inline(always)]
  pub fn get_event(&self) -> PromiseRejectEvent {
    unsafe { v8__PromiseRejectMessage__GetEvent(self) }
  }

  #[inline(always)]
  pub fn get_value(&self) -> Option<Local<'msg, Value>> {
    unsafe { Local::from_raw(v8__PromiseRejectMessage__GetValue(self)) }
  }
}
