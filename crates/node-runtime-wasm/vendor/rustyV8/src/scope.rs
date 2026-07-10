// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

//! This module's public API exports a number of 'scope' types.
//!
//! These types carry information about the state of the V8 Isolate, as well as
//! lifetimes for certain (return) values. More specialized scopes typically
//! deref to more generic scopes, and ultimately they all deref to `Isolate`.
//!
//! The scope types in the public API are all pointer wrappers, and they all
//! point at a heap-allocated struct `data::ScopeData`. `ScopeData` allocations
//! are never shared between scopes; each Handle/Context/CallbackScope gets
//! its own instance.
//!
//! Notes about the available scope types:
//! See also the tests at the end of this file.
//!
//! - `HandleScope<'s, ()>`
//!   - 's = lifetime of local handles created in this scope, and of the scope
//!     itself.
//!   - This type is returned when a HandleScope is constructed from a direct
//!     reference to an isolate (`&mut Isolate` or `&mut OwnedIsolate`).
//!   - A `Context` is _not_ available. Only certain types JavaScript values can
//!     be created: primitive values, templates, and instances of `Context`.
//!   - Derefs to `Isolate`.
//!
//! - `HandleScope<'s>`
//!   - 's = lifetime of local handles created in this scope, and of the scope
//!     itself.
//!   - A `Context` is available; any type of value can be created.
//!   - Derefs to `HandleScope<'s, ()>`
//!
//! - `ContextScope<'s, P>`
//!   - 's = lifetime of the scope itself.
//!   - A `Context` is available; any type of value can be created.
//!   - Derefs to `P`.
//!   - When constructed as the child of a `HandleScope<'a, ()>`, the returned
//!     type is `ContextScope<'s, HandleScope<'p>>`. In other words, the parent
//!     HandleScope gets an upgrade to indicate the availability of a `Context`.
//!   - When a new scope is constructed inside this type of scope, the
//!     `ContextScope` wrapper around `P` is erased first, which means that the
//!     child scope is set up as if it had been created with `P` as its parent.
//!
//! - `EscapableHandleScope<'s, 'e>`
//!   - 's = lifetime of local handles created in this scope, and of the scope
//!     itself.
//!   - 'e = lifetime of the HandleScope that will receive the local handle that
//!     is created by `EscapableHandleScope::escape()`.
//!   - A `Context` is available; any type of value can be created.
//!   - Derefs to `HandleScope<'s>`.
//!
//! - `TryCatch<'s, P>`
//!   - 's = lifetime of the TryCatch scope.
//!   - `P` is either a `HandleScope` or an `EscapableHandleScope`. This type
//!     also determines for how long the values returned by `TryCatch` methods
//!     `exception()`, `message()`, and `stack_trace()` are valid.
//!   - Derefs to `P`.
//!   - Creating a new scope inside the `TryCatch` block makes its methods
//!     inaccessible until the inner scope is dropped. However, the `TryCatch`
//!     object will nonetheless catch all exception thrown during its lifetime.
//!
//! - `CallbackScope<'s, ()>`
//!   - 's = lifetime of local handles created in this scope, and the value
//!     returned from the callback, and of the scope itself.
//!   - A `Context` is _not_ available. Only certain types JavaScript values can
//!     be created: primitive values, templates, and instances of `Context`.
//!   - Derefs to `HandleScope<'s, ()>`.
//!   - This scope type is only to be constructed inside embedder defined
//!     callbacks when these are called by V8.
//!   - When a scope is created inside, type is erased to `HandleScope<'s, ()>`.
//!
//! - `CallbackScope<'s>`
//!   - 's = lifetime of local handles created in this scope, and the value
//!     returned from the callback, and of the scope itself.
//!   - A `Context` is available; any type of value can be created.
//!   - Derefs to `HandleScope<'s>`.
//!   - This scope type is only to be constructed inside embedder defined
//!     callbacks when these are called by V8.
//!   - When a scope is created inside, type is erased to `HandleScope<'s>`.
//!
//! - `DisallowJavascriptExecutionScope<'s, P>`
//!   - 's = lifetime of the `DisallowJavascriptExecutionScope` scope.
//!   - `P` is either a `HandleScope`, `ContextScope`, `EscapableHandleScope`
//!     or a `TryCatch`.
//!   - Derefs to `P`.
//!
//! - `AllowJavascriptExecutionScope<'s, P>`
//!   - 's = lifetime of the `AllowJavascriptExecutionScope` scope.
//!   - `P` is `DisallowJavascriptExecutionScope`.
//!   - Derefs to `HandleScope<'s, ()>`.

use std::alloc::Layout;
use std::alloc::alloc;
use std::any::type_name;
use std::cell::Cell;
use std::convert::TryInto;

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::num::NonZeroUsize;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr;
use std::ptr::NonNull;

use crate::Context;
use crate::Data;
use crate::DataError;
use crate::Function;
use crate::Handle;
use crate::Isolate;
use crate::Local;
use crate::Message;
use crate::Object;
use crate::OwnedIsolate;
use crate::Primitive;
use crate::PromiseRejectMessage;
use crate::Value;
use crate::fast_api::FastApiCallbackOptions;
use crate::function::FunctionCallbackInfo;
use crate::function::PropertyCallbackInfo;

/// Stack-allocated class which sets the execution context for all operations
/// executed within a local scope. After entering a context, all code compiled
/// and run is compiled and run in this context.
#[derive(Debug)]
pub struct ContextScope<'s, P> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut P>,
}

impl<'s, P: param::NewContextScope<'s>> ContextScope<'s, P> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'s mut P, context: Local<Context>) -> P::NewScope {
    let scope_data = param.get_scope_data_mut();
    if scope_data.get_isolate_ptr()
      != unsafe { raw::v8__Context__GetIsolate(&*context) }
    {
      panic!(
        "{} and Context do not belong to the same Isolate",
        type_name::<P>()
      )
    }
    let new_scope_data = scope_data.new_context_scope_data(context);
    new_scope_data.as_scope()
  }
}

/// A stack-allocated class that governs a number of local handles.
/// After a handle scope has been created, all local handles will be
/// allocated within that handle scope until either the handle scope is
/// deleted or another handle scope is created.  If there is already a
/// handle scope and a new one is created, all allocations will take
/// place in the new handle scope until it is deleted.  After that,
/// new handles will again be allocated in the original handle scope.
///
/// After the handle scope of a local handle has been deleted the
/// garbage collector will no longer track the object stored in the
/// handle and may deallocate it.  The behavior of accessing a handle
/// for which the handle scope has been deleted is undefined.
#[derive(Debug)]
pub struct HandleScope<'s, C = Context> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut C>,
}

impl<'s> HandleScope<'s> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new<P: param::NewHandleScope<'s>>(param: &'s mut P) -> P::NewScope {
    param
      .get_scope_data_mut()
      .new_handle_scope_data()
      .as_scope()
  }

  /// Opens a new `HandleScope` and enters a `Context` in one step.
  /// The first argument should be an `Isolate` or `OwnedIsolate`.
  /// The second argument can be any handle that refers to a `Context` object;
  /// usually this will be a `Global<Context>`.
  pub fn with_context<
    P: param::NewHandleScopeWithContext<'s>,
    H: Handle<Data = Context>,
  >(
    param: &'s mut P,
    context: H,
  ) -> Self {
    let context_ref = context.open(param.get_isolate_mut());
    param
      .get_scope_data_mut()
      .new_handle_scope_data_with_context(context_ref)
      .as_scope()
  }

  /// Returns the context of the currently running JavaScript, or the context
  /// on the top of the stack if no JavaScript is running.
  #[inline(always)]
  pub fn get_current_context(&self) -> Local<'s, Context> {
    let context_ptr = data::ScopeData::get(self).get_current_context();
    unsafe { Local::from_raw(context_ptr) }.unwrap()
  }

  /// Returns either the last context entered through V8's C++ API, or the
  /// context of the currently running microtask while processing microtasks.
  /// If a context is entered while executing a microtask, that context is
  /// returned.
  pub fn get_entered_or_microtask_context(&self) -> Local<'s, Context> {
    let data = data::ScopeData::get(self);
    let isolate_ptr = data.get_isolate_ptr();
    let context_ptr =
      unsafe { raw::v8__Isolate__GetEnteredOrMicrotaskContext(isolate_ptr) };
    unsafe { Local::from_raw(context_ptr) }.unwrap()
  }

  /// Returns the host defined options set for currently running script or
  /// module, if available.
  #[inline(always)]
  pub fn get_current_host_defined_options(&self) -> Option<Local<'s, Data>> {
    let data = data::ScopeData::get(self);
    let isolate_ptr = data.get_isolate_ptr();
    unsafe {
      Local::from_raw(raw::v8__Isolate__GetCurrentHostDefinedOptions(
        isolate_ptr,
      ))
    }
  }
}

