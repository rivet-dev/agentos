use std::convert::TryFrom;
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::ptr::null;

use crate::Array;
use crate::Boolean;
use crate::CallbackScope;
use crate::Context;
use crate::Function;
use crate::Integer;
use crate::Isolate;
use crate::Local;
use crate::Name;
use crate::Object;
use crate::PropertyDescriptor;
use crate::ScriptOrigin;
use crate::SealedLocal;
use crate::Signature;
use crate::String;
use crate::UniqueRef;
use crate::UnsafeRawIsolatePtr;
use crate::Value;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;
use crate::scope::callback_scope;
use crate::script_compiler::CachedData;
use crate::support::MapFnFrom;
use crate::support::MapFnTo;
use crate::support::ToCFn;
use crate::support::UnitType;
use crate::support::{Opaque, int};
use crate::template::Intercepted;

unsafe extern "C" {
  fn v8__Function__New(
    context: *const Context,
    callback: FunctionCallback,
    data_or_null: *const Value,
    length: i32,
    constructor_behavior: ConstructorBehavior,
    side_effect_type: SideEffectType,
  ) -> *const Function;
  fn v8__Function__Call(
    this: *const Function,
    context: *const Context,
    recv: *const Value,
    argc: int,
    argv: *const *const Value,
  ) -> *const Value;
  fn v8__Function__NewInstance(
    this: *const Function,
    context: *const Context,
    argc: int,
    argv: *const *const Value,
  ) -> *const Object;
  fn v8__Function__GetName(this: *const Function) -> *const String;
  fn v8__Function__SetName(this: *const Function, name: *const String);
  fn v8__Function__GetScriptColumnNumber(this: *const Function) -> int;
  fn v8__Function__GetScriptLineNumber(this: *const Function) -> int;
  fn v8__Function__ScriptId(this: *const Function) -> int;
  fn v8__Function__GetScriptOrigin<'a>(
    this: *const Function,
    out: *mut MaybeUninit<ScriptOrigin<'a>>,
  );

  fn v8__Function__CreateCodeCache(
    script: *const Function,
  ) -> *mut CachedData<'static>;

  fn v8__FunctionCallbackInfo__GetIsolate(
    this: *const FunctionCallbackInfo,
  ) -> *mut RealIsolate;
  fn v8__FunctionCallbackInfo__GetParts(
    this: *const FunctionCallbackInfo,
  ) -> RawFunctionCallbackInfoParts;
  fn v8__FunctionCallbackInfo__Data(
    this: *const FunctionCallbackInfo,
  ) -> *const Value;
  fn v8__FunctionCallbackInfo__This(
    this: *const FunctionCallbackInfo,
  ) -> *const Object;
  fn v8__FunctionCallbackInfo__NewTarget(
    this: *const FunctionCallbackInfo,
  ) -> *const Value;
  fn v8__FunctionCallbackInfo__IsConstructCall(
    this: *const FunctionCallbackInfo,
  ) -> bool;
  fn v8__FunctionCallbackInfo__Get(
    this: *const FunctionCallbackInfo,
    index: int,
  ) -> *const Value;
  fn v8__FunctionCallbackInfo__Length(this: *const FunctionCallbackInfo)
  -> int;
  fn v8__FunctionCallbackInfo__GetReturnValue(
    this: *const FunctionCallbackInfo,
  ) -> usize;

  fn v8__PropertyCallbackInfo__GetIsolate(
    this: *const RawPropertyCallbackInfo,
  ) -> *mut RealIsolate;
  fn v8__PropertyCallbackInfo__Data(
    this: *const RawPropertyCallbackInfo,
  ) -> *const Value;
  fn v8__PropertyCallbackInfo__Holder(
    this: *const RawPropertyCallbackInfo,
  ) -> *const Object;
  fn v8__PropertyCallbackInfo__GetReturnValue(
    this: *const RawPropertyCallbackInfo,
  ) -> usize;
  fn v8__PropertyCallbackInfo__ShouldThrowOnError(
    this: *const RawPropertyCallbackInfo,
  ) -> bool;

  fn v8__ReturnValue__Value__Set(
    this: *mut RawReturnValue,
    value: *const Value,
  );
  fn v8__ReturnValue__Value__Set__Bool(this: *mut RawReturnValue, value: bool);
  fn v8__ReturnValue__Value__Set__Int32(this: *mut RawReturnValue, value: i32);
  fn v8__ReturnValue__Value__Set__Uint32(this: *mut RawReturnValue, value: u32);
  fn v8__ReturnValue__Value__Set__Double(this: *mut RawReturnValue, value: f64);
  fn v8__ReturnValue__Value__SetNull(this: *mut RawReturnValue);
  fn v8__ReturnValue__Value__SetUndefined(this: *mut RawReturnValue);
  fn v8__ReturnValue__Value__SetEmptyString(this: *mut RawReturnValue);
  fn v8__ReturnValue__Value__Get(this: *const RawReturnValue) -> *const Value;
}

