use std::convert::TryFrom;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::ptr::null;

use crate::Array;
use crate::Boolean;
use crate::Context;
use crate::Function;
use crate::HandleScope;
use crate::Integer;
use crate::Isolate;
use crate::Local;
use crate::Name;
use crate::Object;
use crate::PropertyDescriptor;
use crate::Signature;
use crate::String;
use crate::UniqueRef;
use crate::Value;
use crate::scope::CallbackScope;
use crate::script_compiler::CachedData;
use crate::support::MapFnFrom;
use crate::support::MapFnTo;
use crate::support::ToCFn;
use crate::support::UnitType;
use crate::support::{Opaque, int};
use crate::template::Intercepted;
use crate::{ScriptOrigin, undefined};

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
  fn v8__Function__GetScriptOrigin(
    this: *const Function,
  ) -> *const ScriptOrigin<'static>;

  fn v8__Function__CreateCodeCache(
    script: *const Function,
  ) -> *mut CachedData<'static>;

  static v8__FunctionCallbackInfo__kArgsLength: int;

  fn v8__FunctionCallbackInfo__Data(
    this: *const FunctionCallbackInfo,
  ) -> *const Value;

  fn v8__PropertyCallbackInfo__GetIsolate(
    this: *const RawPropertyCallbackInfo,
  ) -> *mut Isolate;
  fn v8__PropertyCallbackInfo__Data(
    this: *const RawPropertyCallbackInfo,
  ) -> *const Value;
  fn v8__PropertyCallbackInfo__This(
    this: *const RawPropertyCallbackInfo,
  ) -> *const Object;
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
#[derive(Debug)]
struct RawReturnValue(usize);

// Note: the 'cb lifetime is required because the ReturnValue object must not
// outlive the FunctionCallbackInfo/PropertyCallbackInfo object from which it
// is derived.
#[derive(Debug)]
pub struct ReturnValue<'cb, T = Value>(RawReturnValue, PhantomData<&'cb T>);