impl<'s> HandleScope<'s, ()> {
  /// Schedules an exception to be thrown when returning to JavaScript. When
  /// an exception has been scheduled it is illegal to invoke any
  /// JavaScript operation; the caller must return immediately and only
  /// after the exception has been handled does it become legal to invoke
  /// JavaScript operations.
  ///
  /// This function always returns the `undefined` value.
  #[inline(always)]
  pub fn throw_exception(
    &mut self,
    exception: Local<Value>,
  ) -> Local<'s, Value> {
    unsafe {
      self.cast_local(|sd| {
        raw::v8__Isolate__ThrowException(sd.get_isolate_ptr(), &*exception)
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub(crate) unsafe fn cast_local<T>(
    &mut self,
    f: impl FnOnce(&mut data::ScopeData) -> *const T,
  ) -> Option<Local<'s, T>> {
    unsafe { Local::from_raw(f(data::ScopeData::get_mut(self))) }
  }

  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut Isolate {
    data::ScopeData::get(self).get_isolate_ptr()
  }
}

// TODO(mmastrac): When the never type is stabilized, we can replace this trait
// with type bounds (https://github.com/rust-lang/rust/issues/35121):

// for<'l> DataError: From<<Local<'s, Data> as TryInto<Local<'l, T>>>::Error>,
mod get_data_sealed {
  use crate::DataError;
  use std::convert::Infallible;

  pub trait ToDataError {
    fn to_data_error(self) -> DataError;
  }
  impl ToDataError for DataError {
    fn to_data_error(self) -> DataError {
      self
    }
  }
  impl ToDataError for Infallible {
    fn to_data_error(self) -> DataError {
      unreachable!()
    }
  }
}

impl<'s> HandleScope<'s> {
  /// Return data that was previously attached to the isolate snapshot via
  /// SnapshotCreator, and removes the reference to it. If called again with
  /// same `index` argument, this function returns `DataError::NoData`.
  ///
  /// The value that was stored in the snapshot must either match or be
  /// convertible to type parameter `T`, otherwise `DataError::BadType` is
  /// returned.
  pub fn get_isolate_data_from_snapshot_once<T>(
    &mut self,
    index: usize,
  ) -> Result<Local<'s, T>, DataError>
  where
    T: 'static,
    for<'l> <Local<'l, Data> as TryInto<Local<'l, T>>>::Error:
      get_data_sealed::ToDataError,
    for<'l> Local<'l, Data>: TryInto<Local<'l, T>>,
  {
    unsafe {
      let Some(res) = self.cast_local(|sd| {
        raw::v8__Isolate__GetDataFromSnapshotOnce(sd.get_isolate_ptr(), index)
      }) else {
        return Err(DataError::no_data::<T>());
      };
      use get_data_sealed::ToDataError;
      match res.try_into() {
        Ok(x) => Ok(x),
        Err(e) => Err(e.to_data_error()),
      }
    }
  }

  /// Return data that was previously attached to the context snapshot via
  /// SnapshotCreator, and removes the reference to it. If called again with
  /// same `index` argument, this function returns `DataError::NoData`.
  ///
  /// The value that was stored in the snapshot must either match or be
  /// convertible to type parameter `T`, otherwise `DataError::BadType` is
  /// returned.
  pub fn get_context_data_from_snapshot_once<T>(
    &mut self,
    index: usize,
  ) -> Result<Local<'s, T>, DataError>
  where
    T: 'static,
    for<'l> <Local<'l, Data> as TryInto<Local<'l, T>>>::Error:
      get_data_sealed::ToDataError,
    for<'l> Local<'l, Data>: TryInto<Local<'l, T>>,
  {
    unsafe {
      let Some(res) = self.cast_local(|sd| {
        raw::v8__Context__GetDataFromSnapshotOnce(
          sd.get_current_context(),
          index,
        )
      }) else {
        return Err(DataError::no_data::<T>());
      };
      use get_data_sealed::ToDataError;
      match res.try_into() {
        Ok(x) => Ok(x),
        Err(e) => Err(e.to_data_error()),
      }
    }
  }

  #[inline(always)]
  pub fn set_promise_hooks(
    &mut self,
    init_hook: Option<Local<Function>>,
    before_hook: Option<Local<Function>>,
    after_hook: Option<Local<Function>>,
    resolve_hook: Option<Local<Function>>,
  ) {
    unsafe {
      let sd = data::ScopeData::get_mut(self);
      raw::v8__Context__SetPromiseHooks(
        sd.get_current_context(),
        init_hook.map_or_else(std::ptr::null, |v| &*v),
        before_hook.map_or_else(std::ptr::null, |v| &*v),
        after_hook.map_or_else(std::ptr::null, |v| &*v),
        resolve_hook.map_or_else(std::ptr::null, |v| &*v),
      );
    }
  }

  #[inline(always)]
  pub fn set_continuation_preserved_embedder_data(
    &mut self,
    data: Local<Value>,
  ) {
    unsafe {
      let sd = data::ScopeData::get_mut(self);
      raw::v8__Context__SetContinuationPreservedEmbedderData(
        sd.get_isolate_ptr(),
        &*data,
      );
    }
  }

  #[inline(always)]
  pub fn get_continuation_preserved_embedder_data(
    &mut self,
  ) -> Local<'s, Value> {
    unsafe {
      self
        .cast_local(|sd| {
          raw::v8__Context__GetContinuationPreservedEmbedderData(
            sd.get_isolate_ptr(),
          )
        })
        .unwrap()
    }
  }
}

/// A HandleScope which first allocates a handle in the current scope
/// which will be later filled with the escape value.
// TODO(piscisaureus): type parameter `C` is not very useful in practice; being
// a source of complexity and potential confusion, it is desirable to
// eventually remove it. Blocker at the time of writing is that there are some
// tests that enter an `EscapableHandleScope` without creating a `ContextScope`
// at all. These tests need to updated first.
#[derive(Debug)]
pub struct EscapableHandleScope<'s, 'e: 's, C = Context> {
  _data: NonNull<data::ScopeData>,
  _phantom:
    PhantomData<(&'s mut raw::HandleScope, &'e mut raw::EscapeSlot, &'s C)>,
}

impl<'s, 'e: 's> EscapableHandleScope<'s, 'e> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new<P: param::NewEscapableHandleScope<'s, 'e>>(
    param: &'s mut P,
  ) -> P::NewScope {
    param
      .get_scope_data_mut()
      .new_escapable_handle_scope_data()
      .as_scope()
  }
}

impl<'s, 'e: 's, C> EscapableHandleScope<'s, 'e, C> {
  /// Pushes the value into the previous scope and returns a handle to it.
  /// Cannot be called twice.
  pub fn escape<T>(&mut self, value: Local<T>) -> Local<'e, T>
  where
    for<'l> Local<'l, T>: Into<Local<'l, Data>>,
  {
    let escape_slot = data::ScopeData::get_mut(self)
      .get_escape_slot_mut()
      .expect("internal error: EscapableHandleScope has no escape slot")
      .take()
      .expect("EscapableHandleScope::escape() called twice");
    escape_slot.escape(value)
  }
}

/// An external exception handler.
#[derive(Debug)]
pub struct TryCatch<'s, P> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut P>,
}

impl<'s, P: param::NewTryCatch<'s>> TryCatch<'s, P> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'s mut P) -> P::NewScope {
    param.get_scope_data_mut().new_try_catch_data().as_scope()
  }
}

impl<P> TryCatch<'_, P> {
  /// Returns true if an exception has been caught by this try/catch block.
  #[inline(always)]
  pub fn has_caught(&self) -> bool {
    unsafe { raw::v8__TryCatch__HasCaught(self.get_raw()) }
  }

  /// For certain types of exceptions, it makes no sense to continue execution.
  ///
  /// If CanContinue returns false, the correct action is to perform any C++
  /// cleanup needed and then return. If CanContinue returns false and
  /// HasTerminated returns true, it is possible to call
  /// CancelTerminateExecution in order to continue calling into the engine.
  #[inline(always)]
  pub fn can_continue(&self) -> bool {
    unsafe { raw::v8__TryCatch__CanContinue(self.get_raw()) }
  }

  /// Returns true if an exception has been caught due to script execution
  /// being terminated.
  ///
  /// There is no JavaScript representation of an execution termination
  /// exception. Such exceptions are thrown when the TerminateExecution
  /// methods are called to terminate a long-running script.
  ///
  /// If such an exception has been thrown, HasTerminated will return true,
  /// indicating that it is possible to call CancelTerminateExecution in order
  /// to continue calling into the engine.
  #[inline(always)]
  pub fn has_terminated(&self) -> bool {
    unsafe { raw::v8__TryCatch__HasTerminated(self.get_raw()) }
  }

  /// Returns true if verbosity is enabled.
  #[inline(always)]
  pub fn is_verbose(&self) -> bool {
    unsafe { raw::v8__TryCatch__IsVerbose(self.get_raw()) }
  }

  /// Set verbosity of the external exception handler.
  ///
  /// By default, exceptions that are caught by an external exception
  /// handler are not reported. Call SetVerbose with true on an
  /// external exception handler to have exceptions caught by the
  /// handler reported as if they were not caught.
  #[inline(always)]
  pub fn set_verbose(&mut self, value: bool) {
    unsafe { raw::v8__TryCatch__SetVerbose(self.get_raw_mut(), value) };
  }

  /// Set whether or not this TryCatch should capture a Message object
  /// which holds source information about where the exception
  /// occurred. True by default.
  #[inline(always)]
  pub fn set_capture_message(&mut self, value: bool) {
    unsafe { raw::v8__TryCatch__SetCaptureMessage(self.get_raw_mut(), value) };
  }

  /// Clears any exceptions that may have been caught by this try/catch block.
  /// After this method has been called, HasCaught() will return false. Cancels
  /// the scheduled exception if it is caught and ReThrow() is not called
  /// before.
  ///
  /// It is not necessary to clear a try/catch block before using it again; if
  /// another exception is thrown the previously caught exception will just be
  /// overwritten. However, it is often a good idea since it makes it easier
  /// to determine which operation threw a given exception.
  #[inline(always)]
  pub fn reset(&mut self) {
    unsafe { raw::v8__TryCatch__Reset(self.get_raw_mut()) };
  }

  #[inline(always)]
  fn get_raw(&self) -> &raw::TryCatch {
    data::ScopeData::get(self).get_try_catch()
  }

  #[inline(always)]
  fn get_raw_mut(&mut self) -> &mut raw::TryCatch {
    data::ScopeData::get_mut(self).get_try_catch_mut()
  }
}

impl<'s, 'p: 's, P> TryCatch<'s, P>
where
  Self: AsMut<HandleScope<'p, ()>>,
{
  /// Returns the exception caught by this try/catch block. If no exception has
  /// been caught an empty handle is returned.
  ///
  /// Note: v8.h states that "the returned handle is valid until this TryCatch
  /// block has been destroyed". This is incorrect; the return value lives
  /// no longer and no shorter than the active HandleScope at the time this
  /// method is called. An issue has been opened about this in the V8 bug
  /// tracker: https://bugs.chromium.org/p/v8/issues/detail?id=10537.
  pub fn exception(&mut self) -> Option<Local<'p, Value>> {
    unsafe {
      self
        .as_mut()
        .cast_local(|sd| raw::v8__TryCatch__Exception(sd.get_try_catch()))
    }
  }

  /// Returns the message associated with this exception. If there is
  /// no message associated an empty handle is returned.
  ///
  /// Note: the remark about the lifetime for the `exception()` return value
  /// applies here too.
  pub fn message(&mut self) -> Option<Local<'p, Message>> {
    unsafe {
      self
        .as_mut()
        .cast_local(|sd| raw::v8__TryCatch__Message(sd.get_try_catch()))
    }
  }

  /// Throws the exception caught by this TryCatch in a way that avoids
  /// it being caught again by this same TryCatch. As with ThrowException
  /// it is illegal to execute any JavaScript operations after calling
  /// ReThrow; the caller must return immediately to where the exception
  /// is caught.
  ///
  /// This function returns the `undefined` value when successful, or `None` if
  /// no exception was caught and therefore there was nothing to rethrow.
  pub fn rethrow(&mut self) -> Option<Local<'_, Value>> {
    unsafe {
      self
        .as_mut()
        .cast_local(|sd| raw::v8__TryCatch__ReThrow(sd.get_try_catch_mut()))
    }
  }
}

impl<'s, 'p: 's, P> TryCatch<'s, P>
where
  Self: AsMut<HandleScope<'p>>,
{
  /// Returns the .stack property of the thrown object. If no .stack
  /// property is present an empty handle is returned.
  pub fn stack_trace(&mut self) -> Option<Local<'p, Value>> {
    unsafe {
      self.as_mut().cast_local(|sd| {
        raw::v8__TryCatch__StackTrace(
          sd.get_try_catch(),
          sd.get_current_context(),
        )
      })
    }
  }
}

/// A `CallbackScope` can be used to bootstrap a `HandleScope` and
/// `ContextScope` inside a callback function that gets called by V8.
/// Bootstrapping a scope inside a callback is the only valid use case of this
/// type; using it in other places leads to undefined behavior, which is also
/// the reason `CallbackScope::new()` is marked as being an unsafe function.
///
/// For some callback types, rusty_v8 internally creates a scope and passes it
/// as an argument to to embedder callback. Eventually we intend to wrap all
/// callbacks in this fashion, so the embedder would never needs to construct
/// a CallbackScope.
///
/// A `CallbackScope<()>`, without context, can be created from:
///   - `&mut Isolate`
///   - `&mut OwnedIsolate`
///
/// A `CallbackScope`, with context, can be created from:
///   - `Local<Context>`
///   - `Local<Message>`
///   - `Local<Object>`
///   - `Local<Promise>`
///   - `Local<SharedArrayBuffer>`
///   - `&FunctionCallbackInfo`
///   - `&PropertyCallbackInfo`
///   - `&PromiseRejectMessage`
///   - `&FastApiCallbackOptions`
#[derive(Debug)]
pub struct CallbackScope<'s, C = Context> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut HandleScope<'s, C>>,
}

impl<'s> CallbackScope<'s> {
  #[allow(clippy::new_ret_no_self)]
  pub unsafe fn new<P: param::NewCallbackScope<'s>>(param: P) -> P::NewScope {
    let context = param.get_context();
    let scope_data =
      data::ScopeData::get_current_mut(unsafe { param.get_isolate_mut() });
    // A HandleScope is not implicitly created for
    // fast functions, so one must be opened here.
    let scope_data = if P::NEEDS_SCOPE {
      if let Some(context) = context {
        scope_data.new_handle_scope_data_with_context(&context)
      } else {
        scope_data.new_handle_scope_data()
      }
    } else {
      scope_data.new_callback_scope_data(context)
    };
    // This scope needs to exit when dropped, as it
    // must not live beyond the callback activation.
    scope_data.disable_zombie();
    scope_data.as_scope()
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum OnFailure {
  CrashOnFailure,
  ThrowOnFailure,
  DumpOnFailure,
}

#[derive(Debug)]
pub struct DisallowJavascriptExecutionScope<'s, P> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut P>,
}