// Ad-libbed - V8 does not document ConstructorBehavior.
/// ConstructorBehavior::Allow creates a regular API function.
///
/// ConstructorBehavior::Throw creates a "concise" API function, a function
/// without a ".prototype" property, that is somewhat faster to create and has
/// a smaller footprint. Functionally equivalent to ConstructorBehavior::Allow
/// followed by a call to FunctionTemplate::RemovePrototype().
#[repr(C)]
pub enum ConstructorBehavior {
  Throw,
  Allow,
}

/// Options for marking whether callbacks may trigger JS-observable side
/// effects. Side-effect-free callbacks are allowlisted during debug evaluation
/// with throwOnSideEffect. It applies when calling a Function,
/// FunctionTemplate, or an Accessor callback. For Interceptors, please see
/// PropertyHandlerFlags's kHasNoSideEffect.
/// Callbacks that only cause side effects to the receiver are allowlisted if
/// invoked on receiver objects that are created within the same debug-evaluate
/// call, as these objects are temporary and the side effect does not escape.
#[repr(C)]
pub enum SideEffectType {
  HasSideEffect,
  HasNoSideEffect,
  HasSideEffectToReceiver,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RawReturnValue(usize);

#[repr(C)]
#[derive(Debug)]
struct RawFunctionCallbackInfoParts {
  isolate: *mut RealIsolate,
  return_value: usize,
  data: *const Value,
  length: int,
}

// Note: the 'cb lifetime is required because the ReturnValue object must not
// outlive the FunctionCallbackInfo/PropertyCallbackInfo object from which it
// is derived.
#[derive(Debug)]
pub struct ReturnValue<'cb, T = Value>(RawReturnValue, PhantomData<&'cb T>);

impl<T> Clone for ReturnValue<'_, T> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<T> Copy for ReturnValue<'_, T> {}

impl<'cb, T> ReturnValue<'cb, T> {
  #[inline(always)]
  fn from_raw(raw: RawReturnValue) -> Self {
    Self(raw, PhantomData)
  }

  #[inline(always)]
  pub fn from_property_callback_info(
    info: &'cb PropertyCallbackInfo<T>,
  ) -> Self {
    Self(
      unsafe {
        RawReturnValue(v8__PropertyCallbackInfo__GetReturnValue(&info.0))
      },
      PhantomData,
    )
  }
}

impl<'cb> ReturnValue<'cb, Value> {
  #[inline(always)]
  pub fn from_function_callback_info(info: &'cb FunctionCallbackInfo) -> Self {
    Self(
      unsafe { RawReturnValue(v8__FunctionCallbackInfo__GetReturnValue(info)) },
      PhantomData,
    )
  }
}

impl ReturnValue<'_, ()> {
  #[inline(always)]
  pub fn set_bool(&mut self, value: bool) {
    unsafe { v8__ReturnValue__Value__Set__Bool(&mut self.0, value) }
  }
}