impl<'cb, T> ReturnValue<'cb, T> {
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
    let nn = info.get_return_value_non_null();
    Self(RawReturnValue(nn.as_ptr() as _), PhantomData)
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
  pub fn get<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, Value> {
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
pub struct FunctionCallbackInfo {
  // The layout of this struct must match that of `class FunctionCallbackInfo`
  // as defined in v8.h.
  implicit_args: *mut *const Opaque,
  values: *mut *const Opaque,
  length: int,
}

// These constants must match those defined on `class FunctionCallbackInfo` in
// v8-function-callback.h.
#[allow(dead_code, non_upper_case_globals)]
impl FunctionCallbackInfo {
  const kHolderIndex: i32 = 0;
  const kIsolateIndex: i32 = 1;
  const kContextIndex: i32 = 2;
  const kReturnValueIndex: i32 = 3;
  const kTargetIndex: i32 = 4;
  const kNewTargetIndex: i32 = 5;
  const kArgsLength: i32 = 6;
}

impl FunctionCallbackInfo {
  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut Isolate {
    let arg_nn =
      self.get_implicit_arg_non_null::<*mut Isolate>(Self::kIsolateIndex);
    *unsafe { arg_nn.as_ref() }
  }

  #[inline(always)]
  pub(crate) fn get_return_value_non_null(&self) -> NonNull<Value> {
    self.get_implicit_arg_non_null::<Value>(Self::kReturnValueIndex)
  }

  #[inline(always)]
  pub(crate) fn new_target(&self) -> Local<Value> {
    unsafe { self.get_implicit_arg_local(Self::kNewTargetIndex) }
  }

  #[inline(always)]
  pub(crate) fn this(&self) -> Local<Object> {
    unsafe { self.get_arg_local(-1) }
  }

  #[inline(always)]
  pub(crate) fn data(&self) -> Local<Value> {
    unsafe {
      let ptr = v8__FunctionCallbackInfo__Data(self);
      let nn = NonNull::new_unchecked(ptr as *mut Value);
      Local::from_non_null(nn)
    }
  }

  #[inline(always)]
  pub(crate) fn length(&self) -> i32 {
    self.length
  }

  #[inline(always)]
  pub(crate) fn get(&self, index: int) -> Local<Value> {
    if index >= 0 && index < self.length {
      unsafe { self.get_arg_local(index) }
    } else {
      let isolate = unsafe { &mut *self.get_isolate_ptr() };
      undefined(isolate).into()
    }
  }

  #[inline(always)]
  fn get_implicit_arg_non_null<T>(&self, index: i32) -> NonNull<T> {
    // In debug builds, check that `FunctionCallbackInfo::kArgsLength` matches
    // the C++ definition. Unfortunately we can't check the other constants
    // because they are declared protected in the C++ header.
    debug_assert_eq!(
      unsafe { v8__FunctionCallbackInfo__kArgsLength },
      Self::kArgsLength
    );
    // Assert that `index` is in bounds.
    assert!(index >= 0);
    assert!(index < Self::kArgsLength);
    // Compute the address of the implicit argument and cast to `NonNull<T>`.
    let ptr = unsafe { self.implicit_args.offset(index as isize) as *mut T };
    debug_assert!(!ptr.is_null());
    unsafe { NonNull::new_unchecked(ptr) }
  }

  // SAFETY: caller must guarantee that the implicit argument at `index`
  // contains a valid V8 handle.
  #[inline(always)]
  unsafe fn get_implicit_arg_local<T>(&self, index: i32) -> Local<T> {
    let nn = self.get_implicit_arg_non_null::<T>(index);
    unsafe { Local::from_non_null(nn) }
  }

  // SAFETY: caller must guarantee that the `index` value lies between -1 and
  // self.length.
  #[inline(always)]
  unsafe fn get_arg_local<T>(&self, index: i32) -> Local<T> {
    let ptr = unsafe { self.values.offset(index as _) } as *mut T;
    debug_assert!(!ptr.is_null());
    let nn = unsafe { NonNull::new_unchecked(ptr) };
    unsafe { Local::from_non_null(nn) }
  }
}

#[repr(C)]
#[derive(Debug)]
struct RawPropertyCallbackInfo(Opaque);

/// The information passed to a property callback about the context
/// of the property access.
#[repr(C)]
#[derive(Debug)]
pub struct PropertyCallbackInfo<T>(RawPropertyCallbackInfo, PhantomData<T>);

impl<T> PropertyCallbackInfo<T> {
  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut Isolate {
    unsafe { v8__PropertyCallbackInfo__GetIsolate(&self.0) }
  }
}

#[derive(Debug)]
pub struct FunctionCallbackArguments<'s>(&'s FunctionCallbackInfo);

impl<'s> FunctionCallbackArguments<'s> {
  #[inline(always)]
  pub fn from_function_callback_info(info: &'s FunctionCallbackInfo) -> Self {
    Self(info)
  }

  /// SAFETY: caller must guarantee that no other references to the isolate are
  /// accessible. Specifically, if an open CallbackScope or HandleScope exists
  /// in the current function, `FunctionCallbackArguments::get_isolate()` should
  /// not be called.
  #[inline(always)]
  pub unsafe fn get_isolate(&mut self) -> &mut Isolate {
    unsafe { &mut *self.0.get_isolate_ptr() }
  }

  /// For construct calls, this returns the "new.target" value.
  #[inline(always)]
  pub fn new_target(&self) -> Local<'s, Value> {
    self.0.new_target()
  }