impl<'s, P: param::NewDisallowJavascriptExecutionScope<'s>>
  DisallowJavascriptExecutionScope<'s, P>
{
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'s mut P, on_failure: OnFailure) -> P::NewScope {
    param
      .get_scope_data_mut()
      .new_disallow_javascript_execution_scope_data(on_failure)
      .as_scope()
  }
}

#[derive(Debug)]
pub struct AllowJavascriptExecutionScope<'s, P> {
  _data: NonNull<data::ScopeData>,
  _phantom: PhantomData<&'s mut P>,
}

impl<'s, P: param::NewAllowJavascriptExecutionScope<'s>>
  AllowJavascriptExecutionScope<'s, P>
{
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'s mut P) -> P::NewScope {
    param
      .get_scope_data_mut()
      .new_allow_javascript_execution_scope_data()
      .as_scope()
  }
}

macro_rules! impl_as {
  // Implements `AsRef<Isolate>` and AsMut<Isolate>` on a scope type.
  (<$($params:tt),+> $src_type:ty as Isolate) => {
    impl<$($params),*> AsRef<Isolate> for $src_type {
      fn as_ref(&self) -> &Isolate {
        data::ScopeData::get(self).get_isolate()
      }
    }

    impl<$($params),*> AsMut<Isolate> for $src_type {
      fn as_mut(&mut self) -> &mut Isolate {
        data::ScopeData::get_mut(self).get_isolate_mut()
      }
    }
  };

  // Implements `AsRef` and `AsMut` traits for the purpose of converting a
  // a scope reference to a scope reference with a different but compatible type.
  (<$($params:tt),+> $src_type:ty as $tgt_type:ty) => {
    impl<$($params),*> AsRef<$tgt_type> for $src_type {
      fn as_ref(&self) -> &$tgt_type {
        self.cast_ref()
      }
    }

    impl<$($params),*> AsMut< $tgt_type> for $src_type {
      fn as_mut(&mut self) -> &mut $tgt_type {
        self.cast_mut()
      }
    }
  };
}