impl<T> ReturnValue<'_, T>
where
  for<'s> Local<'s, T>: Into<Local<'s, Value>>,
{
  #[inline(always)]
  pub fn set(&mut self, value: Local<T>) {
    unsafe { v8__ReturnValue__Value__Set(&mut self.0, &*value.into()) }
  }

  #[inline(always)]
  pub fn set_bool(&mut self, value: bool) {
    unsafe { v8__ReturnValue__Value__Set__Bool(&mut self.0, value) }
  }

  #[inline(always)]
  pub fn set_int32(&mut self, value: i32) {
    unsafe { v8__ReturnValue__Value__Set__Int32(&mut self.0, value) }
  }

  #[inline(always)]
  pub fn set_uint32(&mut self, value: u32) {
    unsafe { v8__ReturnValue__Value__Set__Uint32(&mut self.0, value) }
  }

  #[inline(always)]
  pub fn set_double(&mut self, value: f64) {
    unsafe { v8__ReturnValue__Value__Set__Double(&mut self.0, value) }
  }

  #[inline(always)]
  pub fn set_null(&mut self) {
    unsafe { v8__ReturnValue__Value__SetNull(&mut self.0) }
  }

  #[inline(always)]
  pub fn set_undefined(&mut self) {
    unsafe { v8__ReturnValue__Value__SetUndefined(&mut self.0) }
  }

  #[inline(always)]
  pub fn set_empty_string(&mut self) {
    unsafe { v8__ReturnValue__Value__SetEmptyString(&mut self.0) }
  }

  /// Getter. Creates a new Local<> so it comes with a certain performance
  /// hit. If the ReturnValue was not yet set, this will return the undefined
  /// value.
  #[inline(always)]
  pub fn get<'s>(&self, scope: &PinScope<'s, '_>) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__ReturnValue__Value__Get(&self.0)) }
      .unwrap()
  }
}

/// The argument information given to function call callbacks.  This
/// class provides access to information about the context of the call,
/// including the receiver, the number and values of arguments, and
/// the holder of the function.
#[repr(C)]
#[derive(Debug)]
pub struct FunctionCallbackInfo(*mut Opaque);

/// Values commonly needed at the start of a function callback.
///
/// This lets raw callbacks fetch the isolate, return value, callback data, and
/// argument length with one FFI call. The returned parts can also seed
/// [`FunctionCallbackArguments`] and [`CallbackScope`](crate::CallbackScope)
/// without re-reading those values from V8.
#[repr(C)]
#[derive(Debug)]
pub struct FunctionCallbackInfoParts<'cb> {
  pub isolate: UnsafeRawIsolatePtr,
  pub return_value: ReturnValue<'cb>,
  pub data: Local<'cb, Value>,
  pub length: int,
}

impl FunctionCallbackInfo {
  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut RealIsolate {
    unsafe { v8__FunctionCallbackInfo__GetIsolate(self) }
  }

  /// Returns the common callback entry values with a single C++ shim call.
  #[inline(always)]
  pub fn get_parts(&self) -> FunctionCallbackInfoParts<'_> {
    let raw = unsafe { v8__FunctionCallbackInfo__GetParts(self) };
    FunctionCallbackInfoParts {
      isolate: UnsafeRawIsolatePtr::from_real_ptr(raw.isolate),
      return_value: ReturnValue::from_raw(RawReturnValue(raw.return_value)),
      data: unsafe {
        Local::from_non_null(NonNull::new_unchecked(raw.data as *mut Value))
      },
      length: raw.length,
    }
  }

  #[inline(always)]
  pub(crate) fn new_target(&self) -> Local<'_, Value> {
    unsafe {
      let ptr = v8__FunctionCallbackInfo__NewTarget(self);
      let nn = NonNull::new_unchecked(ptr as *mut _);
      Local::from_non_null(nn)
    }
  }

  #[inline(always)]
  pub(crate) fn this(&self) -> Local<'_, Object> {
    unsafe {
      let ptr = v8__FunctionCallbackInfo__This(self);
      let nn = NonNull::new_unchecked(ptr as *mut _);
      Local::from_non_null(nn)
    }
  }

  #[inline]
  pub fn is_construct_call(&self) -> bool {
    unsafe { v8__FunctionCallbackInfo__IsConstructCall(self) }
  }

  #[inline(always)]
  pub(crate) fn data(&self) -> Local<'_, Value> {
    unsafe {
      let ptr = v8__FunctionCallbackInfo__Data(self);
      let nn = NonNull::new_unchecked(ptr as *mut Value);
      Local::from_non_null(nn)
    }
  }

  #[inline(always)]
  pub(crate) fn length(&self) -> int {
    unsafe { v8__FunctionCallbackInfo__Length(self) }
  }

  #[inline(always)]
  pub(crate) fn get(&self, index: int) -> Local<'_, Value> {
    unsafe {
      let ptr = v8__FunctionCallbackInfo__Get(self, index);
      let nn = NonNull::new_unchecked(ptr as *mut Value);
      Local::from_non_null(nn)
    }
  }
}