  /// Returns the receiver. This corresponds to the "this" value.
  #[inline(always)]
  pub fn this(&self) -> Local<'s, Object> {
    self.0.this()
  }

  /// Returns the data argument specified when creating the callback.
  #[inline(always)]
  pub fn data(&self) -> Local<'s, Value> {
    self.0.data()
  }

  /// The number of available arguments.
  #[inline(always)]
  pub fn length(&self) -> int {
    self.0.length()
  }

  /// Accessor for the available arguments. Returns `undefined` if the index is
  /// out of bounds.
  #[inline(always)]
  pub fn get(&self, i: int) -> Local<'s, Value> {
    self.0.get(i)
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

  /// Returns the receiver. In many cases, this is the object on which the
  /// property access was intercepted. When using
  /// `Reflect.get`, `Function.prototype.call`, or similar functions, it is the
  /// object passed in as receiver or thisArg.
  ///
  /// ```c++
  ///   void GetterCallback(Local<Name> name,
  ///                       const v8::PropertyCallbackInfo<v8::Value>& info) {
  ///      auto context = info.GetIsolate()->GetCurrentContext();
  ///
  ///      v8::Local<v8::Value> a_this =
  ///          info.This()
  ///              ->GetRealNamedProperty(context, v8_str("a"))
  ///              .ToLocalChecked();
  ///      v8::Local<v8::Value> a_holder =
  ///          info.Holder()
  ///              ->GetRealNamedProperty(context, v8_str("a"))
  ///              .ToLocalChecked();
  ///
  ///     CHECK(v8_str("r")->Equals(context, a_this).FromJust());
  ///     CHECK(v8_str("obj")->Equals(context, a_holder).FromJust());
  ///
  ///     info.GetReturnValue().Set(name);
  ///   }
  ///
  ///   v8::Local<v8::FunctionTemplate> templ =
  ///   v8::FunctionTemplate::New(isolate);
  ///   templ->InstanceTemplate()->SetHandler(
  ///       v8::NamedPropertyHandlerConfiguration(GetterCallback));
  ///   LocalContext env;
  ///   env->Global()
  ///       ->Set(env.local(), v8_str("obj"), templ->GetFunction(env.local())
  ///                                            .ToLocalChecked()
  ///                                            ->NewInstance(env.local())
  ///                                            .ToLocalChecked())
  ///       .FromJust();
  ///
  ///   CompileRun("obj.a = 'obj'; var r = {a: 'r'}; Reflect.get(obj, 'x', r)");
  /// ```
  #[inline(always)]
  pub fn this(&self) -> Local<'s, Object> {
    unsafe {
      Local::from_raw(v8__PropertyCallbackInfo__This(self.0)).unwrap_unchecked()
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
    + for<'s> Fn(
      &mut HandleScope<'s>,
      FunctionCallbackArguments<'s>,
      ReturnValue<Value>,
    ),
{
  fn mapping() -> Self {
    let f = |info: *const FunctionCallbackInfo| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = FunctionCallbackArguments::from_function_callback_info(info);
      let rv = ReturnValue::from_function_callback_info(info);
      (F::get())(scope, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedGetterCallbackForAccessor<'s> =
  unsafe extern "C" fn(Local<'s, Name>, *const PropertyCallbackInfo<Value>);

impl<F> MapFnFrom<F> for NamedGetterCallbackForAccessor<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ),
{
  fn mapping() -> Self {
    let f = |key: Local<Name>, info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedGetterCallback<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  *const PropertyCallbackInfo<Value>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedGetterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: Local<Name>, info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedQueryCallback<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  *const PropertyCallbackInfo<Integer>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedQueryCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Integer>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: Local<Name>, info: *const PropertyCallbackInfo<Integer>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedSetterCallbackForAccessor<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  Local<'s, Value>,
  *const PropertyCallbackInfo<()>,
);

impl<F> MapFnFrom<F> for NamedSetterCallbackForAccessor<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ),
{
  fn mapping() -> Self {
    let f = |key: Local<Name>,
             value: Local<Value>,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, value, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedSetterCallback<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  Local<'s, Value>,
  *const PropertyCallbackInfo<()>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedSetterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: Local<Name>,
             value: Local<Value>,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, value, args, rv)
    };
    f.to_c_fn()
  }
}

// Should return an Array in Return Value
pub(crate) type PropertyEnumeratorCallback<'s> =
  unsafe extern "C" fn(*const PropertyCallbackInfo<Array>);

impl<F> MapFnFrom<F> for PropertyEnumeratorCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Array>,
    ),
{
  fn mapping() -> Self {
    let f = |info: *const PropertyCallbackInfo<Array>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, args, rv);
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedDefinerCallback<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  *const PropertyDescriptor,
  *const PropertyCallbackInfo<()>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedDefinerCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      &PropertyDescriptor,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: Local<Name>,
             desc: *const PropertyDescriptor,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let desc = unsafe { &*desc };
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, desc, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type NamedDeleterCallback<'s> = unsafe extern "C" fn(
  Local<'s, Name>,
  *const PropertyCallbackInfo<Boolean>,
) -> Intercepted;