impl_as!(<'s, 'p, P> ContextScope<'s, P> as Isolate);
impl_as!(<'s, C> HandleScope<'s, C> as Isolate);
impl_as!(<'s, 'e, C> EscapableHandleScope<'s, 'e, C> as Isolate);
impl_as!(<'s, P> TryCatch<'s, P> as Isolate);
impl_as!(<'s, P> DisallowJavascriptExecutionScope<'s, P> as Isolate);
impl_as!(<'s, P> AllowJavascriptExecutionScope<'s, P> as Isolate);
impl_as!(<'s, C> CallbackScope<'s, C> as Isolate);

impl_as!(<'s, 'p> ContextScope<'s, HandleScope<'p>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, 'e> ContextScope<'s, EscapableHandleScope<'p, 'e>> as HandleScope<'p, ()>);
impl_as!(<'s, C> HandleScope<'s, C> as HandleScope<'s, ()>);
impl_as!(<'s, 'e, C> EscapableHandleScope<'s, 'e, C> as HandleScope<'s, ()>);
impl_as!(<'s, 'p, C> TryCatch<'s, HandleScope<'p, C>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, 'e, C> TryCatch<'s, EscapableHandleScope<'p, 'e, C>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, C> DisallowJavascriptExecutionScope<'s, HandleScope<'p, C>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, 'e, C> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, C> AllowJavascriptExecutionScope<'s, HandleScope<'p, C>> as HandleScope<'p, ()>);
impl_as!(<'s, 'p, 'e, C> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as HandleScope<'p, ()>);
impl_as!(<'s, C> CallbackScope<'s, C> as HandleScope<'s, ()>);

impl_as!(<'s, 'p> ContextScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_as!(<'s, 'p, 'e> ContextScope<'s, EscapableHandleScope<'p, 'e>> as HandleScope<'p>);
impl_as!(<'s> HandleScope<'s> as HandleScope<'s>);
impl_as!(<'s, 'e> EscapableHandleScope<'s, 'e> as HandleScope<'s>);
impl_as!(<'s, 'p> TryCatch<'s, HandleScope<'p>> as HandleScope<'p>);
impl_as!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e>> as HandleScope<'p>);
impl_as!(<'s, 'p> DisallowJavascriptExecutionScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_as!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as HandleScope<'p>);
impl_as!(<'s, 'p> AllowJavascriptExecutionScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_as!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as HandleScope<'p>);
impl_as!(<'s> CallbackScope<'s> as HandleScope<'s>);

impl_as!(<'s, 'p, 'e> ContextScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e, ()>);
impl_as!(<'s, 'e, C> EscapableHandleScope<'s, 'e, C> as EscapableHandleScope<'s, 'e, ()>);
impl_as!(<'s, 'p, 'e, C> TryCatch<'s, EscapableHandleScope<'p, 'e, C>> as EscapableHandleScope<'p, 'e, ()>);
impl_as!(<'s, 'p, 'e, C> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as EscapableHandleScope<'p, 'e, ()>);
impl_as!(<'s, 'p, 'e, C> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as EscapableHandleScope<'p, 'e, ()>);

impl_as!(<'s, 'p, 'e> ContextScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);
impl_as!(<'s, 'e> EscapableHandleScope<'s, 'e> as EscapableHandleScope<'s, 'e>);
impl_as!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);
impl_as!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);
impl_as!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);

impl_as!(<'s, 'p, C> TryCatch<'s, HandleScope<'p, C>> as TryCatch<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> TryCatch<'s, EscapableHandleScope<'p, 'e, C>> as TryCatch<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> TryCatch<'s, EscapableHandleScope<'p, 'e, C>> as TryCatch<'s, EscapableHandleScope<'p, 'e, ()>>);

impl_as!(<'s, 'p> TryCatch<'s, HandleScope<'p>> as TryCatch<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e>> as TryCatch<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e>> as TryCatch<'s, EscapableHandleScope<'p, 'e>>);

impl_as!(<'s, 'p, C> DisallowJavascriptExecutionScope<'s, HandleScope<'p, C>> as DisallowJavascriptExecutionScope<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as DisallowJavascriptExecutionScope<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, ()>>);

impl_as!(<'s, 'p> DisallowJavascriptExecutionScope<'s, HandleScope<'p>> as DisallowJavascriptExecutionScope<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as DisallowJavascriptExecutionScope<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>>);

impl_as!(<'s, 'p, C> AllowJavascriptExecutionScope<'s, HandleScope<'p, C>> as AllowJavascriptExecutionScope<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as AllowJavascriptExecutionScope<'s, HandleScope<'p, ()>>);
impl_as!(<'s, 'p, 'e, C> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>> as AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, ()>>);

impl_as!(<'s, 'p> AllowJavascriptExecutionScope<'s, HandleScope<'p>> as AllowJavascriptExecutionScope<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as AllowJavascriptExecutionScope<'s, HandleScope<'p>>);
impl_as!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>>);

impl_as!(<'s, 'p, P> DisallowJavascriptExecutionScope<'s, TryCatch<'p, P>> as TryCatch<'p, P>);
impl_as!(<'s, 'p, P> AllowJavascriptExecutionScope<'s, TryCatch<'p, P>> as TryCatch<'p, P>);

macro_rules! impl_deref {
  (<$($params:tt),+> $src_type:ty as $tgt_type:ty) => {
    impl<$($params),*> Deref for $src_type {
      type Target = $tgt_type;
      fn deref(&self) -> &Self::Target {
        self.as_ref()
      }
    }

    impl<$($params),*> DerefMut for $src_type {
      fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
      }
    }
  };
}

impl_deref!(<'s, 'p> ContextScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_deref!(<'s, 'p, 'e> ContextScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);

impl_deref!(<'s> HandleScope<'s, ()> as Isolate);
impl_deref!(<'s> HandleScope<'s> as HandleScope<'s, ()>);

impl_deref!(<'s, 'e> EscapableHandleScope<'s, 'e, ()> as HandleScope<'s, ()>);
impl_deref!(<'s, 'e> EscapableHandleScope<'s, 'e> as HandleScope<'s>);

impl_deref!(<'s, 'p> TryCatch<'s, HandleScope<'p, ()>> as HandleScope<'p, ()>);
impl_deref!(<'s, 'p> TryCatch<'s, HandleScope<'p>> as HandleScope<'p>);
impl_deref!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e, ()>> as EscapableHandleScope<'p, 'e, ()>);
impl_deref!(<'s, 'p, 'e> TryCatch<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);

impl_deref!(<'s, 'p> DisallowJavascriptExecutionScope<'s, HandleScope<'p, ()>> as HandleScope<'p, ()>);
impl_deref!(<'s, 'p> DisallowJavascriptExecutionScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_deref!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, ()>> as EscapableHandleScope<'p, 'e, ()>);
impl_deref!(<'s, 'p, 'e> DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);
impl_deref!(<'s, 'p, P> DisallowJavascriptExecutionScope<'s, TryCatch<'p, P>> as TryCatch<'p, P>);

impl_deref!(<'s, 'p> AllowJavascriptExecutionScope<'s, HandleScope<'p, ()>> as HandleScope<'p, ()>);
impl_deref!(<'s, 'p> AllowJavascriptExecutionScope<'s, HandleScope<'p>> as HandleScope<'p>);
impl_deref!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, ()>> as EscapableHandleScope<'p, 'e, ()>);
impl_deref!(<'s, 'p, 'e> AllowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e>> as EscapableHandleScope<'p, 'e>);
impl_deref!(<'s, 'p, P> AllowJavascriptExecutionScope<'s, TryCatch<'p, P>> as TryCatch<'p, P>);

impl_deref!(<'s> CallbackScope<'s, ()> as HandleScope<'s, ()>);
impl_deref!(<'s> CallbackScope<'s> as HandleScope<'s>);

macro_rules! impl_scope_drop {
  (<$($params:tt),+> $type:ty) => {
    unsafe impl<$($params),*> Scope for $type {}

    impl<$($params),*> Drop for $type {
      #[inline(always)]
      fn drop(&mut self) {
        data::ScopeData::get_mut(self).notify_scope_dropped();
      }
    }
  };
}

impl_scope_drop!(<'s, 'p, P> ContextScope<'s, P>);
impl_scope_drop!(<'s, C> HandleScope<'s, C> );
impl_scope_drop!(<'s, 'e, C> EscapableHandleScope<'s, 'e, C> );
impl_scope_drop!(<'s, P> TryCatch<'s, P> );
impl_scope_drop!(<'s, P> DisallowJavascriptExecutionScope<'s, P>);
impl_scope_drop!(<'s, P> AllowJavascriptExecutionScope<'s, P>);
impl_scope_drop!(<'s, C> CallbackScope<'s, C> );

pub unsafe trait Scope: Sized {}

trait ScopeCast: Sized {
  fn cast_ref<S: Scope>(&self) -> &S;
  fn cast_mut<S: Scope>(&mut self) -> &mut S;
}

impl<T: Scope> ScopeCast for T {
  fn cast_ref<S: Scope>(&self) -> &S {
    assert_eq!(Layout::new::<Self>(), Layout::new::<S>());
    unsafe { &*(self as *const _ as *const S) }
  }

  fn cast_mut<S: Scope>(&mut self) -> &mut S {
    assert_eq!(Layout::new::<Self>(), Layout::new::<S>());
    unsafe { &mut *(self as *mut _ as *mut S) }
  }
}

/// Scopes are typically constructed as the child of another scope. The scope
/// that is returned from `«Child»Scope::new(parent: &mut «Parent»Scope)` does
/// not necessarily have type `«Child»Scope`, but rather its type is a merger of
/// both the the parent and child scope types.
///
/// For example: a `ContextScope` created inside `HandleScope<'a, ()>` does not
/// produce a `ContextScope`, but rather a `HandleScope<'a, Context>`, which
/// describes a scope that is both a `HandleScope` _and_ a `ContextScope`.
///
/// The Traits in the (private) `param` module define which types can be passed
/// as a parameter to the `«Some»Scope::new()` constructor, and what the
/// actual, merged scope type will be that `new()` returns for a specific
/// parameter type.
mod param {
  use super::*;

  pub trait NewContextScope<'s>: getter::GetScopeData {
    type NewScope: Scope;
  }

  impl<'s, 'p: 's, P: Scope> NewContextScope<'s> for ContextScope<'p, P> {
    type NewScope = ContextScope<'s, P>;
  }

  impl<'s, 'p: 's, C> NewContextScope<'s> for HandleScope<'p, C> {
    type NewScope = ContextScope<'s, HandleScope<'p>>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewContextScope<'s>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope = ContextScope<'s, EscapableHandleScope<'p, 'e>>;
  }

  impl<'s, 'p: 's, P: NewContextScope<'s>> NewContextScope<'s>
    for TryCatch<'p, P>
  {
    type NewScope = <P as NewContextScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, P: NewContextScope<'s>> NewContextScope<'s>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewContextScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, P: NewContextScope<'s>> NewContextScope<'s>
    for AllowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewContextScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewContextScope<'s> for CallbackScope<'p, C> {
    type NewScope = ContextScope<'s, HandleScope<'p>>;
  }

  pub trait NewHandleScope<'s>: getter::GetScopeData {
    type NewScope: Scope;
  }

  impl<'s> NewHandleScope<'s> for Isolate {
    type NewScope = HandleScope<'s, ()>;
  }

  impl<'s> NewHandleScope<'s> for OwnedIsolate {
    type NewScope = HandleScope<'s, ()>;
  }

  impl<'s, 'p: 's, P: NewHandleScope<'s>> NewHandleScope<'s>
    for ContextScope<'p, P>
  {
    type NewScope = <P as NewHandleScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewHandleScope<'s> for HandleScope<'p, C> {
    type NewScope = HandleScope<'s, C>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewHandleScope<'s>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope = EscapableHandleScope<'s, 'e, C>;
  }

  impl<'s, 'p: 's, P: NewHandleScope<'s>> NewHandleScope<'s> for TryCatch<'p, P> {
    type NewScope = <P as NewHandleScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, P: NewHandleScope<'s>> NewHandleScope<'s>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewHandleScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, P: NewHandleScope<'s>> NewHandleScope<'s>
    for AllowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewHandleScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewHandleScope<'s> for CallbackScope<'p, C> {
    type NewScope = HandleScope<'s, C>;
  }

  pub trait NewHandleScopeWithContext<'s>: getter::GetScopeData {
    fn get_isolate_mut(&mut self) -> &mut Isolate;
  }

  impl NewHandleScopeWithContext<'_> for Isolate {
    fn get_isolate_mut(&mut self) -> &mut Isolate {
      self
    }
  }

  impl NewHandleScopeWithContext<'_> for OwnedIsolate {
    fn get_isolate_mut(&mut self) -> &mut Isolate {
      &mut *self
    }
  }

  pub trait NewEscapableHandleScope<'s, 'e: 's>: getter::GetScopeData {
    type NewScope: Scope;
  }

  impl<'s, 'p: 's, 'e: 'p, P: NewEscapableHandleScope<'s, 'e>>
    NewEscapableHandleScope<'s, 'e> for ContextScope<'p, P>
  {
    type NewScope = <P as NewEscapableHandleScope<'s, 'e>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewEscapableHandleScope<'s, 'p> for HandleScope<'p, C> {
    type NewScope = EscapableHandleScope<'s, 'p, C>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewEscapableHandleScope<'s, 'p>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope = EscapableHandleScope<'s, 'p, C>;
  }

  impl<'s, 'p: 's, 'e: 'p, P: NewEscapableHandleScope<'s, 'e>>
    NewEscapableHandleScope<'s, 'e> for TryCatch<'p, P>
  {
    type NewScope = <P as NewEscapableHandleScope<'s, 'e>>::NewScope;
  }

  impl<'s, 'p: 's, 'e: 'p, P: NewEscapableHandleScope<'s, 'e>>
    NewEscapableHandleScope<'s, 'e>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewEscapableHandleScope<'s, 'e>>::NewScope;
  }

  impl<'s, 'p: 's, 'e: 'p, P: NewEscapableHandleScope<'s, 'e>>
    NewEscapableHandleScope<'s, 'e> for AllowJavascriptExecutionScope<'p, P>
  {
    type NewScope = <P as NewEscapableHandleScope<'s, 'e>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewEscapableHandleScope<'s, 'p> for CallbackScope<'p, C> {
    type NewScope = EscapableHandleScope<'s, 'p, C>;
  }

  pub trait NewTryCatch<'s>: getter::GetScopeData {
    type NewScope: Scope;
  }

  impl<'s, 'p: 's, P: NewTryCatch<'s>> NewTryCatch<'s> for ContextScope<'p, P> {
    type NewScope = <P as NewTryCatch<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewTryCatch<'s> for HandleScope<'p, C> {
    type NewScope = TryCatch<'s, HandleScope<'p, C>>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewTryCatch<'s>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope = TryCatch<'s, EscapableHandleScope<'p, 'e, C>>;
  }

  impl<'s, 'p: 's, P> NewTryCatch<'s> for TryCatch<'p, P> {
    type NewScope = TryCatch<'s, P>;
  }

  impl<'s, 'p: 's, P> NewTryCatch<'s>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = TryCatch<'s, P>;
  }

  impl<'s, 'p: 's, P> NewTryCatch<'s> for AllowJavascriptExecutionScope<'p, P> {
    type NewScope = TryCatch<'s, P>;
  }

  impl<'s, 'p: 's, C> NewTryCatch<'s> for CallbackScope<'p, C> {
    type NewScope = TryCatch<'s, HandleScope<'p, C>>;
  }

  pub trait NewDisallowJavascriptExecutionScope<'s>:
    getter::GetScopeData
  {
    type NewScope: Scope;
  }

  impl<'s, 'p: 's, P: NewDisallowJavascriptExecutionScope<'s>>
    NewDisallowJavascriptExecutionScope<'s> for ContextScope<'p, P>
  {
    type NewScope = <P as NewDisallowJavascriptExecutionScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewDisallowJavascriptExecutionScope<'s>
    for HandleScope<'p, C>
  {
    type NewScope = DisallowJavascriptExecutionScope<'s, HandleScope<'p, C>>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewDisallowJavascriptExecutionScope<'s>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope =
      DisallowJavascriptExecutionScope<'s, EscapableHandleScope<'p, 'e, C>>;
  }

  impl<'s, 'p: 's, P> NewDisallowJavascriptExecutionScope<'s>
    for TryCatch<'p, P>
  {
    type NewScope = DisallowJavascriptExecutionScope<'s, TryCatch<'p, P>>;
  }

  impl<'s, 'p: 's, P> NewDisallowJavascriptExecutionScope<'s>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = DisallowJavascriptExecutionScope<'s, P>;
  }

  impl<'s, 'p: 's, P> NewDisallowJavascriptExecutionScope<'s>
    for AllowJavascriptExecutionScope<'p, P>
  {
    type NewScope = DisallowJavascriptExecutionScope<'s, P>;
  }

  pub trait NewAllowJavascriptExecutionScope<'s>: getter::GetScopeData {
    type NewScope: Scope;
  }

  impl<'s, 'p: 's, P: NewAllowJavascriptExecutionScope<'s>>
    NewAllowJavascriptExecutionScope<'s> for ContextScope<'p, P>
  {
    type NewScope = <P as NewAllowJavascriptExecutionScope<'s>>::NewScope;
  }

  impl<'s, 'p: 's, C> NewAllowJavascriptExecutionScope<'s>
    for HandleScope<'p, C>
  {
    type NewScope = HandleScope<'p, C>;
  }

  impl<'s, 'p: 's, 'e: 'p, C> NewAllowJavascriptExecutionScope<'s>
    for EscapableHandleScope<'p, 'e, C>
  {
    type NewScope = EscapableHandleScope<'p, 'e, C>;
  }

  impl<'s, 'p: 's, P> NewAllowJavascriptExecutionScope<'s> for TryCatch<'p, P> {
    type NewScope = TryCatch<'s, P>;
  }

  impl<'s, 'p: 's, P: Scope> NewAllowJavascriptExecutionScope<'s>
    for DisallowJavascriptExecutionScope<'p, P>
  {
    type NewScope = P;
  }

  impl<'s, 'p: 's, P> NewAllowJavascriptExecutionScope<'s>
    for AllowJavascriptExecutionScope<'p, P>
  {
    type NewScope = AllowJavascriptExecutionScope<'s, P>;
  }

  pub trait NewCallbackScope<'s>: Sized + getter::GetIsolate<'s> {
    type NewScope: Scope;
    const NEEDS_SCOPE: bool = false;

    fn get_context(&self) -> Option<Local<'s, Context>> {
      None
    }
  }

  impl<'s> NewCallbackScope<'s> for &'s mut Isolate {
    type NewScope = CallbackScope<'s, ()>;
  }

  impl<'s> NewCallbackScope<'s> for &'s mut OwnedIsolate {
    type NewScope = CallbackScope<'s, ()>;
  }

  impl<'s> NewCallbackScope<'s> for &'s FunctionCallbackInfo {
    type NewScope = CallbackScope<'s>;
  }

  impl<'s, T> NewCallbackScope<'s> for &'s PropertyCallbackInfo<T> {
    type NewScope = CallbackScope<'s>;
  }

  impl<'s> NewCallbackScope<'s> for &'s FastApiCallbackOptions<'s> {
    type NewScope = CallbackScope<'s>;
    const NEEDS_SCOPE: bool = true;
  }

  impl<'s> NewCallbackScope<'s> for Local<'s, Context> {
    type NewScope = CallbackScope<'s>;

    fn get_context(&self) -> Option<Local<'s, Context>> {
      Some(*self)
    }
  }

  impl<'s> NewCallbackScope<'s> for Local<'s, Message> {
    type NewScope = CallbackScope<'s>;
  }

  impl<'s, T: Into<Local<'s, Object>>> NewCallbackScope<'s> for T {
    type NewScope = CallbackScope<'s>;
  }

  impl<'s> NewCallbackScope<'s> for &'s PromiseRejectMessage<'s> {
    type NewScope = CallbackScope<'s>;
  }
}

/// The private `getter` module defines traits to look up the related `Isolate`
/// and `ScopeData` for many different types. The implementation of those traits
/// on the types that implement them are also all contained in this module.
mod getter {
  pub use super::*;

  pub trait GetIsolate<'s> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate;
  }

  impl<'s> GetIsolate<'s> for &'s mut Isolate {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      self
    }
  }

  impl<'s> GetIsolate<'s> for &'s mut OwnedIsolate {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      &mut *self
    }
  }

  impl<'s> GetIsolate<'s> for &'s FunctionCallbackInfo {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      unsafe { &mut *self.get_isolate_ptr() }
    }
  }

  impl<'s, T> GetIsolate<'s> for &'s PropertyCallbackInfo<T> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      unsafe { &mut *self.get_isolate_ptr() }
    }
  }

  impl<'s> GetIsolate<'s> for &'s FastApiCallbackOptions<'s> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      unsafe { &mut *self.isolate }
    }
  }

  impl<'s> GetIsolate<'s> for Local<'s, Context> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      unsafe { &mut *raw::v8__Context__GetIsolate(&*self) }
    }
  }

  impl<'s> GetIsolate<'s> for Local<'s, Message> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      unsafe { &mut *raw::v8__Message__GetIsolate(&*self) }
    }
  }

  impl<'s, T: Into<Local<'s, Object>>> GetIsolate<'s> for T {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      let object: Local<Object> = self.into();
      unsafe { &mut *raw::v8__Object__GetIsolate(&*object) }
    }
  }

  impl<'s> GetIsolate<'s> for &'s PromiseRejectMessage<'s> {
    unsafe fn get_isolate_mut(self) -> &'s mut Isolate {
      let object: Local<Object> = self.get_promise().into();
      unsafe { &mut *raw::v8__Object__GetIsolate(&*object) }
    }
  }

  pub trait GetScopeData {
    fn get_scope_data_mut(&mut self) -> &mut data::ScopeData;
  }

  impl<T: Scope> GetScopeData for T {
    fn get_scope_data_mut(&mut self) -> &mut data::ScopeData {
      data::ScopeData::get_mut(self)
    }
  }

  impl GetScopeData for Isolate {
    fn get_scope_data_mut(&mut self) -> &mut data::ScopeData {
      data::ScopeData::get_root_mut(self)
    }
  }

  impl GetScopeData for OwnedIsolate {
    fn get_scope_data_mut(&mut self) -> &mut data::ScopeData {
      data::ScopeData::get_root_mut(self)
    }
  }
}