#[repr(C)]
#[derive(Debug)]
struct RawPropertyCallbackInfo(*mut Opaque);

/// The information passed to a property callback about the context
/// of the property access.
#[repr(C)]
#[derive(Debug)]
pub struct PropertyCallbackInfo<T>(RawPropertyCallbackInfo, PhantomData<T>);

impl<T> PropertyCallbackInfo<T> {
  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut RealIsolate {
    unsafe { v8__PropertyCallbackInfo__GetIsolate(&self.0) }
  }
}

#[derive(Debug)]
pub struct FunctionCallbackArguments<'s> {
  info: &'s FunctionCallbackInfo,
  data: Option<Local<'s, Value>>,
  length: Option<int>,
}

impl<'s> FunctionCallbackArguments<'s> {
  #[inline(always)]
  pub fn from_function_callback_info(info: &'s FunctionCallbackInfo) -> Self {
    Self {
      info,
      data: None,
      length: None,
    }
  }

  /// Creates callback arguments from [`FunctionCallbackInfoParts`], reusing the
  /// already-loaded callback data and argument length.
  #[inline(always)]
  pub fn from_function_callback_info_parts(
    info: &'s FunctionCallbackInfo,
    parts: &FunctionCallbackInfoParts<'s>,
  ) -> Self {
    Self {
      info,
      data: Some(parts.data),
      length: Some(parts.length),
    }
  }

  /// SAFETY: caller must guarantee that no other references to the isolate are
  /// accessible. Specifically, if an open CallbackScope or HandleScope exists
  /// in the current function, `FunctionCallbackArguments::get_isolate()` should
  /// not be called.
  #[inline(always)]
  pub unsafe fn get_isolate(&mut self) -> &mut Isolate {
    unsafe {
      &mut *(self.info.get_isolate_ptr() as *mut crate::isolate::Isolate)
    }
  }

  /// For construct calls, this returns the "new.target" value.
  #[inline(always)]
  pub fn new_target(&self) -> Local<'s, Value> {
    self.info.new_target()
  }

  /// Returns true if this is a construct call, i.e., if the function was
  /// called with the `new` operator.
  #[inline]
  pub fn is_construct_call(&self) -> bool {
    self.info.is_construct_call()
  }

  /// Returns the receiver. This corresponds to the "this" value.
  #[inline(always)]
  pub fn this(&self) -> Local<'s, Object> {
    self.info.this()
  }

  /// Returns the data argument specified when creating the callback.
  #[inline(always)]
  pub fn data(&self) -> Local<'s, Value> {
    self.data.unwrap_or_else(|| self.info.data())
  }

  /// The number of available arguments.
  #[inline(always)]
  pub fn length(&self) -> int {
    self.length.unwrap_or_else(|| self.info.length())
  }

  /// Accessor for the available arguments. Returns `undefined` if the index is
  /// out of bounds.
  #[inline(always)]
  pub fn get(&self, i: int) -> Local<'s, Value> {
    self.info.get(i)
  }
}

#[derive(Debug)]
pub struct PropertyCallbackArguments<'s>(&'s RawPropertyCallbackInfo);

impl<'s> PropertyCallbackArguments<'s> {
  #[inline(always)]
  pub(crate) fn from_property_callback_info<T>(
    info: &'s PropertyCallbackInfo<T>,
  ) -> Self {
    Self(&info.0)
  }