impl<F> MapFnFrom<F> for NamedDeleterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      Local<'s, Name>,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: Local<Name>, info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedGetterCallback<'s> =
  unsafe extern "C" fn(u32, *const PropertyCallbackInfo<Value>) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedGetterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Value>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32, info: *const PropertyCallbackInfo<Value>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedQueryCallback<'s> = unsafe extern "C" fn(
  u32,
  *const PropertyCallbackInfo<Integer>,
) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedQueryCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Integer>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |key: u32, info: *const PropertyCallbackInfo<Integer>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, key, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedSetterCallback<'s> = unsafe extern "C" fn(
  u32,
  Local<'s, Value>,
  *const PropertyCallbackInfo<()>,
)
  -> Intercepted;

impl<F> MapFnFrom<F> for IndexedSetterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      u32,
      Local<'s, Value>,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32,
             value: Local<Value>,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, value, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedDefinerCallback<'s> =
  unsafe extern "C" fn(
    u32,
    *const PropertyDescriptor,
    *const PropertyCallbackInfo<()>,
  ) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedDefinerCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      u32,
      &PropertyDescriptor,
      PropertyCallbackArguments<'s>,
      ReturnValue<()>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32,
             desc: *const PropertyDescriptor,
             info: *const PropertyCallbackInfo<()>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      let desc = unsafe { &*desc };
      (F::get())(scope, index, desc, args, rv)
    };
    f.to_c_fn()
  }
}

pub(crate) type IndexedDeleterCallback<'s> =
  unsafe extern "C" fn(
    u32,
    *const PropertyCallbackInfo<Boolean>,
  ) -> Intercepted;

impl<F> MapFnFrom<F> for IndexedDeleterCallback<'_>
where
  F: UnitType
    + for<'s> Fn(
      &mut HandleScope<'s>,
      u32,
      PropertyCallbackArguments<'s>,
      ReturnValue<Boolean>,
    ) -> Intercepted,
{
  fn mapping() -> Self {
    let f = |index: u32, info: *const PropertyCallbackInfo<Boolean>| {
      let info = unsafe { &*info };
      let scope = &mut unsafe { CallbackScope::new(info) };
      let args = PropertyCallbackArguments::from_property_callback_info(info);
      let rv = ReturnValue::from_property_callback_info(info);
      (F::get())(scope, index, args, rv)
    };
    f.to_c_fn()
  }
}

/// A builder to construct the properties of a Function or FunctionTemplate.
pub struct FunctionBuilder<'s, T> {
  pub(crate) callback: FunctionCallback,
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
  pub fn build(
    self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Function>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Function__New(
          sd.get_current_context(),
          self.callback,
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
    scope: &mut HandleScope<'s>,
    callback: impl MapFnTo<FunctionCallback>,
  ) -> Option<Local<'s, Function>> {
    Self::builder(callback).build(scope)
  }

  #[inline(always)]
  pub fn new_raw<'s>(
    scope: &mut HandleScope<'s>,
    callback: FunctionCallback,
  ) -> Option<Local<'s, Function>> {
    Self::builder_raw(callback).build(scope)
  }

  /// Call a function in a context scope.
  #[inline]
  pub fn call<'s>(
    &self,
    scope: &mut HandleScope<'s>,
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
    scope: &mut HandleScope<'s, ()>,
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
    scope: &mut HandleScope<'s>,
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
  pub fn get_name<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, String> {
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
  pub fn get_script_origin(&self) -> &ScriptOrigin {
    unsafe {
      let ptr = v8__Function__GetScriptOrigin(self);
      &*ptr
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