/// All publicly exported `«Some»Scope` types are essentially wrapping a pointer
/// to a heap-allocated struct `ScopeData`. This module contains the definition
/// for `ScopeData` and its inner types, as well as related helper traits.
pub(crate) mod data {
  use super::*;

  #[derive(Debug)]
  pub struct ScopeData {
    // The first four fields are always valid - even when the `Box<ScopeData>`
    // struct is free (does not contain data related to an actual scope).
    // The `previous` and `isolate` fields never change; the `next` field is
    // set to `None` initially when the struct is created, but it may later be
    // assigned a `Some(Box<ScopeData>)` value, after which this field never
    // changes again.
    isolate: NonNull<Isolate>,
    previous: Option<NonNull<ScopeData>>,
    next: Option<Box<ScopeData>>,
    // The 'status' field is also always valid (but does change).
    status: Cell<ScopeStatus>,
    // The following fields are only valid when this ScopeData object is in use
    // (eiter current or shadowed -- not free).
    context: Cell<Option<NonNull<Context>>>,
    escape_slot: Option<NonNull<Option<raw::EscapeSlot>>>,
    try_catch: Option<NonNull<raw::TryCatch>>,
    scope_type_specific_data: ScopeTypeSpecificData,
  }

  impl ScopeData {
    /// Returns a mutable reference to the data associated with topmost scope
    /// on the scope stack. This function does not automatically exit zombie
    /// scopes, so it might return a zombie ScopeData reference.
    #[inline(always)]
    pub(crate) fn get_current_mut(isolate: &mut Isolate) -> &mut Self {
      let self_mut = isolate
        .get_current_scope_data()
        .map(NonNull::as_ptr)
        .map(|p| unsafe { &mut *p })
        .unwrap();
      match self_mut.status.get() {
        ScopeStatus::Current { .. } => self_mut,
        _ => unreachable!(),
      }
    }

    /// Initializes the scope stack by creating a 'dummy' `ScopeData` at the
    /// very bottom. This makes it possible to store the freelist of reusable
    /// ScopeData objects even when no scope is entered.
    pub(crate) fn new_root(isolate: &mut Isolate) {
      let root = Box::leak(Self::boxed(isolate.into()));
      root.status = ScopeStatus::Current { zombie: false }.into();
      debug_assert!(isolate.get_current_scope_data().is_none());
      isolate.set_current_scope_data(Some(root.into()));
    }

    /// Activates and returns the 'root' `ScopeData` object that is created when
    /// the isolate is initialized. In order to do so, any zombie scopes that
    /// remain on the scope stack are cleaned up.
    ///
    /// # Panics
    ///
    /// This function panics if the root can't be activated because there are
    /// still other scopes on the stack and they're not zombies.
    #[inline(always)]
    pub(crate) fn get_root_mut(isolate: &mut Isolate) -> &mut Self {
      let mut current_scope_data = Self::get_current_mut(isolate);
      loop {
        current_scope_data = match current_scope_data {
          root if root.previous.is_none() => break root,
          data => data.try_exit_scope(),
        };
      }
    }

    /// Drops the scope stack and releases all `Box<ScopeData>` allocations.
    /// This function should be called only when an Isolate is being disposed.
    pub(crate) fn drop_root(isolate: &mut Isolate) {
      let root = Self::get_root_mut(isolate);
      unsafe {
        let _ = Box::from_raw(root);
      };
      isolate.set_current_scope_data(None);
    }