  /// Returns the object in the prototype chain of the receiver that has the
  /// interceptor. Suppose you have `x` and its prototype is `y`, and `y`
  /// has an interceptor. Then `info.This()` is `x` and `info.Holder()` is `y`.
  /// In case the property is installed on the global object the Holder()
  /// would return the global proxy.
  #[inline(always)]
  pub fn holder(&self) -> Local<'s, Object> {
    unsafe {
      Local::from_raw(v8__PropertyCallbackInfo__Holder(self.0))
        .unwrap_unchecked()
    }
  }

  /// Returns the data set in the configuration, i.e., in
  /// `NamedPropertyHandlerConfiguration` or
  /// `IndexedPropertyHandlerConfiguration.`
  #[inline(always)]
  pub fn data(&self) -> Local<'s, Value> {
    unsafe {
      Local::from_raw(v8__PropertyCallbackInfo__Data(self.0)).unwrap_unchecked()
    }
  }

  /// Returns `true` if the intercepted function should throw if an error
  /// occurs. Usually, `true` corresponds to `'use strict'`.
  ///
  /// Always `false` when intercepting `Reflect.set()` independent of the
  /// language mode.
  #[inline(always)]
  pub fn should_throw_on_error(&self) -> bool {
    unsafe { v8__PropertyCallbackInfo__ShouldThrowOnError(self.0) }
  }
}

pub type FunctionCallback = unsafe extern "C" fn(*const FunctionCallbackInfo);