    #[inline(always)]
    pub(super) fn new_context_scope_data<'s>(
      &'s mut self,
      context: Local<'s, Context>,
    ) -> &'s mut Self {
      self.new_scope_data_with(move |data| {
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::ContextScope {
            _raw_context_scope: raw::ContextScope::new(context),
          }
        });
        data.context.set(Some(context.as_non_null()));
      })
    }

    /// Implementation helper function, which creates the raw `HandleScope`, but
    /// defers (maybe) entering a context to the provided callback argument.
    /// This function gets called by `Self::new_handle_scope_data()` and
    /// `Self::new_handle_scope_data_with_context()`.
    #[inline(always)]
    fn new_handle_scope_data_with<F>(&mut self, init_context_fn: F) -> &mut Self
    where
      F: FnOnce(
        NonNull<Isolate>,
        &mut Cell<Option<NonNull<Context>>>,
        &mut Option<raw::ContextScope>,
      ),
    {
      self.new_scope_data_with(|data| {
        let isolate = data.isolate;
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::HandleScope {
            allow_zombie: true,
            raw_handle_scope: unsafe { raw::HandleScope::uninit() },
            raw_context_scope: None,
          }
        });
        match &mut data.scope_type_specific_data {
          ScopeTypeSpecificData::HandleScope {
            raw_handle_scope,
            raw_context_scope,
            ..
          } => {
            unsafe { raw_handle_scope.init(isolate) };
            init_context_fn(isolate, &mut data.context, raw_context_scope);
          }
          _ => unreachable!(),
        };
      })
    }

    #[inline(always)]
    pub(super) fn disable_zombie(&mut self) {
      if let ScopeTypeSpecificData::HandleScope { allow_zombie, .. } =
        &mut self.scope_type_specific_data
      {
        *allow_zombie = false;
      }
    }

    #[inline(always)]
    pub(super) fn new_handle_scope_data(&mut self) -> &mut Self {
      self.new_handle_scope_data_with(|_, _, raw_context_scope| {
        debug_assert!(raw_context_scope.is_none());
      })
    }

    #[inline(always)]
    pub(super) fn new_handle_scope_data_with_context(
      &mut self,
      context_ref: &Context,
    ) -> &mut Self {
      self.new_handle_scope_data_with(
        move |isolate, context_data, raw_context_scope| unsafe {
          let context_nn = NonNull::from(context_ref);
          // Copy the `Context` reference to a new local handle to enure that it
          // cannot get garbage collected until after this scope is dropped.
          let local_context_ptr =
            raw::v8__Local__New(isolate.as_ptr(), context_nn.cast().as_ptr())
              as *const Context;
          let local_context_nn =
            NonNull::new_unchecked(local_context_ptr as *mut _);
          let local_context = Local::from_non_null(local_context_nn);
          // Initialize the `raw::ContextScope`. This enters the context too.
          debug_assert!(raw_context_scope.is_none());
          ptr::write(
            raw_context_scope,
            Some(raw::ContextScope::new(local_context)),
          );
          // Also store the newly created `Local<Context>` in the `Cell` that
          // serves as a look-up cache for the current context.
          context_data.set(Some(local_context_nn));
        },
      )
    }

    #[inline(always)]
    pub(super) fn new_escapable_handle_scope_data(&mut self) -> &mut Self {
      self.new_scope_data_with(|data| {
        // Note: the `raw_escape_slot` field must be initialized _before_ the
        // `raw_handle_scope` field, otherwise the escaped local handle ends up
        // inside the `EscapableHandleScope` that's being constructed here,
        // rather than escaping from it.
        let isolate = data.isolate;
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::EscapableHandleScope {
            raw_handle_scope: unsafe { raw::HandleScope::uninit() },
            raw_escape_slot: Some(raw::EscapeSlot::new(isolate)),
          }
        });
        match &mut data.scope_type_specific_data {
          ScopeTypeSpecificData::EscapableHandleScope {
            raw_handle_scope,
            raw_escape_slot,
          } => {
            unsafe { raw_handle_scope.init(isolate) };
            data.escape_slot.replace(raw_escape_slot.into());
          }
          _ => unreachable!(),
        }
      })
    }

    #[inline(always)]
    pub(super) fn new_try_catch_data(&mut self) -> &mut Self {
      self.new_scope_data_with(|data| {
        let isolate = data.isolate;
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::TryCatch {
            raw_try_catch: unsafe { raw::TryCatch::uninit() },
          }
        });
        match &mut data.scope_type_specific_data {
          ScopeTypeSpecificData::TryCatch { raw_try_catch } => {
            unsafe { raw_try_catch.init(isolate) };
            data.try_catch.replace(raw_try_catch.into());
          }
          _ => unreachable!(),
        }
      })
    }

    #[inline(always)]
    pub(super) fn new_disallow_javascript_execution_scope_data(
      &mut self,
      on_failure: OnFailure,
    ) -> &mut Self {
      self.new_scope_data_with(|data| {
        let isolate = data.isolate;
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::DisallowJavascriptExecutionScope {
            raw_scope: unsafe {
              raw::DisallowJavascriptExecutionScope::uninit()
            },
          }
        });
        match &mut data.scope_type_specific_data {
          ScopeTypeSpecificData::DisallowJavascriptExecutionScope {
            raw_scope,
          } => unsafe { raw_scope.init(isolate, on_failure) },
          _ => unreachable!(),
        }
      })
    }

    #[inline(always)]
    pub(super) fn new_allow_javascript_execution_scope_data(
      &mut self,
    ) -> &mut Self {
      self.new_scope_data_with(|data| {
        let isolate = data.isolate;
        data.scope_type_specific_data.init_with(|| {
          ScopeTypeSpecificData::AllowJavascriptExecutionScope {
            raw_scope: unsafe { raw::AllowJavascriptExecutionScope::uninit() },
          }
        });
        match &mut data.scope_type_specific_data {
          ScopeTypeSpecificData::AllowJavascriptExecutionScope {
            raw_scope,
          } => unsafe { raw_scope.init(isolate) },
          _ => unreachable!(),
        }
      })
    }

    #[inline(always)]
    pub(super) fn new_callback_scope_data<'s>(
      &'s mut self,
      maybe_current_context: Option<Local<'s, Context>>,
    ) -> &'s mut Self {
      self.new_scope_data_with(|data| {
        debug_assert!(data.scope_type_specific_data.is_none());
        data
          .context
          .set(maybe_current_context.map(|cx| cx.as_non_null()));
      })
    }

    #[inline(always)]
    fn new_scope_data_with(
      &mut self,
      init_fn: impl FnOnce(&mut Self),
    ) -> &mut Self {
      // Mark this scope (the parent of the newly created scope) as 'shadowed';
      self.status.set(match self.status.get() {
        ScopeStatus::Current { zombie } => ScopeStatus::Shadowed { zombie },
        _ => unreachable!(),
      });
      // Copy fields that that will be inherited by the new scope.
      let context = self.context.get().into();
      let escape_slot = self.escape_slot;
      // Initialize the `struct ScopeData` for the new scope.
      let new_scope_data = self.allocate_or_reuse_scope_data();
      // In debug builds, `zombie` is initially set to `true`, and the flag is
      // later cleared in the `as_scope()` method, to verify that we're
      // always creating exactly one scope from any `ScopeData` object.
      // For performance reasons this check is not performed in release builds.
      new_scope_data.status = Cell::new(ScopeStatus::Current {
        zombie: cfg!(debug_assertions),
      });
      // Store fields inherited from the parent scope.
      new_scope_data.context = context;
      new_scope_data.escape_slot = escape_slot;
      (init_fn)(new_scope_data);
      // Make the newly created scope the 'current' scope for this isolate.
      let new_scope_nn = unsafe { NonNull::new_unchecked(new_scope_data) };
      new_scope_data
        .get_isolate_mut()
        .set_current_scope_data(Some(new_scope_nn));
      new_scope_data
    }

    /// Either returns an free `Box<ScopeData>` that is available for reuse,
    /// or allocates a new one on the heap.
    #[inline(always)]
    fn allocate_or_reuse_scope_data(&mut self) -> &mut Self {
      let self_nn = NonNull::new(self);
      match &mut self.next {
        Some(next_box) => {
          // Reuse a free `Box<ScopeData>` allocation.
          debug_assert_eq!(next_box.isolate, self.isolate);
          debug_assert_eq!(next_box.previous, self_nn);
          debug_assert_eq!(next_box.status.get(), ScopeStatus::Free);
          debug_assert!(next_box.scope_type_specific_data.is_none());
          next_box.as_mut()
        }
        next_field @ None => {
          // Allocate a new `Box<ScopeData>`.
          let mut next_box = Self::boxed(self.isolate);
          next_box.previous = self_nn;
          next_field.replace(next_box);
          next_field.as_mut().unwrap()
        }
      }
    }

    #[inline(always)]
    pub(super) fn as_scope<S: Scope>(&mut self) -> S {
      assert_eq!(Layout::new::<&mut Self>(), Layout::new::<S>());
      // In debug builds, a new initialized `ScopeStatus` will have the `zombie`
      // flag set, so we have to reset it. In release builds, new `ScopeStatus`
      // objects come with the `zombie` flag cleared, so no update is necessary.
      if cfg!(debug_assertions) {
        assert_eq!(self.status.get(), ScopeStatus::Current { zombie: true });
        self.status.set(ScopeStatus::Current { zombie: false });
      }
      let self_nn = NonNull::from(self);
      unsafe { ptr::read(&self_nn as *const _ as *const S) }
    }

    #[inline(always)]
    pub(super) fn get<S: Scope>(scope: &S) -> &Self {
      let self_mut = unsafe {
        (*(scope as *const S as *mut S as *mut NonNull<Self>)).as_mut()
      };
      self_mut.try_activate_scope();
      self_mut
    }

    #[inline(always)]
    pub(super) fn get_mut<S: Scope>(scope: &mut S) -> &mut Self {
      let self_mut =
        unsafe { (*(scope as *mut S as *mut NonNull<Self>)).as_mut() };
      self_mut.try_activate_scope();
      self_mut
    }

    #[inline(always)]
    fn try_activate_scope(mut self: &mut Self) -> &mut Self {
      self = match self.status.get() {
        ScopeStatus::Current { zombie: false } => self,
        ScopeStatus::Shadowed { zombie: false } => {
          self.next.as_mut().unwrap().try_exit_scope()
        }
        _ => unreachable!(),
      };
      debug_assert_eq!(
        self.get_isolate().get_current_scope_data(),
        NonNull::new(self as *mut _)
      );
      self
    }

    #[inline(always)]
    fn try_exit_scope(mut self: &mut Self) -> &mut Self {
      loop {
        self = match self.status.get() {
          ScopeStatus::Shadowed { .. } => {
            self.next.as_mut().unwrap().try_exit_scope()
          }
          ScopeStatus::Current { zombie: true } => break self.exit_scope(),
          ScopeStatus::Current { zombie: false } => {
            panic!("active scope can't be dropped")
          }
          _ => unreachable!(),
        }
      }
    }

    #[inline(always)]
    fn exit_scope(&mut self) -> &mut Self {
      // Clear out the scope type specific data field. None of the other fields
      // have a destructor, and there's no need to do any cleanup on them.
      if !matches!(self.scope_type_specific_data, ScopeTypeSpecificData::None) {
        self.scope_type_specific_data = Default::default();
      }

      // Change the ScopeData's status field from 'Current' to 'Free', which
      // means that it is not associated with a scope and can be reused.
      self.status.set(ScopeStatus::Free);

      // Point the Isolate's current scope data slot at our parent scope.
      let previous_nn = self.previous.unwrap();
      self
        .get_isolate_mut()
        .set_current_scope_data(Some(previous_nn));

      // Update the parent scope's status field to reflect that it is now
      // 'Current' again an no longer 'Shadowed'.
      let previous_mut = unsafe { &mut *previous_nn.as_ptr() };
      previous_mut.status.set(match previous_mut.status.get() {
        ScopeStatus::Shadowed { zombie } => ScopeStatus::Current { zombie },
        _ => unreachable!(),
      });

      previous_mut
    }

    /// This function is called when any of the public scope objects (e.g
    /// `HandleScope`, `ContextScope`, etc.) are dropped.
    ///
    /// The Rust borrow checker allows values of type `HandleScope<'a>` and
    /// `EscapableHandleScope<'a, 'e>` to be dropped before their maximum
    /// lifetime ('a) is up. This creates a potential problem because any local
    /// handles that are created while these scopes are active are bound to
    /// that 'a lifetime. This means that we run the risk of creating local
    /// handles that outlive their creation scope.
    ///
    /// Therefore, we don't immediately exit the current scope at the very
    /// moment the user drops their Escapable/HandleScope handle.
    /// Instead, the current scope is marked as being a 'zombie': the scope
    /// itself is gone, but its data still on the stack. The zombie's data will
    /// be dropped when the user touches the parent scope; when that happens, it
    /// is certain that there are no accessible `Local<'a, T>` handles left,
    /// because the 'a lifetime ends there.
    ///
    /// Scope types that do no store local handles are exited immediately.
    #[inline(always)]
    pub(super) fn notify_scope_dropped(&mut self) {
      match &self.scope_type_specific_data {
        ScopeTypeSpecificData::HandleScope {
          allow_zombie: true, ..
        }
        | ScopeTypeSpecificData::EscapableHandleScope { .. } => {
          // Defer scope exit until the parent scope is touched.
          self.status.set(match self.status.get() {
            ScopeStatus::Current { zombie: false } => {
              ScopeStatus::Current { zombie: true }
            }
            _ => unreachable!(),
          });
        }
        _ => {
          // Regular, immediate exit.
          self.exit_scope();
        }
      }
    }

    #[inline(always)]
    pub(crate) fn get_isolate(&self) -> &Isolate {
      unsafe { self.isolate.as_ref() }
    }

    #[inline(always)]
    pub(crate) fn get_isolate_mut(&mut self) -> &mut Isolate {
      unsafe { self.isolate.as_mut() }
    }

    #[inline(always)]
    pub(crate) fn get_isolate_ptr(&self) -> *mut Isolate {
      self.isolate.as_ptr()
    }

    #[inline(always)]
    pub(crate) fn get_current_context(&self) -> *const Context {
      // To avoid creating a new Local every time `get_current_context() is
      // called, the current context is usually cached in the `context` field.
      // If the `context` field contains `None`, this might mean that this cache
      // field never got populated, so we'll do that here when necessary.
      let get_current_context_from_isolate = || unsafe {
        raw::v8__Isolate__GetCurrentContext(self.get_isolate_ptr())
      };
      match self.context.get().map(|nn| nn.as_ptr() as *const _) {
        Some(context) => {
          debug_assert!(unsafe {
            raw::v8__Context__EQ(context, get_current_context_from_isolate())
          });
          context
        }
        None => {
          let context = get_current_context_from_isolate();
          self.context.set(NonNull::new(context as *mut _));
          context
        }
      }
    }

    #[inline(always)]
    pub(super) fn get_escape_slot_mut(
      &mut self,
    ) -> Option<&mut Option<raw::EscapeSlot>> {
      self
        .escape_slot
        .as_mut()
        .map(|escape_slot_nn| unsafe { escape_slot_nn.as_mut() })
    }

    #[inline(always)]
    pub(super) fn get_try_catch(&self) -> &raw::TryCatch {
      self
        .try_catch
        .as_ref()
        .map(|try_catch_nn| unsafe { try_catch_nn.as_ref() })
        .unwrap()
    }

    #[inline(always)]
    pub(super) fn get_try_catch_mut(&mut self) -> &mut raw::TryCatch {
      self
        .try_catch
        .as_mut()
        .map(|try_catch_nn| unsafe { try_catch_nn.as_mut() })
        .unwrap()
    }

    /// Returns a new `Box<ScopeData>` with the `isolate` field set as specified
    /// by the first parameter, and the other fields initialized to their
    /// default values. This function exists solely because it turns out that
    /// Rust doesn't optimize `Box::new(Self{ .. })` very well (a.k.a. not at
    /// all) in this case, which is why `std::alloc::alloc()` is used directly.
    #[cold]
    fn boxed(isolate: NonNull<Isolate>) -> Box<Self> {
      unsafe {
        #[allow(clippy::cast_ptr_alignment)]
        let self_ptr = alloc(Layout::new::<Self>()) as *mut Self;
        ptr::write(
          self_ptr,
          Self {
            isolate,
            previous: Default::default(),
            next: Default::default(),
            status: Default::default(),
            context: Default::default(),
            escape_slot: Default::default(),
            try_catch: Default::default(),
            scope_type_specific_data: Default::default(),
          },
        );
        Box::from_raw(self_ptr)
      }
    }
  }

  #[derive(Debug, Clone, Copy, Eq, PartialEq)]
  enum ScopeStatus {
    Free,
    Current { zombie: bool },
    Shadowed { zombie: bool },
  }

  impl Default for ScopeStatus {
    fn default() -> Self {
      Self::Free
    }
  }

  #[derive(Debug)]
  enum ScopeTypeSpecificData {
    None,
    ContextScope {
      _raw_context_scope: raw::ContextScope,
    },
    HandleScope {
      allow_zombie: bool,
      raw_handle_scope: raw::HandleScope,
      raw_context_scope: Option<raw::ContextScope>,
    },
    EscapableHandleScope {
      raw_handle_scope: raw::HandleScope,
      raw_escape_slot: Option<raw::EscapeSlot>,
    },
    TryCatch {
      raw_try_catch: raw::TryCatch,
    },
    DisallowJavascriptExecutionScope {
      raw_scope: raw::DisallowJavascriptExecutionScope,
    },
    AllowJavascriptExecutionScope {
      raw_scope: raw::AllowJavascriptExecutionScope,
    },
  }

  impl Default for ScopeTypeSpecificData {
    fn default() -> Self {
      Self::None
    }
  }

  impl Drop for ScopeTypeSpecificData {
    #[inline(always)]
    fn drop(&mut self) {
      // For `HandleScope`s that also enter a `Context`, drop order matters. The
      // context is stored in a `Local` handle, which is allocated in this
      // scope's own private `raw::HandleScope`. When that `raw::HandleScope`
      // is dropped first, we immediately lose the `Local<Context>` handle,
      // which we need in order to exit `ContextScope`.
      if let Self::HandleScope {
        raw_context_scope, ..
      } = self
      {
        *raw_context_scope = None;
      }
    }
  }

  impl ScopeTypeSpecificData {
    pub fn is_none(&self) -> bool {
      matches!(self, Self::None)
    }

    /// Replaces a `ScopeTypeSpecificData::None` value with the value returned
    /// from the specified closure. This function exists because initializing
    /// scopes is performance critical, and `ptr::write()` produces more
    /// efficient code than using a regular assign statement, which will try to
    /// drop the old value and move the new value into place, even after
    /// asserting `self.is_none()`.
    pub fn init_with(&mut self, init_fn: impl FnOnce() -> Self) {
      assert!(self.is_none());
      unsafe { ptr::write(self, (init_fn)()) }
    }
  }
}

/// The `raw` module contains prototypes for all the `extern C` functions that
/// are used in this file, as well as definitions for the types they operate on.
mod raw {
  use super::*;

  #[derive(Clone, Copy, Debug)]
  #[repr(transparent)]
  pub(super) struct Address(NonZeroUsize);

  #[derive(Debug)]
  pub(super) struct ContextScope {
    entered_context: NonNull<Context>,
  }

  impl ContextScope {
    pub fn new(context: Local<Context>) -> Self {
      unsafe { v8__Context__Enter(&*context) };
      Self {
        entered_context: context.as_non_null(),
      }
    }
  }

  impl Drop for ContextScope {
    #[inline(always)]
    fn drop(&mut self) {
      unsafe { v8__Context__Exit(self.entered_context.as_ptr()) };
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(super) struct HandleScope([MaybeUninit<usize>; 3]);

  impl HandleScope {
    /// Creates an uninitialized `HandleScope`.
    ///
    /// This function is marked unsafe because the caller must ensure that the
    /// returned value isn't dropped before `init()` has been called.
    pub unsafe fn uninit() -> Self {
      Self(unsafe { MaybeUninit::uninit().assume_init() })
    }

    /// This function is marked unsafe because `init()` must be called exactly
    /// once, no more and no less, after creating a `HandleScope` value with
    /// `HandleScope::uninit()`.
    pub unsafe fn init(&mut self, isolate: NonNull<Isolate>) {
      let buf = NonNull::from(self).cast();
      unsafe {
        v8__HandleScope__CONSTRUCT(buf.as_ptr(), isolate.as_ptr());
      }
    }
  }

  impl Drop for HandleScope {
    #[inline(always)]
    fn drop(&mut self) {
      unsafe { v8__HandleScope__DESTRUCT(self) };
    }
  }

  #[repr(transparent)]
  #[derive(Debug)]
  pub(super) struct EscapeSlot(NonNull<raw::Address>);

  impl EscapeSlot {
    pub fn new(isolate: NonNull<Isolate>) -> Self {
      unsafe {
        let undefined = raw::v8__Undefined(isolate.as_ptr()) as *const _;
        let local = raw::v8__Local__New(isolate.as_ptr(), undefined);
        let slot_address_ptr = local as *const Address as *mut _;
        let slot_address_nn = NonNull::new_unchecked(slot_address_ptr);
        Self(slot_address_nn)
      }
    }

    pub fn escape<'e, T>(self, value: Local<'_, T>) -> Local<'e, T>
    where
      for<'l> Local<'l, T>: Into<Local<'l, Data>>,
    {
      assert_eq!(Layout::new::<Self>(), Layout::new::<Local<T>>());
      unsafe {
        let undefined = Local::<Value>::from_non_null(self.0.cast());
        debug_assert!(undefined.is_undefined());
        let value_address = *(&*value as *const T as *const Address);
        ptr::write(self.0.as_ptr(), value_address);
        Local::from_non_null(self.0.cast())
      }
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(super) struct TryCatch([MaybeUninit<usize>; 6]);

  impl TryCatch {
    /// Creates an uninitialized `TryCatch`.
    ///
    /// This function is marked unsafe because the caller must ensure that the
    /// returned value isn't dropped before `init()` has been called.
    pub unsafe fn uninit() -> Self {
      Self(unsafe { MaybeUninit::uninit().assume_init() })
    }

    /// This function is marked unsafe because `init()` must be called exactly
    /// once, no more and no less, after creating a `TryCatch` value with
    /// `TryCatch::uninit()`.
    pub unsafe fn init(&mut self, isolate: NonNull<Isolate>) {
      let buf = NonNull::from(self).cast();
      unsafe {
        v8__TryCatch__CONSTRUCT(buf.as_ptr(), isolate.as_ptr());
      }
    }
  }

  impl Drop for TryCatch {
    #[inline(always)]
    fn drop(&mut self) {
      unsafe { v8__TryCatch__DESTRUCT(self) };
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(super) struct DisallowJavascriptExecutionScope([MaybeUninit<usize>; 2]);

  impl DisallowJavascriptExecutionScope {
    /// Creates an uninitialized `DisallowJavascriptExecutionScope`.
    ///
    /// This function is marked unsafe because the caller must ensure that the
    /// returned value isn't dropped before `init()` has been called.
    pub unsafe fn uninit() -> Self {
      Self(unsafe { MaybeUninit::uninit().assume_init() })
    }

    /// This function is marked unsafe because `init()` must be called exactly
    /// once, no more and no less, after creating a
    /// `DisallowJavascriptExecutionScope` value with
    /// `DisallowJavascriptExecutionScope::uninit()`.
    pub unsafe fn init(
      &mut self,
      isolate: NonNull<Isolate>,
      on_failure: OnFailure,
    ) {
      let buf = NonNull::from(self).cast();
      unsafe {
        v8__DisallowJavascriptExecutionScope__CONSTRUCT(
          buf.as_ptr(),
          isolate.as_ptr(),
          on_failure,
        );
      }
    }
  }

  impl Drop for DisallowJavascriptExecutionScope {
    #[inline(always)]
    fn drop(&mut self) {
      unsafe { v8__DisallowJavascriptExecutionScope__DESTRUCT(self) };
    }
  }

  #[repr(C)]
  #[derive(Debug)]
  pub(super) struct AllowJavascriptExecutionScope([MaybeUninit<usize>; 2]);

  impl AllowJavascriptExecutionScope {
    /// Creates an uninitialized `AllowJavascriptExecutionScope`.
    ///
    /// This function is marked unsafe because the caller must ensure that the
    /// returned value isn't dropped before `init()` has been called.
    pub unsafe fn uninit() -> Self {
      Self(unsafe { MaybeUninit::uninit().assume_init() })
    }

    /// This function is marked unsafe because `init()` must be called exactly
    /// once, no more and no less, after creating an
    /// `AllowJavascriptExecutionScope` value with
    /// `AllowJavascriptExecutionScope::uninit()`.
    pub unsafe fn init(&mut self, isolate: NonNull<Isolate>) {
      unsafe {
        let buf = NonNull::from(self).cast();
        v8__AllowJavascriptExecutionScope__CONSTRUCT(
          buf.as_ptr(),
          isolate.as_ptr(),
        );
      }
    }
  }

  impl Drop for AllowJavascriptExecutionScope {
    #[inline(always)]
    fn drop(&mut self) {
      unsafe { v8__AllowJavascriptExecutionScope__DESTRUCT(self) };
    }
  }

  unsafe extern "C" {
    pub(super) fn v8__Isolate__GetCurrentContext(
      isolate: *mut Isolate,
    ) -> *const Context;
    pub(super) fn v8__Isolate__GetEnteredOrMicrotaskContext(
      isolate: *mut Isolate,
    ) -> *const Context;
    pub(super) fn v8__Isolate__ThrowException(
      isolate: *mut Isolate,
      exception: *const Value,
    ) -> *const Value;
    pub(super) fn v8__Isolate__GetDataFromSnapshotOnce(
      this: *mut Isolate,
      index: usize,
    ) -> *const Data;
    pub(super) fn v8__Isolate__GetCurrentHostDefinedOptions(
      this: *mut Isolate,
    ) -> *const Data;

    pub(super) fn v8__Context__EQ(
      this: *const Context,
      other: *const Context,
    ) -> bool;
    pub(super) fn v8__Context__Enter(this: *const Context);
    pub(super) fn v8__Context__Exit(this: *const Context);
    pub(super) fn v8__Context__GetIsolate(this: *const Context)
    -> *mut Isolate;
    pub(super) fn v8__Context__GetDataFromSnapshotOnce(
      this: *const Context,
      index: usize,
    ) -> *const Data;
    pub(super) fn v8__Context__SetPromiseHooks(
      this: *const Context,
      init_hook: *const Function,
      before_hook: *const Function,
      after_hook: *const Function,
      resolve_hook: *const Function,
    );
    pub(super) fn v8__Context__SetContinuationPreservedEmbedderData(
      this: *mut Isolate,
      value: *const Value,
    );
    pub(super) fn v8__Context__GetContinuationPreservedEmbedderData(
      this: *mut Isolate,
    ) -> *const Value;

    pub(super) fn v8__HandleScope__CONSTRUCT(
      buf: *mut MaybeUninit<HandleScope>,
      isolate: *mut Isolate,
    );
    pub(super) fn v8__HandleScope__DESTRUCT(this: *mut HandleScope);

    pub(super) fn v8__Local__New(
      isolate: *mut Isolate,
      other: *const Data,
    ) -> *const Data;
    pub(super) fn v8__Undefined(isolate: *mut Isolate) -> *const Primitive;

    pub(super) fn v8__TryCatch__CONSTRUCT(
      buf: *mut MaybeUninit<TryCatch>,
      isolate: *mut Isolate,
    );
    pub(super) fn v8__TryCatch__DESTRUCT(this: *mut TryCatch);
    pub(super) fn v8__TryCatch__HasCaught(this: *const TryCatch) -> bool;
    pub(super) fn v8__TryCatch__CanContinue(this: *const TryCatch) -> bool;
    pub(super) fn v8__TryCatch__HasTerminated(this: *const TryCatch) -> bool;
    pub(super) fn v8__TryCatch__IsVerbose(this: *const TryCatch) -> bool;
    pub(super) fn v8__TryCatch__SetVerbose(this: *mut TryCatch, value: bool);
    pub(super) fn v8__TryCatch__SetCaptureMessage(
      this: *mut TryCatch,
      value: bool,
    );
    pub(super) fn v8__TryCatch__Reset(this: *mut TryCatch);
    pub(super) fn v8__TryCatch__Exception(
      this: *const TryCatch,
    ) -> *const Value;
    pub(super) fn v8__TryCatch__StackTrace(
      this: *const TryCatch,
      context: *const Context,
    ) -> *const Value;
    pub(super) fn v8__TryCatch__Message(
      this: *const TryCatch,
    ) -> *const Message;
    pub(super) fn v8__TryCatch__ReThrow(this: *mut TryCatch) -> *const Value;

    pub(super) fn v8__DisallowJavascriptExecutionScope__CONSTRUCT(
      buf: *mut MaybeUninit<DisallowJavascriptExecutionScope>,
      isolate: *mut Isolate,
      on_failure: OnFailure,
    );
    pub(super) fn v8__DisallowJavascriptExecutionScope__DESTRUCT(
      this: *mut DisallowJavascriptExecutionScope,
    );

    pub(super) fn v8__AllowJavascriptExecutionScope__CONSTRUCT(
      buf: *mut MaybeUninit<AllowJavascriptExecutionScope>,
      isolate: *mut Isolate,
    );
    pub(super) fn v8__AllowJavascriptExecutionScope__DESTRUCT(
      this: *mut AllowJavascriptExecutionScope,
    );

    pub(super) fn v8__Message__GetIsolate(this: *const Message)
    -> *mut Isolate;
    pub(super) fn v8__Object__GetIsolate(this: *const Object) -> *mut Isolate;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ContextOptions;
  use crate::Global;
  use std::any::type_name;

  trait SameType {}
  impl<A> SameType for (A, A) {}

  /// `AssertTypeOf` facilitates comparing types. The important difference with
  /// assigning a value to a variable with an explicitly stated type is that the
  /// latter allows coercions and dereferencing to change the type, whereas
  /// `AssertTypeOf` requires the compared types to match exactly.
  struct AssertTypeOf<'a, T>(#[allow(dead_code)] &'a T);
  impl<T> AssertTypeOf<'_, T> {
    pub fn is<A>(self)
    where
      (A, T): SameType,
    {
      assert_eq!(type_name::<A>(), type_name::<T>());
    }
  }

  #[test]
  fn deref_types() {
    crate::initialize_v8();
    let isolate = &mut Isolate::new(Default::default());
    AssertTypeOf(isolate).is::<OwnedIsolate>();
    let l1_hs = &mut HandleScope::new(isolate);
    AssertTypeOf(l1_hs).is::<HandleScope<()>>();
    let context = Context::new(l1_hs, ContextOptions::default());
    {
      let l2_cxs = &mut ContextScope::new(l1_hs, context);
      AssertTypeOf(l2_cxs).is::<ContextScope<HandleScope>>();
      {
        let d = l2_cxs.deref_mut();
        AssertTypeOf(d).is::<HandleScope>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<HandleScope<()>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
      }
      {
        let l3_tc = &mut TryCatch::new(l2_cxs);
        AssertTypeOf(l3_tc).is::<TryCatch<HandleScope>>();
        let d = l3_tc.deref_mut();
        AssertTypeOf(d).is::<HandleScope>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<HandleScope<()>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
      }
      {
        let l3_djses = &mut DisallowJavascriptExecutionScope::new(
          l2_cxs,
          OnFailure::CrashOnFailure,
        );
        AssertTypeOf(l3_djses)
          .is::<DisallowJavascriptExecutionScope<HandleScope>>();
        let d = l3_djses.deref_mut();
        AssertTypeOf(d).is::<HandleScope>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<HandleScope<()>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
        {
          let l4_ajses = &mut AllowJavascriptExecutionScope::new(l3_djses);
          AssertTypeOf(l4_ajses).is::<HandleScope>();
          let d = l4_ajses.deref_mut();
          AssertTypeOf(d).is::<HandleScope<()>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
      }
      {
        let l3_ehs = &mut EscapableHandleScope::new(l2_cxs);
        AssertTypeOf(l3_ehs).is::<EscapableHandleScope>();
        {
          let l4_cxs = &mut ContextScope::new(l3_ehs, context);
          AssertTypeOf(l4_cxs).is::<ContextScope<EscapableHandleScope>>();
          let d = l4_cxs.deref_mut();
          AssertTypeOf(d).is::<EscapableHandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope<()>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
        {
          let l4_tc = &mut TryCatch::new(l3_ehs);
          AssertTypeOf(l4_tc).is::<TryCatch<EscapableHandleScope>>();
          let d = l4_tc.deref_mut();
          AssertTypeOf(d).is::<EscapableHandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope<()>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
        {
          let l4_djses = &mut DisallowJavascriptExecutionScope::new(
            l3_ehs,
            OnFailure::CrashOnFailure,
          );
          AssertTypeOf(l4_djses)
            .is::<DisallowJavascriptExecutionScope<EscapableHandleScope>>();
          let d = l4_djses.deref_mut();
          AssertTypeOf(d).is::<EscapableHandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<HandleScope<()>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
          {
            let l5_ajses = &mut AllowJavascriptExecutionScope::new(l4_djses);
            AssertTypeOf(l5_ajses).is::<EscapableHandleScope>();
            let d = l5_ajses.deref_mut();
            AssertTypeOf(d).is::<HandleScope>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<HandleScope<()>>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<Isolate>();
          }
        }
      }
    }
    {
      let l2_tc = &mut TryCatch::new(l1_hs);
      AssertTypeOf(l2_tc).is::<TryCatch<HandleScope<()>>>();
      let d = l2_tc.deref_mut();
      AssertTypeOf(d).is::<HandleScope<()>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
      {
        let l3_djses = &mut DisallowJavascriptExecutionScope::new(
          l2_tc,
          OnFailure::CrashOnFailure,
        );
        AssertTypeOf(l3_djses)
          .is::<DisallowJavascriptExecutionScope<TryCatch<HandleScope<()>>>>();
        let d = l3_djses.deref_mut();
        AssertTypeOf(d).is::<TryCatch<HandleScope<()>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<HandleScope<()>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
        {
          let l4_ajses = &mut AllowJavascriptExecutionScope::new(l3_djses);
          AssertTypeOf(l4_ajses).is::<TryCatch<HandleScope<()>>>();
          let d = l4_ajses.deref_mut();
          AssertTypeOf(d).is::<HandleScope<()>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
      }
    }
    {
      let l2_ehs = &mut EscapableHandleScope::new(l1_hs);
      AssertTypeOf(l2_ehs).is::<EscapableHandleScope<()>>();
      let l3_tc = &mut TryCatch::new(l2_ehs);
      AssertTypeOf(l3_tc).is::<TryCatch<EscapableHandleScope<()>>>();
      let d = l3_tc.deref_mut();
      AssertTypeOf(d).is::<EscapableHandleScope<()>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<HandleScope<()>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
    {
      // `CallbackScope` is meant to be used inside V8 API callback functions
      // only. It assumes that a `HandleScope` already exists on the stack, and
      // that a context has been entered. Push a `ContextScope` onto the stack
      // to also meet the second expectation.
      let _ = ContextScope::new(l1_hs, context);
      let l2_cbs = &mut unsafe { CallbackScope::new(context) };
      AssertTypeOf(l2_cbs).is::<CallbackScope>();
      let d = l2_cbs.deref_mut();
      AssertTypeOf(d).is::<HandleScope>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<HandleScope<()>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
    {
      let isolate: &mut Isolate = l1_hs.as_mut();
      let l2_cbs = &mut unsafe { CallbackScope::new(isolate) };
      AssertTypeOf(l2_cbs).is::<CallbackScope<()>>();
      let d = l2_cbs.deref_mut();
      AssertTypeOf(d).is::<HandleScope<()>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
  }

  #[test]
  fn new_scope_types() {
    crate::initialize_v8();
    let isolate = &mut Isolate::new(Default::default());
    AssertTypeOf(isolate).is::<OwnedIsolate>();
    let global_context: Global<Context>;
    {
      let l1_hs = &mut HandleScope::new(isolate);
      AssertTypeOf(l1_hs).is::<HandleScope<()>>();
      let context = Context::new(l1_hs, Default::default());
      global_context = Global::new(l1_hs, context);
      AssertTypeOf(&HandleScope::new(l1_hs)).is::<HandleScope<()>>();
      {
        let l2_cxs = &mut ContextScope::new(l1_hs, context);
        AssertTypeOf(l2_cxs).is::<ContextScope<HandleScope>>();
        AssertTypeOf(&ContextScope::new(l2_cxs, context))
          .is::<ContextScope<HandleScope>>();
        AssertTypeOf(&HandleScope::new(l2_cxs)).is::<HandleScope>();
        AssertTypeOf(&EscapableHandleScope::new(l2_cxs))
          .is::<EscapableHandleScope>();
        AssertTypeOf(&TryCatch::new(l2_cxs)).is::<TryCatch<HandleScope>>();
      }
      {
        let l2_ehs = &mut EscapableHandleScope::new(l1_hs);
        AssertTypeOf(l2_ehs).is::<EscapableHandleScope<()>>();
        AssertTypeOf(&HandleScope::new(l2_ehs))
          .is::<EscapableHandleScope<()>>();
        AssertTypeOf(&EscapableHandleScope::new(l2_ehs))
          .is::<EscapableHandleScope<()>>();
        {
          let l3_cxs = &mut ContextScope::new(l2_ehs, context);
          AssertTypeOf(l3_cxs).is::<ContextScope<EscapableHandleScope>>();
          AssertTypeOf(&ContextScope::new(l3_cxs, context))
            .is::<ContextScope<EscapableHandleScope>>();
          AssertTypeOf(&HandleScope::new(l3_cxs)).is::<EscapableHandleScope>();
          AssertTypeOf(&EscapableHandleScope::new(l3_cxs))
            .is::<EscapableHandleScope>();
          {
            let l4_tc = &mut TryCatch::new(l3_cxs);
            AssertTypeOf(l4_tc).is::<TryCatch<EscapableHandleScope>>();
            AssertTypeOf(&ContextScope::new(l4_tc, context))
              .is::<ContextScope<EscapableHandleScope>>();
            AssertTypeOf(&HandleScope::new(l4_tc)).is::<EscapableHandleScope>();
            AssertTypeOf(&EscapableHandleScope::new(l4_tc))
              .is::<EscapableHandleScope>();
            AssertTypeOf(&TryCatch::new(l4_tc))
              .is::<TryCatch<EscapableHandleScope>>();
          }
        }
        {
          let l3_tc = &mut TryCatch::new(l2_ehs);
          AssertTypeOf(l3_tc).is::<TryCatch<EscapableHandleScope<()>>>();
          AssertTypeOf(&ContextScope::new(l3_tc, context))
            .is::<ContextScope<EscapableHandleScope>>();
          AssertTypeOf(&HandleScope::new(l3_tc))
            .is::<EscapableHandleScope<()>>();
          AssertTypeOf(&EscapableHandleScope::new(l3_tc))
            .is::<EscapableHandleScope<()>>();
          AssertTypeOf(&TryCatch::new(l3_tc))
            .is::<TryCatch<EscapableHandleScope<()>>>();
        }
      }
      {
        let l2_tc = &mut TryCatch::new(l1_hs);
        AssertTypeOf(l2_tc).is::<TryCatch<HandleScope<()>>>();
        AssertTypeOf(&ContextScope::new(l2_tc, context))
          .is::<ContextScope<HandleScope>>();
        AssertTypeOf(&HandleScope::new(l2_tc)).is::<HandleScope<()>>();
        AssertTypeOf(&EscapableHandleScope::new(l2_tc))
          .is::<EscapableHandleScope<()>>();
        AssertTypeOf(&TryCatch::new(l2_tc)).is::<TryCatch<HandleScope<()>>>();
      }
      {
        let l2_cbs = &mut unsafe { CallbackScope::new(context) };
        AssertTypeOf(l2_cbs).is::<CallbackScope>();
        AssertTypeOf(&ContextScope::new(l2_cbs, context))
          .is::<ContextScope<HandleScope>>();
        {
          let l3_hs = &mut HandleScope::new(l2_cbs);
          AssertTypeOf(l3_hs).is::<HandleScope>();
          AssertTypeOf(&ContextScope::new(l3_hs, context))
            .is::<ContextScope<HandleScope>>();
          AssertTypeOf(&HandleScope::new(l3_hs)).is::<HandleScope>();
          AssertTypeOf(&EscapableHandleScope::new(l3_hs))
            .is::<EscapableHandleScope>();
          AssertTypeOf(&TryCatch::new(l3_hs)).is::<TryCatch<HandleScope>>();
        }
        {
          let l3_ehs = &mut EscapableHandleScope::new(l2_cbs);
          AssertTypeOf(l3_ehs).is::<EscapableHandleScope>();
          AssertTypeOf(&ContextScope::new(l3_ehs, context))
            .is::<ContextScope<EscapableHandleScope>>();
          AssertTypeOf(&HandleScope::new(l3_ehs)).is::<EscapableHandleScope>();
          AssertTypeOf(&EscapableHandleScope::new(l3_ehs))
            .is::<EscapableHandleScope>();
          AssertTypeOf(&TryCatch::new(l3_ehs))
            .is::<TryCatch<EscapableHandleScope>>();
        }
        {
          let l3_tc = &mut TryCatch::new(l2_cbs);
          AssertTypeOf(l3_tc).is::<TryCatch<HandleScope>>();
          AssertTypeOf(&ContextScope::new(l3_tc, context))
            .is::<ContextScope<HandleScope>>();
          AssertTypeOf(&HandleScope::new(l3_tc)).is::<HandleScope>();
          AssertTypeOf(&EscapableHandleScope::new(l3_tc))
            .is::<EscapableHandleScope>();
          AssertTypeOf(&TryCatch::new(l3_tc)).is::<TryCatch<HandleScope>>();
        }
      }
    }
    {
      let l1_cbs = &mut unsafe { CallbackScope::new(&mut *isolate) };
      AssertTypeOf(l1_cbs).is::<CallbackScope<()>>();
      let context = Context::new(l1_cbs, Default::default());
      AssertTypeOf(&ContextScope::new(l1_cbs, context))
        .is::<ContextScope<HandleScope>>();
      AssertTypeOf(&HandleScope::new(l1_cbs)).is::<HandleScope<()>>();
      AssertTypeOf(&EscapableHandleScope::new(l1_cbs))
        .is::<EscapableHandleScope<()>>();
      AssertTypeOf(&TryCatch::new(l1_cbs)).is::<TryCatch<HandleScope<()>>>();
    }
    {
      AssertTypeOf(&HandleScope::with_context(isolate, &global_context))
        .is::<HandleScope>();
      AssertTypeOf(&HandleScope::with_context(isolate, global_context))
        .is::<HandleScope>();
    }
  }
}