impl<F> MapFnFrom<F> for FunctionCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      FunctionCallbackArguments<'s>,
      ReturnValue<'s, Value>,
    ),
{
  fn mapping() -> Self {
    let f = |info: *const FunctionCallbackInfo| {
      let info = unsafe { &*info };
      let scope = std::pin::pin!(unsafe { CallbackScope::new(info) });
      let mut scope = scope.init();
      let args = FunctionCallbackArguments::from_function_callback_info(info);
      let rv = ReturnValue::from_function_callback_info(info);
      (F::get())(&mut scope, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedGetterCallbackForAccessor =
  unsafe extern "C" fn(SealedLocal<Name>, *const PropertyCallbackInfo<Value>);

impl<F> MapFnFrom<F> for NamedGetterCallbackForAccessor
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ),
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedGetterCallback = unsafe extern "C" fn(
  SealedLocal<Name>,
  *const PropertyCallbackInfo<Value>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedGetterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedQueryCallback = unsafe extern "C" fn(
  SealedLocal<Name>,
  *const PropertyCallbackInfo<Integer>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedQueryCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Integer>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             info: *const PropertyCallbackInfo<Integer>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedSetterCallbackForAccessor = unsafe extern "C" fn(
  SealedLocal<Name>,
  SealedLocal<Value>,
  *const PropertyCallbackInfo<()>,
);

impl<F> MapFnFrom<F> for NamedSetterCallbackForAccessor
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ),
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             value: SealedLocal<Value>,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let value = unsafe { scope.unseal(value) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, value, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedSetterCallback = unsafe extern "C" fn(
  SealedLocal<Name>,
  SealedLocal<Value>,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedSetterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             value: SealedLocal<Value>,
             info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let value = unsafe { scope.unseal(value) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, value, args, rv)
    };
    f.to_c_fn()
  }
}

// Should return an Array in Return Value
pub(crate) type PropertyEnumeratorCallback =
  unsafe extern "C" fn(*const PropertyCallbackInfo<Array>);

impl<F> MapFnFrom<F> for PropertyEnumeratorCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Array>,
    ),
{
  fn mapping() -> Self {
    let f = |info: *const PropertyCallbackInfo<Array>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedDefinerCallback = unsafe extern "C" fn(
  SealedLocal<Name>,
  *const PropertyDescriptor,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedDefinerCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      &PropertyDescriptor,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             desc: *const PropertyDescriptor,
             info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let desc = unsafe { &*desc };
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, desc, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedDeleterCallback = unsafe extern "C" fn(
  SealedLocal<Name>,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedDeleterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: SealedLocal<Name>,
             info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let key = unsafe { scope.unseal(key) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedGetterCallback =
  unsafe extern "C" fn(u32, *const PropertyCallbackInfo<Value>) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedGetterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32, info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedQueryCallback = unsafe extern "C" fn(
  u32,
  *const PropertyCallbackInfo<Integer>,
) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedQueryCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Integer>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: u32, info: *const PropertyCallbackInfo<Integer>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedSetterCallback = unsafe extern "C" fn(
  u32,
  SealedLocal<Value>,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedSetterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      u32,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32,
             value: SealedLocal<Value>,
             info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let value = unsafe { scope.unseal(value) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, value, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedDefinerCallback = unsafe extern "C" fn(
  u32,
  *const PropertyDescriptor,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedDefinerCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      u32,
      &PropertyDescriptor,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32,
             desc: *const PropertyDescriptor,
             info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      let desc = unsafe { &*desc };
      (F::get())(scope, index, desc, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedDeleterCallback = unsafe extern "C" fn(
  u32,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedDeleterCallback
where
  F: UnitType
    + for<'s, 'i> Fn(
      &mut PinScope<'s, 'i>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32, info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      callback_scope!(unsafe scope, info);
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, args, rv)
    };
    f.to_c_fn()
  }
}

/// A builder to construct the properties of a Function or FunctionTemplate.
pub struct FunctionBuilder<'s, T> {
  pub(crate) callback: Option<FunctionCallback>,
  pub(crate) data: Option<Local<'s, Value>>,
  pub(crate) signature: Option<Local<'s, Signature>>,
  pub(crate) length: i32,
  pub(crate) constructor_behavior: ConstructorBehavior,
  pub(crate) side_effect_type: SideEffectType,
  phantom: PhantomData<T>,
}

impl<'s, T> FunctionBuilder<'s, T> {
  /// Create a new FunctionBuilder.
  #[inline(always)]
  pub fn new(callback: impl MapFnTo<FunctionCallback>) -> Self {
    Self::new_raw(callback.map_fn_to())
  }

  #[inline(always)]
  pub fn new_raw(callback: FunctionCallback) -> Self {
    Self::new_raw_optional(Some(callback))
  }

  /// Create a new FunctionBuilder, optionally without a callback. V8 only
  /// accepts a missing callback when building a `FunctionTemplate`.
  #[inline(always)]
  pub(crate) fn new_raw_optional(callback: Option<FunctionCallback>) -> Self {
    Self {
      callback,
      data: None,
      signature: None,
      length: 0,
      constructor_behavior: ConstructorBehavior::Allow,
      side_effect_type: SideEffectType::HasSideEffect,
      phantom: PhantomData,
    }
  }

  /// Set the associated data. The default is no associated data.
  #[inline(always)]
  pub fn data(mut self, data: Local<'s, Value>) -> Self {
    self.data = Some(data);
    self
  }

  /// Set the function length. The default is 0.
  #[inline(always)]
  pub fn length(mut self, length: i32) -> Self {
    self.length = length;
    self
  }

  /// Set the constructor behavior. The default is ConstructorBehavior::Allow.
  #[inline(always)]
  pub fn constructor_behavior(
    mut self,
    constructor_behavior: ConstructorBehavior,
  ) -> Self {
    self.constructor_behavior = constructor_behavior;
    self
  }

  /// Set the side effect type. The default is SideEffectType::HasSideEffect.
  #[inline(always)]
  pub fn side_effect_type(mut self, side_effect_type: SideEffectType) -> Self {
    self.side_effect_type = side_effect_type;
    self
  }
}

impl<'s> FunctionBuilder<'s, Function> {
  /// Create the function in the current execution context.
  #[inline(always)]
  pub fn build<'i>(
    self,
    scope: &PinScope<'s, 'i>,
  ) -> Option<Local<'s, Function>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Function__New(
          sd.get_current_context(),
          self.callback.unwrap(),
          self.data.map_or_else(null, |p| &*p),
          self.length,
          self.constructor_behavior,
          self.side_effect_type,
        )
      })
    }
  }
}

impl Function {
  /// Create a FunctionBuilder to configure a Function.
  /// This is the same as FunctionBuilder::<Function>::new().
  #[inline(always)]
  pub fn builder<'s>(
    callback: impl MapFnTo<FunctionCallback>,
  ) -> FunctionBuilder<'s, Self> {
    FunctionBuilder::new(callback)
  }

  #[inline(always)]
  pub fn builder_raw<'s>(
    callback: FunctionCallback,
  ) -> FunctionBuilder<'s, Self> {
    FunctionBuilder::new_raw(callback)
  }

  /// Create a function in the current execution context
  /// for a given FunctionCallback.
  #[inline(always)]
  pub fn new<'s>(
    scope: &mut PinScope<'s, '_>,
    callback: impl MapFnTo<FunctionCallback>,
  ) -> Option<Local<'s, Function>> {
    Self::builder(callback).build(scope)
  }

  #[inline(always)]
  pub fn new_raw<'s>(
    scope: &mut PinScope<'s, '_>,
    callback: FunctionCallback,
  ) -> Option<Local<'s, Function>> {
    Self::builder_raw(callback).build(scope)
  }

  /// Call a function in a context scope.
  #[inline]
  pub fn call<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    recv: Local<Value>,
    args: &[Local<Value>],
  ) -> Option<Local<'s, Value>> {
    let args = Local::slice_into_raw(args);
    let argc = int::try_from(args.len()).unwrap();
    let argv = args.as_ptr();
    unsafe {
      scope.cast_local(|sd| {
        v8__Function__Call(self, sd.get_current_context(), &*recv, argc, argv)
      })
    }
  }

  /// Call a function in a given context.
  #[inline]
  pub fn call_with_context<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
    context: Local<Context>,
    recv: Local<Value>,
    args: &[Local<Value>],
  ) -> Option<Local<'s, Value>> {
    let args = Local::slice_into_raw(args);
    let argc = int::try_from(args.len()).unwrap();
    let argv = args.as_ptr();
    unsafe {
      let ret = v8__Function__Call(
        self,
        context.as_non_null().as_ptr(),
        &*recv,
        argc,
        argv,
      );
      if ret.is_null() {
        None
      } else {
        scope.cast_local(|_| ret)
      }
    }
  }

  #[inline(always)]
  pub fn new_instance<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    args: &[Local<Value>],
  ) -> Option<Local<'s, Object>> {
    let args = Local::slice_into_raw(args);
    let argc = int::try_from(args.len()).unwrap();
    let argv = args.as_ptr();
    unsafe {
      scope.cast_local(|sd| {
        v8__Function__NewInstance(self, sd.get_current_context(), argc, argv)
      })
    }
  }

  #[inline(always)]
  pub fn get_name<'s>(&self, scope: &PinScope<'s, '_>) -> Local<'s, String> {
    unsafe { scope.cast_local(|_| v8__Function__GetName(self)).unwrap() }
  }

  #[inline(always)]
  pub fn set_name(&self, name: Local<String>) {
    unsafe { v8__Function__SetName(self, &*name) }
  }

  /// Get the (zero-indexed) column number of the function's definition, if available.
  #[inline(always)]
  pub fn get_script_column_number(&self) -> Option<u32> {
    let ret = unsafe { v8__Function__GetScriptColumnNumber(self) };
    (ret >= 0).then_some(ret as u32)
  }

  /// Get the (zero-indexed) line number of the function's definition, if available.
  #[inline(always)]
  pub fn get_script_line_number(&self) -> Option<u32> {
    let ret = unsafe { v8__Function__GetScriptLineNumber(self) };
    (ret >= 0).then_some(ret as u32)
  }

  #[inline(always)]
  pub fn get_script_origin<'s>(
    &self,
    _scope: &PinScope<'s, '_>,
  ) -> ScriptOrigin<'s> {
    unsafe {
      let mut script_origin: MaybeUninit<ScriptOrigin<'_>> =
        MaybeUninit::uninit();
      v8__Function__GetScriptOrigin(self, &mut script_origin);
      script_origin.assume_init()
    }
  }

  /// Returns scriptId.
  #[inline(always)]
  pub fn script_id(&self) -> i32 {
    unsafe { v8__Function__ScriptId(self) }
  }

  /// Creates and returns code cache for the specified unbound_script.
  /// This will return nullptr if the script cannot be serialized. The
  /// CachedData returned by this function should be owned by the caller.
  #[inline(always)]
  pub fn create_code_cache(&self) -> Option<UniqueRef<CachedData<'static>>> {
    let code_cache =
      unsafe { UniqueRef::try_from_raw(v8__Function__CreateCodeCache(self)) };
    if let Some(code_cache) = &code_cache {
      debug_assert_eq!(
        code_cache.buffer_policy(),
        crate::script_compiler::BufferPolicy::BufferOwned
      );
    }
    code_cache
  }
}
