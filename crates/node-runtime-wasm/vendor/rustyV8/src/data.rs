// Copyright 2019-2022 the Deno authors. All rights reserved. MIT license.

use std::any::type_name;
use std::convert::From;
use std::convert::TryFrom;
use std::error::Error;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::hash::Hash;
use std::hash::Hasher;
use std::mem::transmute;
use std::ops::Deref;

use crate::Local;
use crate::support::Opaque;

unsafe extern "C" {
  fn v8__Data__EQ(this: *const Data, other: *const Data) -> bool;
  fn v8__Data__IsBigInt(this: *const Data) -> bool;
  fn v8__Data__IsBoolean(this: *const Data) -> bool;
  fn v8__Data__IsContext(this: *const Data) -> bool;
  fn v8__Data__IsFixedArray(this: *const Data) -> bool;
  fn v8__Data__IsFunctionTemplate(this: *const Data) -> bool;
  fn v8__Data__IsModule(this: *const Data) -> bool;
  fn v8__Data__IsModuleRequest(this: *const Data) -> bool;
  fn v8__Data__IsName(this: *const Data) -> bool;
  fn v8__Data__IsNumber(this: *const Data) -> bool;
  fn v8__Data__IsObjectTemplate(this: *const Data) -> bool;
  fn v8__Data__IsPrimitive(this: *const Data) -> bool;
  fn v8__Data__IsPrivate(this: *const Data) -> bool;
  fn v8__Data__IsString(this: *const Data) -> bool;
  fn v8__Data__IsSymbol(this: *const Data) -> bool;
  fn v8__Data__IsValue(this: *const Data) -> bool;

  fn v8__Value__SameValue(this: *const Value, other: *const Value) -> bool;
  fn v8__Value__StrictEquals(this: *const Value, other: *const Value) -> bool;
}

impl Data {
  /// Returns true if this data is a `BigInt`.
  #[inline(always)]
  pub fn is_big_int(&self) -> bool {
    unsafe { v8__Data__IsBigInt(self) }
  }

  /// Returns true if this data is a `Boolean`.
  #[inline(always)]
  pub fn is_boolean(&self) -> bool {
    unsafe { v8__Data__IsBoolean(self) }
  }

  /// Returns true if this data is a `Context`.
  #[inline(always)]
  pub fn is_context(&self) -> bool {
    unsafe { v8__Data__IsContext(self) }
  }

  /// Returns true if this data is a `FixedArray`.
  #[inline(always)]
  pub fn is_fixed_array(&self) -> bool {
    unsafe { v8__Data__IsFixedArray(self) }
  }

  /// Returns true if this data is a `FunctionTemplate`.
  #[inline(always)]
  pub fn is_function_template(&self) -> bool {
    unsafe { v8__Data__IsFunctionTemplate(self) }
  }

  /// Returns true if this data is a `Module`.
  #[inline(always)]
  pub fn is_module(&self) -> bool {
    unsafe { v8__Data__IsModule(self) }
  }

  /// Returns true if this data is a `ModuleRequest`.
  #[inline(always)]
  pub fn is_module_request(&self) -> bool {
    unsafe { v8__Data__IsModuleRequest(self) }
  }

  /// Returns true if this data is a `Name`.
  #[inline(always)]
  pub fn is_name(&self) -> bool {
    unsafe { v8__Data__IsName(self) }
  }

  /// Returns true if this data is a `Number`.
  #[inline(always)]
  pub fn is_number(&self) -> bool {
    unsafe { v8__Data__IsNumber(self) }
  }

  /// Returns true if this data is a `ObjectTemplate`.
  #[inline(always)]
  pub fn is_object_template(&self) -> bool {
    unsafe { v8__Data__IsObjectTemplate(self) }
  }

  /// Returns true if this data is a `Primitive`.
  #[inline(always)]
  pub fn is_primitive(&self) -> bool {
    unsafe { v8__Data__IsPrimitive(self) }
  }

  /// Returns true if this data is a `Private`.
  #[inline(always)]
  pub fn is_private(&self) -> bool {
    unsafe { v8__Data__IsPrivate(self) }
  }

  /// Returns true if this data is a `String`.
  #[inline(always)]
  pub fn is_string(&self) -> bool {
    unsafe { v8__Data__IsString(self) }
  }

  /// Returns true if this data is a `Symbol`.
  #[inline(always)]
  pub fn is_symbol(&self) -> bool {
    unsafe { v8__Data__IsSymbol(self) }
  }

  /// Returns true if this data is a `Value`.
  #[inline(always)]
  pub fn is_value(&self) -> bool {
    unsafe { v8__Data__IsValue(self) }
  }
}

macro_rules! impl_deref {
  { $target:ident for $type:ident } => {
    impl Deref for $type {
      type Target = $target;
      fn deref(&self) -> &Self::Target {
        unsafe { &*(self as *const _ as *const Self::Target) }
      }
    }
  };
}

macro_rules! impl_from {
  { $source:ident for $type:ident } => {
    impl<'s> From<Local<'s, $source>> for Local<'s, $type> {
      fn from(l: Local<'s, $source>) -> Self {
        unsafe { transmute(l) }
      }
    }
  };
}

macro_rules! impl_try_from {
  { $source:ident for $target:ident if $value:pat => $check:expr } => {
    impl<'s> TryFrom<Local<'s, $source>> for Local<'s, $target> {
      type Error = DataError;
      fn try_from(l: Local<'s, $source>) -> Result<Self, Self::Error> {
        // Not dead: `cast()` is sometimes used in the $check expression.
        #[allow(dead_code)]
        fn cast<T>(l: Local<$source>) -> Local<T> {
          unsafe { transmute::<Local<$source>, Local<T>>(l) }
        }
        match l {
          $value if $check => Ok(unsafe {
            transmute::<Local<'s, $source>, Local<'s, $target>>(l)
          }),
          _ => Err(DataError::bad_type::<$target, $source>())
        }
      }
    }
  };
}

macro_rules! impl_eq {
  { for $type:ident } => {
    impl Eq for $type {}
  };
}

macro_rules! impl_hash {
  { for $type:ident use $method:ident } => {
    impl Hash for $type {
      fn hash<H: Hasher>(&self, state: &mut H) {
        self.$method().hash(state)
      }
    }
  };
}

macro_rules! impl_partial_eq {
  { $rhs:ident for $type:ident use identity } => {
    impl<'s> PartialEq<$rhs> for $type {
      fn eq(&self, other: &$rhs) -> bool {
        let a = self as *const _ as *const Data;
        let b = other as *const _ as *const Data;
        unsafe { v8__Data__EQ(a, b) }
      }
    }
  };
  { $rhs:ident for $type:ident use strict_equals } => {
    impl<'s> PartialEq<$rhs> for $type {
      fn eq(&self, other: &$rhs) -> bool {
        let a = self as *const _ as *const Value;
        let b = other as *const _ as *const Value;
        unsafe { v8__Value__StrictEquals(a, b) }
      }
    }
  };

  { $rhs:ident for $type:ident use same_value_zero } => {
    impl<'s> PartialEq<$rhs> for $type {
      fn eq(&self, other: &$rhs) -> bool {
        let a = self as *const _ as *const Value;
        let b = other as *const _ as *const Value;
        unsafe {
          v8__Value__SameValue(a, b) || {
            let zero = &*Local::<Value>::from(Integer::zero());
            v8__Value__StrictEquals(a, zero) && v8__Value__StrictEquals(b, zero)
          }
        }
      }
    }
  };
}

#[derive(Clone, Copy, Debug)]
pub enum DataError {
  BadType {
    actual: &'static str,
    expected: &'static str,
  },
  NoData {
    expected: &'static str,
  },
}

impl DataError {
  pub(crate) fn bad_type<E: 'static, A: 'static>() -> Self {
    Self::BadType {
      expected: type_name::<E>(),
      actual: type_name::<A>(),
    }
  }

  pub(crate) fn no_data<E: 'static>() -> Self {
    Self::NoData {
      expected: type_name::<E>(),
    }
  }
}

impl Display for DataError {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    match self {
      Self::BadType { expected, actual } => {
        write!(f, "expected type `{expected}`, got `{actual}`")
      }
      Self::NoData { expected } => {
        write!(f, "expected `Some({expected})`, found `None`")
      }
    }
  }
}

impl Error for DataError {}

/// The superclass of objects that can reside on V8's heap.
#[repr(C)]
#[derive(Debug)]
pub struct Data(Opaque);

impl_from! { AccessorSignature for Data }
impl_from! { Context for Data }
impl_from! { FixedArray for Data }
impl_from! { Module for Data }
impl_from! { ModuleRequest for Data }
impl_from! { PrimitiveArray for Data }
impl_from! { Private for Data }
impl_from! { Signature for Data }
impl_from! { Template for Data }
impl_from! { FunctionTemplate for Data }
impl_from! { ObjectTemplate for Data }
impl_from! { UnboundModuleScript for Data }
impl_from! { Value for Data }
impl_from! { External for Data }
impl_from! { Object for Data }
impl_from! { Array for Data }
impl_from! { ArrayBuffer for Data }
impl_from! { ArrayBufferView for Data }
impl_from! { DataView for Data }
impl_from! { TypedArray for Data }
impl_from! { BigInt64Array for Data }
impl_from! { BigUint64Array for Data }
impl_from! { Float32Array for Data }
impl_from! { Float64Array for Data }
impl_from! { Int16Array for Data }
impl_from! { Int32Array for Data }
impl_from! { Int8Array for Data }
impl_from! { Uint16Array for Data }
impl_from! { Uint32Array for Data }
impl_from! { Uint8Array for Data }
impl_from! { Uint8ClampedArray for Data }
impl_from! { BigIntObject for Data }
impl_from! { BooleanObject for Data }
impl_from! { Date for Data }
impl_from! { Function for Data }
impl_from! { Map for Data }
impl_from! { NumberObject for Data }
impl_from! { Promise for Data }
impl_from! { PromiseResolver for Data }
impl_from! { Proxy for Data }
impl_from! { RegExp for Data }
impl_from! { Set for Data }
impl_from! { SharedArrayBuffer for Data }
impl_from! { StringObject for Data }
impl_from! { SymbolObject for Data }
impl_from! { WasmMemoryObject for Data }
impl_from! { WasmModuleObject for Data }
impl_from! { Primitive for Data }
impl_from! { BigInt for Data }
impl_from! { Boolean for Data }
impl_from! { Name for Data }
impl_from! { String for Data }
impl_from! { Symbol for Data }
impl_from! { Number for Data }
impl_from! { Integer for Data }
impl_from! { Int32 for Data }
impl_from! { Uint32 for Data }
impl_partial_eq! { AccessorSignature for Data use identity }
impl_partial_eq! { Context for Data use identity }
impl_partial_eq! { FixedArray for Data use identity }
impl_partial_eq! { Module for Data use identity }
impl_partial_eq! { ModuleRequest for Data use identity }
impl_partial_eq! { PrimitiveArray for Data use identity }
impl_partial_eq! { Private for Data use identity }
impl_partial_eq! { Signature for Data use identity }
impl_partial_eq! { Template for Data use identity }
impl_partial_eq! { FunctionTemplate for Data use identity }
impl_partial_eq! { ObjectTemplate for Data use identity }
impl_partial_eq! { UnboundModuleScript for Data use identity }
impl_partial_eq! { External for Data use identity }
impl_partial_eq! { Object for Data use identity }
impl_partial_eq! { Array for Data use identity }
impl_partial_eq! { ArrayBuffer for Data use identity }
impl_partial_eq! { ArrayBufferView for Data use identity }
impl_partial_eq! { DataView for Data use identity }
impl_partial_eq! { TypedArray for Data use identity }
impl_partial_eq! { BigInt64Array for Data use identity }
impl_partial_eq! { BigUint64Array for Data use identity }
impl_partial_eq! { Float32Array for Data use identity }
impl_partial_eq! { Float64Array for Data use identity }
impl_partial_eq! { Int16Array for Data use identity }
impl_partial_eq! { Int32Array for Data use identity }
impl_partial_eq! { Int8Array for Data use identity }
impl_partial_eq! { Uint16Array for Data use identity }
impl_partial_eq! { Uint32Array for Data use identity }
impl_partial_eq! { Uint8Array for Data use identity }
impl_partial_eq! { Uint8ClampedArray for Data use identity }
impl_partial_eq! { BigIntObject for Data use identity }
impl_partial_eq! { BooleanObject for Data use identity }
impl_partial_eq! { Date for Data use identity }
impl_partial_eq! { Function for Data use identity }
impl_partial_eq! { Map for Data use identity }
impl_partial_eq! { NumberObject for Data use identity }
impl_partial_eq! { Promise for Data use identity }
impl_partial_eq! { PromiseResolver for Data use identity }
impl_partial_eq! { Proxy for Data use identity }
impl_partial_eq! { RegExp for Data use identity }
impl_partial_eq! { Set for Data use identity }
impl_partial_eq! { SharedArrayBuffer for Data use identity }
impl_partial_eq! { StringObject for Data use identity }
impl_partial_eq! { SymbolObject for Data use identity }
impl_partial_eq! { WasmMemoryObject for Data use identity }
impl_partial_eq! { WasmModuleObject for Data use identity }
impl_partial_eq! { Boolean for Data use identity }
impl_partial_eq! { Symbol for Data use identity }

/// An AccessorSignature specifies which receivers are valid parameters
/// to an accessor callback.
#[repr(C)]
#[derive(Debug)]
pub struct AccessorSignature(Opaque);

impl_deref! { Data for AccessorSignature }
impl_eq! { for AccessorSignature }
impl_partial_eq! { Data for AccessorSignature use identity }
impl_partial_eq! { AccessorSignature for AccessorSignature use identity }

/// A sandboxed execution context with its own set of built-in objects
/// and functions.
#[repr(C)]
#[derive(Debug)]
pub struct Context(Opaque);

impl_deref! { Data for Context }
impl_try_from! { Data for Context if v => v.is_context() }
impl_eq! { for Context }
impl_partial_eq! { Data for Context use identity }
impl_partial_eq! { Context for Context use identity }

/// A fixed-sized array with elements of type Data.
#[repr(C)]
#[derive(Debug)]
pub struct FixedArray(Opaque);

impl_deref! { Data for FixedArray }
impl_try_from! { Data for FixedArray if v => v.is_fixed_array() }
impl_eq! { for FixedArray }
impl_partial_eq! { Data for FixedArray use identity }
impl_partial_eq! { FixedArray for FixedArray use identity }

/// An error message.
#[repr(C)]
#[derive(Debug)]
pub struct Message(Opaque);

impl_eq! { for Message }
impl_partial_eq! { Message for Message use identity }

/// A compiled JavaScript module.
#[repr(C)]
#[derive(Debug)]
pub struct Module(Opaque);

impl_deref! { Data for Module }
impl_try_from! { Data for Module if v => v.is_module() }
impl_eq! { for Module }
impl_hash! { for Module use get_identity_hash }
impl_partial_eq! { Data for Module use identity }
impl_partial_eq! { Module for Module use identity }

#[repr(C)]
#[derive(Debug)]
pub struct ModuleRequest(Opaque);

impl_deref! { Data for ModuleRequest }
impl_try_from! { Data for ModuleRequest if v => v.is_module_request() }
impl_eq! { for ModuleRequest }
impl_partial_eq! { Data for ModuleRequest use identity }
impl_partial_eq! { ModuleRequest for ModuleRequest use identity }

/// An array to hold Primitive values. This is used by the embedder to
/// pass host defined options to the ScriptOptions during compilation.
///
/// This is passed back to the embedder as part of
/// HostImportModuleDynamicallyCallback for module loading.
#[repr(C)]
#[derive(Debug)]
pub struct PrimitiveArray(Opaque);

impl_deref! { Data for PrimitiveArray }
impl_eq! { for PrimitiveArray }
impl_partial_eq! { Data for PrimitiveArray use identity }
impl_partial_eq! { PrimitiveArray for PrimitiveArray use identity }

/// A private symbol
///
/// This is an experimental feature. Use at your own risk.
#[repr(C)]
#[derive(Debug)]
pub struct Private(Opaque);

impl_deref! { Data for Private }
impl_try_from! { Data for Private if v => v.is_private() }
impl_eq! { for Private }
impl_partial_eq! { Data for Private use identity }
impl_partial_eq! { Private for Private use identity }

/// A compiled JavaScript script, tied to a Context which was active when the
/// script was compiled.
#[repr(C)]
#[derive(Debug)]
pub struct Script(Opaque);

impl_eq! { for Script }
impl_partial_eq! { Script for Script use identity }

/// A container type that holds relevant metadata for module loading.
///
/// This is passed back to the embedder as part of
/// HostImportModuleDynamicallyCallback for module loading.
#[repr(C)]
#[derive(Debug)]
pub struct ScriptOrModule(Opaque);

impl_eq! { for ScriptOrModule }
impl_partial_eq! { ScriptOrModule for ScriptOrModule use identity }

/// A Signature specifies which receiver is valid for a function.
///
/// A receiver matches a given signature if the receiver (or any of its
/// hidden prototypes) was created from the signature's FunctionTemplate, or
/// from a FunctionTemplate that inherits directly or indirectly from the
/// signature's FunctionTemplate.
#[repr(C)]
#[derive(Debug)]
pub struct Signature(Opaque);

impl_deref! { Data for Signature }
impl_eq! { for Signature }
impl_partial_eq! { Data for Signature use identity }
impl_partial_eq! { Signature for Signature use identity }

/// A single JavaScript stack frame.
#[repr(C)]
#[derive(Debug)]
pub struct StackFrame(Opaque);

impl_eq! { for StackFrame }
impl_partial_eq! { StackFrame for StackFrame use identity }

/// Representation of a JavaScript stack trace. The information collected is a
/// snapshot of the execution stack and the information remains valid after
/// execution continues.
#[repr(C)]
#[derive(Debug)]
pub struct StackTrace(Opaque);

impl_eq! { for StackTrace }
impl_partial_eq! { StackTrace for StackTrace use identity }

/// The superclass of object and function templates.
#[repr(C)]
#[derive(Debug)]
pub struct Template(Opaque);

impl_deref! { Data for Template }
impl_try_from! { Data for Template if v => v.is_function_template() || v.is_object_template() }
impl_from! { FunctionTemplate for Template }
impl_from! { ObjectTemplate for Template }
impl_eq! { for Template }
impl_partial_eq! { Data for Template use identity }
impl_partial_eq! { Template for Template use identity }
impl_partial_eq! { FunctionTemplate for Template use identity }
impl_partial_eq! { ObjectTemplate for Template use identity }

/// A FunctionTemplate is used to create functions at runtime. There
/// can only be one function created from a FunctionTemplate in a
/// context. The lifetime of the created function is equal to the
/// lifetime of the context. So in case the embedder needs to create
/// temporary functions that can be collected using Scripts is
/// preferred.
///
/// Any modification of a FunctionTemplate after first instantiation will trigger
/// a crash.
///
/// A FunctionTemplate can have properties, these properties are added to the
/// function object when it is created.
///
/// A FunctionTemplate has a corresponding instance template which is
/// used to create object instances when the function is used as a
/// constructor. Properties added to the instance template are added to
/// each object instance.
///
/// A FunctionTemplate can have a prototype template. The prototype template
/// is used to create the prototype object of the function.
///
/// The following example shows how to use a FunctionTemplate:
///
/// ```ignore
///    v8::Local<v8::FunctionTemplate> t = v8::FunctionTemplate::New(isolate);
///    t->Set(isolate, "func_property", v8::Number::New(isolate, 1));
///
///    v8::Local<v8::Template> proto_t = t->PrototypeTemplate();
///    proto_t->Set(isolate,
///                 "proto_method",
///                 v8::FunctionTemplate::New(isolate, InvokeCallback));
///    proto_t->Set(isolate, "proto_const", v8::Number::New(isolate, 2));
///
///    v8::Local<v8::ObjectTemplate> instance_t = t->InstanceTemplate();
///    instance_t->SetAccessor(
///        String::NewFromUtf8Literal(isolate, "instance_accessor"),
///        InstanceAccessorCallback);
///    instance_t->SetHandler(
///        NamedPropertyHandlerConfiguration(PropertyHandlerCallback));
///    instance_t->Set(String::NewFromUtf8Literal(isolate, "instance_property"),
///                    Number::New(isolate, 3));
///
///    v8::Local<v8::Function> function = t->GetFunction();
///    v8::Local<v8::Object> instance = function->NewInstance();
/// ```
///
/// Let's use "function" as the JS variable name of the function object
/// and "instance" for the instance object created above. The function
/// and the instance will have the following properties:
///
/// ```ignore
///   func_property in function == true;
///   function.func_property == 1;
///
///   function.prototype.proto_method() invokes 'InvokeCallback'
///   function.prototype.proto_const == 2;
///
///   instance instanceof function == true;
///   instance.instance_accessor calls 'InstanceAccessorCallback'
///   instance.instance_property == 3;
/// ```
///
/// A FunctionTemplate can inherit from another one by calling the
/// FunctionTemplate::Inherit method. The following graph illustrates
/// the semantics of inheritance:
///
/// ```ignore
///   FunctionTemplate Parent  -> Parent() . prototype -> { }
///     ^                                                  ^
///     | Inherit(Parent)                                  | .__proto__
///     |                                                  |
///   FunctionTemplate Child   -> Child()  . prototype -> { }
/// ```
///
/// A FunctionTemplate 'Child' inherits from 'Parent', the prototype
/// object of the Child() function has __proto__ pointing to the
/// Parent() function's prototype object. An instance of the Child
/// function has all properties on Parent's instance templates.
///
/// Let Parent be the FunctionTemplate initialized in the previous
/// section and create a Child FunctionTemplate by:
///
/// ```ignore
///   Local<FunctionTemplate> parent = t;
///   Local<FunctionTemplate> child = FunctionTemplate::New();
///   child->Inherit(parent);
///
///   Local<Function> child_function = child->GetFunction();
///   Local<Object> child_instance = child_function->NewInstance();
/// ```
///
/// The Child function and Child instance will have the following
/// properties:
///
/// ```ignore
///   child_func.prototype.__proto__ == function.prototype;
///   child_instance.instance_accessor calls 'InstanceAccessorCallback'
///   child_instance.instance_property == 3;
/// ```
///
/// The additional 'c_function' parameter refers to a fast API call, which
/// must not trigger GC or JavaScript execution, or call into V8 in other
/// ways. For more information how to define them, see
/// include/v8-fast-api-calls.h. Please note that this feature is still
/// experimental.
#[repr(C)]
#[derive(Debug)]
pub struct FunctionTemplate(Opaque);

impl_deref! { Template for FunctionTemplate }
impl_try_from! { Data for FunctionTemplate if v => v.is_function_template() }
impl_try_from! { Template for FunctionTemplate if v => v.is_function_template() }
impl_eq! { for FunctionTemplate }
impl_partial_eq! { Data for FunctionTemplate use identity }
impl_partial_eq! { Template for FunctionTemplate use identity }
impl_partial_eq! { FunctionTemplate for FunctionTemplate use identity }

/// An ObjectTemplate is used to create objects at runtime.
///
/// Properties added to an ObjectTemplate are added to each object
/// created from the ObjectTemplate.
#[repr(C)]
#[derive(Debug)]
pub struct ObjectTemplate(Opaque);

impl_deref! { Template for ObjectTemplate }
impl_try_from! { Data for ObjectTemplate if v => v.is_object_template() }
impl_try_from! { Template for ObjectTemplate if v => v.is_object_template() }
impl_eq! { for ObjectTemplate }
impl_partial_eq! { Data for ObjectTemplate use identity }
impl_partial_eq! { Template for ObjectTemplate use identity }
impl_partial_eq! { ObjectTemplate for ObjectTemplate use identity }

/// A compiled JavaScript module, not yet tied to a Context.
#[repr(C)]
#[derive(Debug)]
pub struct UnboundModuleScript(Opaque);

impl_deref! { Data for UnboundModuleScript }
impl_eq! { for UnboundModuleScript }
impl_partial_eq! { Data for UnboundModuleScript use identity }
impl_partial_eq! { UnboundModuleScript for UnboundModuleScript use identity }

/// A compiled JavaScript script, not yet tied to a Context.
#[repr(C)]
#[derive(Debug)]
pub struct UnboundScript(Opaque);

impl_eq! { for UnboundScript }
impl_partial_eq! { UnboundScript for UnboundScript use identity }

/// The superclass of all JavaScript values and objects.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Value(Opaque);

impl_deref! { Data for Value }
impl_try_from! { Data for Value if v => v.is_value() }
impl_from! { External for Value }
impl_from! { Object for Value }
impl_from! { Array for Value }
impl_from! { ArrayBuffer for Value }
impl_from! { ArrayBufferView for Value }
impl_from! { DataView for Value }
impl_from! { TypedArray for Value }
impl_from! { BigInt64Array for Value }
impl_from! { BigUint64Array for Value }
impl_from! { Float32Array for Value }
impl_from! { Float64Array for Value }
impl_from! { Int16Array for Value }
impl_from! { Int32Array for Value }
impl_from! { Int8Array for Value }
impl_from! { Uint16Array for Value }
impl_from! { Uint32Array for Value }
impl_from! { Uint8Array for Value }
impl_from! { Uint8ClampedArray for Value }
impl_from! { BigIntObject for Value }
impl_from! { BooleanObject for Value }
impl_from! { Date for Value }
impl_from! { Function for Value }
impl_from! { Map for Value }
impl_from! { NumberObject for Value }
impl_from! { Promise for Value }
impl_from! { PromiseResolver for Value }
impl_from! { Proxy for Value }
impl_from! { RegExp for Value }
impl_from! { Set for Value }
impl_from! { SharedArrayBuffer for Value }
impl_from! { StringObject for Value }
impl_from! { SymbolObject for Value }
impl_from! { WasmMemoryObject for Value }
impl_from! { WasmModuleObject for Value }
impl_from! { Primitive for Value }
impl_from! { BigInt for Value }
impl_from! { Boolean for Value }
impl_from! { Name for Value }
impl_from! { String for Value }
impl_from! { Symbol for Value }
impl_from! { Number for Value }
impl_from! { Integer for Value }
impl_from! { Int32 for Value }
impl_from! { Uint32 for Value }
impl_eq! { for Value }
impl_hash! { for Value use get_hash }
impl_partial_eq! { Value for Value use same_value_zero }
impl_partial_eq! { External for Value use identity }
impl_partial_eq! { Object for Value use identity }
impl_partial_eq! { Array for Value use identity }
impl_partial_eq! { ArrayBuffer for Value use identity }
impl_partial_eq! { ArrayBufferView for Value use identity }
impl_partial_eq! { DataView for Value use identity }
impl_partial_eq! { TypedArray for Value use identity }
impl_partial_eq! { BigInt64Array for Value use identity }
impl_partial_eq! { BigUint64Array for Value use identity }
impl_partial_eq! { Float32Array for Value use identity }
impl_partial_eq! { Float64Array for Value use identity }
impl_partial_eq! { Int16Array for Value use identity }
impl_partial_eq! { Int32Array for Value use identity }
impl_partial_eq! { Int8Array for Value use identity }
impl_partial_eq! { Uint16Array for Value use identity }
impl_partial_eq! { Uint32Array for Value use identity }
impl_partial_eq! { Uint8Array for Value use identity }
impl_partial_eq! { Uint8ClampedArray for Value use identity }
impl_partial_eq! { BigIntObject for Value use identity }
impl_partial_eq! { BooleanObject for Value use identity }
impl_partial_eq! { Date for Value use identity }
impl_partial_eq! { Function for Value use identity }
impl_partial_eq! { Map for Value use identity }
impl_partial_eq! { NumberObject for Value use identity }
impl_partial_eq! { Promise for Value use identity }
impl_partial_eq! { PromiseResolver for Value use identity }
impl_partial_eq! { Proxy for Value use identity }
impl_partial_eq! { RegExp for Value use identity }
impl_partial_eq! { Set for Value use identity }
impl_partial_eq! { SharedArrayBuffer for Value use identity }
impl_partial_eq! { StringObject for Value use identity }
impl_partial_eq! { SymbolObject for Value use identity }
impl_partial_eq! { WasmMemoryObject for Value use identity }
impl_partial_eq! { WasmModuleObject for Value use identity }
impl_partial_eq! { Primitive for Value use same_value_zero }
impl_partial_eq! { BigInt for Value use same_value_zero }
impl_partial_eq! { Boolean for Value use identity }
impl_partial_eq! { Name for Value use same_value_zero }
impl_partial_eq! { String for Value use same_value_zero }
impl_partial_eq! { Symbol for Value use identity }
impl_partial_eq! { Number for Value use same_value_zero }
impl_partial_eq! { Integer for Value use same_value_zero }
impl_partial_eq! { Int32 for Value use same_value_zero }
impl_partial_eq! { Uint32 for Value use same_value_zero }

/// A JavaScript value that wraps a C++ void*. This type of value is mainly used
/// to associate C++ data structures with JavaScript objects.
#[repr(C)]
#[derive(Debug)]
pub struct External(Opaque);

impl_deref! { Value for External }
impl_try_from! { Data for External if v => v.is_value() && cast::<Value>(v).is_external() }
impl_try_from! { Value for External if v => v.is_external() }
impl_eq! { for External }
impl_hash! { for External use get_hash }
impl_partial_eq! { Data for External use identity }
impl_partial_eq! { Value for External use identity }
impl_partial_eq! { External for External use identity }

/// A JavaScript object (ECMA-262, 4.3.3)
#[repr(C)]
#[derive(Debug)]
pub struct Object(Opaque);

impl_deref! { Value for Object }
impl_try_from! { Data for Object if v => v.is_value() && cast::<Value>(v).is_object() }
impl_try_from! { Value for Object if v => v.is_object() }
impl_from! { Array for Object }
impl_from! { ArrayBuffer for Object }
impl_from! { ArrayBufferView for Object }
impl_from! { DataView for Object }
impl_from! { TypedArray for Object }
impl_from! { BigInt64Array for Object }
impl_from! { BigUint64Array for Object }
impl_from! { Float32Array for Object }
impl_from! { Float64Array for Object }
impl_from! { Int16Array for Object }
impl_from! { Int32Array for Object }
impl_from! { Int8Array for Object }
impl_from! { Uint16Array for Object }
impl_from! { Uint32Array for Object }
impl_from! { Uint8Array for Object }
impl_from! { Uint8ClampedArray for Object }
impl_from! { BigIntObject for Object }
impl_from! { BooleanObject for Object }
impl_from! { Date for Object }
impl_from! { Function for Object }
impl_from! { Map for Object }
impl_from! { NumberObject for Object }
impl_from! { Promise for Object }
impl_from! { PromiseResolver for Object }
impl_from! { Proxy for Object }
impl_from! { RegExp for Object }
impl_from! { Set for Object }
impl_from! { SharedArrayBuffer for Object }
impl_from! { StringObject for Object }
impl_from! { SymbolObject for Object }
impl_from! { WasmMemoryObject for Object }
impl_from! { WasmModuleObject for Object }
impl_eq! { for Object }
impl_hash! { for Object use get_identity_hash }
impl_partial_eq! { Data for Object use identity }
impl_partial_eq! { Value for Object use identity }
impl_partial_eq! { Object for Object use identity }
impl_partial_eq! { Array for Object use identity }
impl_partial_eq! { ArrayBuffer for Object use identity }
impl_partial_eq! { ArrayBufferView for Object use identity }
impl_partial_eq! { DataView for Object use identity }
impl_partial_eq! { TypedArray for Object use identity }
impl_partial_eq! { BigInt64Array for Object use identity }
impl_partial_eq! { BigUint64Array for Object use identity }
impl_partial_eq! { Float32Array for Object use identity }
impl_partial_eq! { Float64Array for Object use identity }
impl_partial_eq! { Int16Array for Object use identity }
impl_partial_eq! { Int32Array for Object use identity }
impl_partial_eq! { Int8Array for Object use identity }
impl_partial_eq! { Uint16Array for Object use identity }
impl_partial_eq! { Uint32Array for Object use identity }
impl_partial_eq! { Uint8Array for Object use identity }
impl_partial_eq! { Uint8ClampedArray for Object use identity }
impl_partial_eq! { BigIntObject for Object use identity }
impl_partial_eq! { BooleanObject for Object use identity }
impl_partial_eq! { Date for Object use identity }
impl_partial_eq! { Function for Object use identity }
impl_partial_eq! { Map for Object use identity }
impl_partial_eq! { NumberObject for Object use identity }
impl_partial_eq! { Promise for Object use identity }
impl_partial_eq! { PromiseResolver for Object use identity }
impl_partial_eq! { Proxy for Object use identity }
impl_partial_eq! { RegExp for Object use identity }
impl_partial_eq! { Set for Object use identity }
impl_partial_eq! { SharedArrayBuffer for Object use identity }
impl_partial_eq! { StringObject for Object use identity }
impl_partial_eq! { SymbolObject for Object use identity }
impl_partial_eq! { WasmMemoryObject for Object use identity }
impl_partial_eq! { WasmModuleObject for Object use identity }

/// An instance of the built-in array constructor (ECMA-262, 15.4.2).
#[repr(C)]
#[derive(Debug)]
pub struct Array(Opaque);

impl_deref! { Object for Array }
impl_try_from! { Data for Array if v => v.is_value() && cast::<Value>(v).is_array() }
impl_try_from! { Value for Array if v => v.is_array() }
impl_try_from! { Object for Array if v => v.is_array() }
impl_eq! { for Array }
impl_hash! { for Array use get_identity_hash }
impl_partial_eq! { Data for Array use identity }
impl_partial_eq! { Value for Array use identity }
impl_partial_eq! { Object for Array use identity }
impl_partial_eq! { Array for Array use identity }

/// An instance of the built-in ArrayBuffer constructor (ES6 draft 15.13.5).
#[repr(C)]
#[derive(Debug)]
pub struct ArrayBuffer(Opaque);

impl_deref! { Object for ArrayBuffer }
impl_try_from! { Data for ArrayBuffer if v => v.is_value() && cast::<Value>(v).is_array_buffer() }
impl_try_from! { Value for ArrayBuffer if v => v.is_array_buffer() }
impl_try_from! { Object for ArrayBuffer if v => v.is_array_buffer() }
impl_eq! { for ArrayBuffer }
impl_hash! { for ArrayBuffer use get_identity_hash }
impl_partial_eq! { Data for ArrayBuffer use identity }
impl_partial_eq! { Value for ArrayBuffer use identity }
impl_partial_eq! { Object for ArrayBuffer use identity }
impl_partial_eq! { ArrayBuffer for ArrayBuffer use identity }

/// A base class for an instance of one of "views" over ArrayBuffer,
/// including TypedArrays and DataView (ES6 draft 15.13).
#[repr(C)]
#[derive(Debug)]
pub struct ArrayBufferView(Opaque);

impl_deref! { Object for ArrayBufferView }
impl_try_from! { Data for ArrayBufferView if v => v.is_value() && cast::<Value>(v).is_array_buffer_view() }
impl_try_from! { Value for ArrayBufferView if v => v.is_array_buffer_view() }
impl_try_from! { Object for ArrayBufferView if v => v.is_array_buffer_view() }
impl_from! { DataView for ArrayBufferView }
impl_from! { TypedArray for ArrayBufferView }
impl_from! { BigInt64Array for ArrayBufferView }
impl_from! { BigUint64Array for ArrayBufferView }
impl_from! { Float32Array for ArrayBufferView }
impl_from! { Float64Array for ArrayBufferView }
impl_from! { Int16Array for ArrayBufferView }
impl_from! { Int32Array for ArrayBufferView }
impl_from! { Int8Array for ArrayBufferView }
impl_from! { Uint16Array for ArrayBufferView }
impl_from! { Uint32Array for ArrayBufferView }
impl_from! { Uint8Array for ArrayBufferView }
impl_from! { Uint8ClampedArray for ArrayBufferView }
impl_eq! { for ArrayBufferView }
impl_hash! { for ArrayBufferView use get_identity_hash }
impl_partial_eq! { Data for ArrayBufferView use identity }
impl_partial_eq! { Value for ArrayBufferView use identity }
impl_partial_eq! { Object for ArrayBufferView use identity }
impl_partial_eq! { ArrayBufferView for ArrayBufferView use identity }
impl_partial_eq! { DataView for ArrayBufferView use identity }
impl_partial_eq! { TypedArray for ArrayBufferView use identity }
impl_partial_eq! { BigInt64Array for ArrayBufferView use identity }
impl_partial_eq! { BigUint64Array for ArrayBufferView use identity }
impl_partial_eq! { Float32Array for ArrayBufferView use identity }
impl_partial_eq! { Float64Array for ArrayBufferView use identity }
impl_partial_eq! { Int16Array for ArrayBufferView use identity }
impl_partial_eq! { Int32Array for ArrayBufferView use identity }
impl_partial_eq! { Int8Array for ArrayBufferView use identity }
impl_partial_eq! { Uint16Array for ArrayBufferView use identity }
impl_partial_eq! { Uint32Array for ArrayBufferView use identity }
impl_partial_eq! { Uint8Array for ArrayBufferView use identity }
impl_partial_eq! { Uint8ClampedArray for ArrayBufferView use identity }

/// An instance of DataView constructor (ES6 draft 15.13.7).
#[repr(C)]
#[derive(Debug)]
pub struct DataView(Opaque);

impl_deref! { ArrayBufferView for DataView }
impl_try_from! { Data for DataView if v => v.is_value() && cast::<Value>(v).is_data_view() }
impl_try_from! { Value for DataView if v => v.is_data_view() }
impl_try_from! { Object for DataView if v => v.is_data_view() }
impl_try_from! { ArrayBufferView for DataView if v => v.is_data_view() }
impl_eq! { for DataView }
impl_hash! { for DataView use get_identity_hash }
impl_partial_eq! { Data for DataView use identity }
impl_partial_eq! { Value for DataView use identity }
impl_partial_eq! { Object for DataView use identity }
impl_partial_eq! { ArrayBufferView for DataView use identity }
impl_partial_eq! { DataView for DataView use identity }

/// A base class for an instance of TypedArray series of constructors
/// (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct TypedArray(Opaque);

impl_deref! { ArrayBufferView for TypedArray }
impl_try_from! { Data for TypedArray if v => v.is_value() && cast::<Value>(v).is_typed_array() }
impl_try_from! { Value for TypedArray if v => v.is_typed_array() }
impl_try_from! { Object for TypedArray if v => v.is_typed_array() }
impl_try_from! { ArrayBufferView for TypedArray if v => v.is_typed_array() }
impl_from! { BigInt64Array for TypedArray }
impl_from! { BigUint64Array for TypedArray }
impl_from! { Float32Array for TypedArray }
impl_from! { Float64Array for TypedArray }
impl_from! { Int16Array for TypedArray }
impl_from! { Int32Array for TypedArray }
impl_from! { Int8Array for TypedArray }
impl_from! { Uint16Array for TypedArray }
impl_from! { Uint32Array for TypedArray }
impl_from! { Uint8Array for TypedArray }
impl_from! { Uint8ClampedArray for TypedArray }
impl_eq! { for TypedArray }
impl_hash! { for TypedArray use get_identity_hash }
impl_partial_eq! { Data for TypedArray use identity }
impl_partial_eq! { Value for TypedArray use identity }
impl_partial_eq! { Object for TypedArray use identity }
impl_partial_eq! { ArrayBufferView for TypedArray use identity }
impl_partial_eq! { TypedArray for TypedArray use identity }
impl_partial_eq! { BigInt64Array for TypedArray use identity }
impl_partial_eq! { BigUint64Array for TypedArray use identity }
impl_partial_eq! { Float32Array for TypedArray use identity }
impl_partial_eq! { Float64Array for TypedArray use identity }
impl_partial_eq! { Int16Array for TypedArray use identity }
impl_partial_eq! { Int32Array for TypedArray use identity }
impl_partial_eq! { Int8Array for TypedArray use identity }
impl_partial_eq! { Uint16Array for TypedArray use identity }
impl_partial_eq! { Uint32Array for TypedArray use identity }
impl_partial_eq! { Uint8Array for TypedArray use identity }
impl_partial_eq! { Uint8ClampedArray for TypedArray use identity }

/// An instance of BigInt64Array constructor.
#[repr(C)]
#[derive(Debug)]
pub struct BigInt64Array(Opaque);

impl_deref! { TypedArray for BigInt64Array }
impl_try_from! { Data for BigInt64Array if v => v.is_value() && cast::<Value>(v).is_big_int64_array() }
impl_try_from! { Value for BigInt64Array if v => v.is_big_int64_array() }
impl_try_from! { Object for BigInt64Array if v => v.is_big_int64_array() }
impl_try_from! { ArrayBufferView for BigInt64Array if v => v.is_big_int64_array() }
impl_try_from! { TypedArray for BigInt64Array if v => v.is_big_int64_array() }
impl_eq! { for BigInt64Array }
impl_hash! { for BigInt64Array use get_identity_hash }
impl_partial_eq! { Data for BigInt64Array use identity }
impl_partial_eq! { Value for BigInt64Array use identity }
impl_partial_eq! { Object for BigInt64Array use identity }
impl_partial_eq! { ArrayBufferView for BigInt64Array use identity }
impl_partial_eq! { TypedArray for BigInt64Array use identity }
impl_partial_eq! { BigInt64Array for BigInt64Array use identity }

/// An instance of BigUint64Array constructor.
#[repr(C)]
#[derive(Debug)]
pub struct BigUint64Array(Opaque);

impl_deref! { TypedArray for BigUint64Array }
impl_try_from! { Data for BigUint64Array if v => v.is_value() && cast::<Value>(v).is_big_uint64_array() }
impl_try_from! { Value for BigUint64Array if v => v.is_big_uint64_array() }
impl_try_from! { Object for BigUint64Array if v => v.is_big_uint64_array() }
impl_try_from! { ArrayBufferView for BigUint64Array if v => v.is_big_uint64_array() }
impl_try_from! { TypedArray for BigUint64Array if v => v.is_big_uint64_array() }
impl_eq! { for BigUint64Array }
impl_hash! { for BigUint64Array use get_identity_hash }
impl_partial_eq! { Data for BigUint64Array use identity }
impl_partial_eq! { Value for BigUint64Array use identity }
impl_partial_eq! { Object for BigUint64Array use identity }
impl_partial_eq! { ArrayBufferView for BigUint64Array use identity }
impl_partial_eq! { TypedArray for BigUint64Array use identity }
impl_partial_eq! { BigUint64Array for BigUint64Array use identity }

/// An instance of Float32Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Float32Array(Opaque);

impl_deref! { TypedArray for Float32Array }
impl_try_from! { Data for Float32Array if v => v.is_value() && cast::<Value>(v).is_float32_array() }
impl_try_from! { Value for Float32Array if v => v.is_float32_array() }
impl_try_from! { Object for Float32Array if v => v.is_float32_array() }
impl_try_from! { ArrayBufferView for Float32Array if v => v.is_float32_array() }
impl_try_from! { TypedArray for Float32Array if v => v.is_float32_array() }
impl_eq! { for Float32Array }
impl_hash! { for Float32Array use get_identity_hash }
impl_partial_eq! { Data for Float32Array use identity }
impl_partial_eq! { Value for Float32Array use identity }
impl_partial_eq! { Object for Float32Array use identity }
impl_partial_eq! { ArrayBufferView for Float32Array use identity }
impl_partial_eq! { TypedArray for Float32Array use identity }
impl_partial_eq! { Float32Array for Float32Array use identity }

/// An instance of Float64Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Float64Array(Opaque);

impl_deref! { TypedArray for Float64Array }
impl_try_from! { Data for Float64Array if v => v.is_value() && cast::<Value>(v).is_float64_array() }
impl_try_from! { Value for Float64Array if v => v.is_float64_array() }
impl_try_from! { Object for Float64Array if v => v.is_float64_array() }
impl_try_from! { ArrayBufferView for Float64Array if v => v.is_float64_array() }
impl_try_from! { TypedArray for Float64Array if v => v.is_float64_array() }
impl_eq! { for Float64Array }
impl_hash! { for Float64Array use get_identity_hash }
impl_partial_eq! { Data for Float64Array use identity }
impl_partial_eq! { Value for Float64Array use identity }
impl_partial_eq! { Object for Float64Array use identity }
impl_partial_eq! { ArrayBufferView for Float64Array use identity }
impl_partial_eq! { TypedArray for Float64Array use identity }
impl_partial_eq! { Float64Array for Float64Array use identity }

/// An instance of Int16Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Int16Array(Opaque);

impl_deref! { TypedArray for Int16Array }
impl_try_from! { Data for Int16Array if v => v.is_value() && cast::<Value>(v).is_int16_array() }
impl_try_from! { Value for Int16Array if v => v.is_int16_array() }
impl_try_from! { Object for Int16Array if v => v.is_int16_array() }
impl_try_from! { ArrayBufferView for Int16Array if v => v.is_int16_array() }
impl_try_from! { TypedArray for Int16Array if v => v.is_int16_array() }
impl_eq! { for Int16Array }
impl_hash! { for Int16Array use get_identity_hash }
impl_partial_eq! { Data for Int16Array use identity }
impl_partial_eq! { Value for Int16Array use identity }
impl_partial_eq! { Object for Int16Array use identity }
impl_partial_eq! { ArrayBufferView for Int16Array use identity }
impl_partial_eq! { TypedArray for Int16Array use identity }
impl_partial_eq! { Int16Array for Int16Array use identity }

/// An instance of Int32Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Int32Array(Opaque);

impl_deref! { TypedArray for Int32Array }
impl_try_from! { Data for Int32Array if v => v.is_value() && cast::<Value>(v).is_int32_array() }
impl_try_from! { Value for Int32Array if v => v.is_int32_array() }
impl_try_from! { Object for Int32Array if v => v.is_int32_array() }
impl_try_from! { ArrayBufferView for Int32Array if v => v.is_int32_array() }
impl_try_from! { TypedArray for Int32Array if v => v.is_int32_array() }
impl_eq! { for Int32Array }
impl_hash! { for Int32Array use get_identity_hash }
impl_partial_eq! { Data for Int32Array use identity }
impl_partial_eq! { Value for Int32Array use identity }
impl_partial_eq! { Object for Int32Array use identity }
impl_partial_eq! { ArrayBufferView for Int32Array use identity }
impl_partial_eq! { TypedArray for Int32Array use identity }
impl_partial_eq! { Int32Array for Int32Array use identity }

/// An instance of Int8Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Int8Array(Opaque);

impl_deref! { TypedArray for Int8Array }
impl_try_from! { Data for Int8Array if v => v.is_value() && cast::<Value>(v).is_int8_array() }
impl_try_from! { Value for Int8Array if v => v.is_int8_array() }
impl_try_from! { Object for Int8Array if v => v.is_int8_array() }
impl_try_from! { ArrayBufferView for Int8Array if v => v.is_int8_array() }
impl_try_from! { TypedArray for Int8Array if v => v.is_int8_array() }
impl_eq! { for Int8Array }
impl_hash! { for Int8Array use get_identity_hash }
impl_partial_eq! { Data for Int8Array use identity }
impl_partial_eq! { Value for Int8Array use identity }
impl_partial_eq! { Object for Int8Array use identity }
impl_partial_eq! { ArrayBufferView for Int8Array use identity }
impl_partial_eq! { TypedArray for Int8Array use identity }
impl_partial_eq! { Int8Array for Int8Array use identity }

/// An instance of Uint16Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Uint16Array(Opaque);

impl_deref! { TypedArray for Uint16Array }
impl_try_from! { Data for Uint16Array if v => v.is_value() && cast::<Value>(v).is_uint16_array() }
impl_try_from! { Value for Uint16Array if v => v.is_uint16_array() }
impl_try_from! { Object for Uint16Array if v => v.is_uint16_array() }
impl_try_from! { ArrayBufferView for Uint16Array if v => v.is_uint16_array() }
impl_try_from! { TypedArray for Uint16Array if v => v.is_uint16_array() }
impl_eq! { for Uint16Array }
impl_hash! { for Uint16Array use get_identity_hash }
impl_partial_eq! { Data for Uint16Array use identity }
impl_partial_eq! { Value for Uint16Array use identity }
impl_partial_eq! { Object for Uint16Array use identity }
impl_partial_eq! { ArrayBufferView for Uint16Array use identity }
impl_partial_eq! { TypedArray for Uint16Array use identity }
impl_partial_eq! { Uint16Array for Uint16Array use identity }

/// An instance of Uint32Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Uint32Array(Opaque);

impl_deref! { TypedArray for Uint32Array }
impl_try_from! { Data for Uint32Array if v => v.is_value() && cast::<Value>(v).is_uint32_array() }
impl_try_from! { Value for Uint32Array if v => v.is_uint32_array() }
impl_try_from! { Object for Uint32Array if v => v.is_uint32_array() }
impl_try_from! { ArrayBufferView for Uint32Array if v => v.is_uint32_array() }
impl_try_from! { TypedArray for Uint32Array if v => v.is_uint32_array() }
impl_eq! { for Uint32Array }
impl_hash! { for Uint32Array use get_identity_hash }
impl_partial_eq! { Data for Uint32Array use identity }
impl_partial_eq! { Value for Uint32Array use identity }
impl_partial_eq! { Object for Uint32Array use identity }
impl_partial_eq! { ArrayBufferView for Uint32Array use identity }
impl_partial_eq! { TypedArray for Uint32Array use identity }
impl_partial_eq! { Uint32Array for Uint32Array use identity }

/// An instance of Uint8Array constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Uint8Array(Opaque);

impl_deref! { TypedArray for Uint8Array }
impl_try_from! { Data for Uint8Array if v => v.is_value() && cast::<Value>(v).is_uint8_array() }
impl_try_from! { Value for Uint8Array if v => v.is_uint8_array() }
impl_try_from! { Object for Uint8Array if v => v.is_uint8_array() }
impl_try_from! { ArrayBufferView for Uint8Array if v => v.is_uint8_array() }
impl_try_from! { TypedArray for Uint8Array if v => v.is_uint8_array() }
impl_eq! { for Uint8Array }
impl_hash! { for Uint8Array use get_identity_hash }
impl_partial_eq! { Data for Uint8Array use identity }
impl_partial_eq! { Value for Uint8Array use identity }
impl_partial_eq! { Object for Uint8Array use identity }
impl_partial_eq! { ArrayBufferView for Uint8Array use identity }
impl_partial_eq! { TypedArray for Uint8Array use identity }
impl_partial_eq! { Uint8Array for Uint8Array use identity }

/// An instance of Uint8ClampedArray constructor (ES6 draft 15.13.6).
#[repr(C)]
#[derive(Debug)]
pub struct Uint8ClampedArray(Opaque);

impl_deref! { TypedArray for Uint8ClampedArray }
impl_try_from! { Data for Uint8ClampedArray if v => v.is_value() && cast::<Value>(v).is_uint8_clamped_array() }
impl_try_from! { Value for Uint8ClampedArray if v => v.is_uint8_clamped_array() }
impl_try_from! { Object for Uint8ClampedArray if v => v.is_uint8_clamped_array() }
impl_try_from! { ArrayBufferView for Uint8ClampedArray if v => v.is_uint8_clamped_array() }
impl_try_from! { TypedArray for Uint8ClampedArray if v => v.is_uint8_clamped_array() }
impl_eq! { for Uint8ClampedArray }
impl_hash! { for Uint8ClampedArray use get_identity_hash }
impl_partial_eq! { Data for Uint8ClampedArray use identity }
impl_partial_eq! { Value for Uint8ClampedArray use identity }
impl_partial_eq! { Object for Uint8ClampedArray use identity }
impl_partial_eq! { ArrayBufferView for Uint8ClampedArray use identity }
impl_partial_eq! { TypedArray for Uint8ClampedArray use identity }
impl_partial_eq! { Uint8ClampedArray for Uint8ClampedArray use identity }

/// A BigInt object (https://tc39.github.io/proposal-bigint)
#[repr(C)]
#[derive(Debug)]
pub struct BigIntObject(Opaque);

impl_deref! { Object for BigIntObject }
impl_try_from! { Data for BigIntObject if v => v.is_value() && cast::<Value>(v).is_big_int_object() }
impl_try_from! { Value for BigIntObject if v => v.is_big_int_object() }
impl_try_from! { Object for BigIntObject if v => v.is_big_int_object() }
impl_eq! { for BigIntObject }
impl_hash! { for BigIntObject use get_identity_hash }
impl_partial_eq! { Data for BigIntObject use identity }
impl_partial_eq! { Value for BigIntObject use identity }
impl_partial_eq! { Object for BigIntObject use identity }
impl_partial_eq! { BigIntObject for BigIntObject use identity }

/// A Boolean object (ECMA-262, 4.3.15).
#[repr(C)]
#[derive(Debug)]
pub struct BooleanObject(Opaque);

impl_deref! { Object for BooleanObject }
impl_try_from! { Data for BooleanObject if v => v.is_value() && cast::<Value>(v).is_boolean_object() }
impl_try_from! { Value for BooleanObject if v => v.is_boolean_object() }
impl_try_from! { Object for BooleanObject if v => v.is_boolean_object() }
impl_eq! { for BooleanObject }
impl_hash! { for BooleanObject use get_identity_hash }
impl_partial_eq! { Data for BooleanObject use identity }
impl_partial_eq! { Value for BooleanObject use identity }
impl_partial_eq! { Object for BooleanObject use identity }
impl_partial_eq! { BooleanObject for BooleanObject use identity }

/// An instance of the built-in Date constructor (ECMA-262, 15.9).
#[repr(C)]
#[derive(Debug)]
pub struct Date(Opaque);

impl_deref! { Object for Date }
impl_try_from! { Data for Date if v => v.is_value() && cast::<Value>(v).is_date() }
impl_try_from! { Value for Date if v => v.is_date() }
impl_try_from! { Object for Date if v => v.is_date() }
impl_eq! { for Date }
impl_hash! { for Date use get_identity_hash }
impl_partial_eq! { Data for Date use identity }
impl_partial_eq! { Value for Date use identity }
impl_partial_eq! { Object for Date use identity }
impl_partial_eq! { Date for Date use identity }

/// A JavaScript function object (ECMA-262, 15.3).
#[repr(C)]
#[derive(Debug)]
pub struct Function(Opaque);

impl_deref! { Object for Function }
impl_try_from! { Data for Function if v => v.is_value() && cast::<Value>(v).is_function() }
impl_try_from! { Value for Function if v => v.is_function() }
impl_try_from! { Object for Function if v => v.is_function() }
impl_eq! { for Function }
impl_hash! { for Function use get_identity_hash }
impl_partial_eq! { Data for Function use identity }
impl_partial_eq! { Value for Function use identity }
impl_partial_eq! { Object for Function use identity }
impl_partial_eq! { Function for Function use identity }

/// An instance of the built-in Map constructor (ECMA-262, 6th Edition, 23.1.1).
#[repr(C)]
#[derive(Debug)]
pub struct Map(Opaque);

impl_deref! { Object for Map }
impl_try_from! { Data for Map if v => v.is_value() && cast::<Value>(v).is_map() }
impl_try_from! { Value for Map if v => v.is_map() }
impl_try_from! { Object for Map if v => v.is_map() }
impl_eq! { for Map }
impl_hash! { for Map use get_identity_hash }
impl_partial_eq! { Data for Map use identity }
impl_partial_eq! { Value for Map use identity }
impl_partial_eq! { Object for Map use identity }
impl_partial_eq! { Map for Map use identity }

/// A Number object (ECMA-262, 4.3.21).
#[repr(C)]
#[derive(Debug)]
pub struct NumberObject(Opaque);

impl_deref! { Object for NumberObject }
impl_try_from! { Data for NumberObject if v => v.is_value() && cast::<Value>(v).is_number_object() }
impl_try_from! { Value for NumberObject if v => v.is_number_object() }
impl_try_from! { Object for NumberObject if v => v.is_number_object() }
impl_eq! { for NumberObject }
impl_hash! { for NumberObject use get_identity_hash }
impl_partial_eq! { Data for NumberObject use identity }
impl_partial_eq! { Value for NumberObject use identity }
impl_partial_eq! { Object for NumberObject use identity }
impl_partial_eq! { NumberObject for NumberObject use identity }

/// An instance of the built-in Promise constructor (ES6 draft).
#[repr(C)]
#[derive(Debug)]
pub struct Promise(Opaque);

impl_deref! { Object for Promise }
impl_try_from! { Data for Promise if v => v.is_value() && cast::<Value>(v).is_promise() }
impl_try_from! { Value for Promise if v => v.is_promise() }
impl_try_from! { Object for Promise if v => v.is_promise() }
impl_eq! { for Promise }
impl_hash! { for Promise use get_identity_hash }
impl_partial_eq! { Data for Promise use identity }
impl_partial_eq! { Value for Promise use identity }
impl_partial_eq! { Object for Promise use identity }
impl_partial_eq! { Promise for Promise use identity }

#[repr(C)]
#[derive(Debug)]
pub struct PromiseResolver(Opaque);

impl_deref! { Object for PromiseResolver }
impl_eq! { for PromiseResolver }
impl_hash! { for PromiseResolver use get_identity_hash }
impl_partial_eq! { Data for PromiseResolver use identity }
impl_partial_eq! { Value for PromiseResolver use identity }
impl_partial_eq! { Object for PromiseResolver use identity }
impl_partial_eq! { PromiseResolver for PromiseResolver use identity }

/// An instance of the built-in Proxy constructor (ECMA-262, 6th Edition,
/// 26.2.1).
#[repr(C)]
#[derive(Debug)]
pub struct Proxy(Opaque);

impl_deref! { Object for Proxy }
impl_try_from! { Data for Proxy if v => v.is_value() && cast::<Value>(v).is_proxy() }
impl_try_from! { Value for Proxy if v => v.is_proxy() }
impl_try_from! { Object for Proxy if v => v.is_proxy() }
impl_eq! { for Proxy }
impl_hash! { for Proxy use get_identity_hash }
impl_partial_eq! { Data for Proxy use identity }
impl_partial_eq! { Value for Proxy use identity }
impl_partial_eq! { Object for Proxy use identity }
impl_partial_eq! { Proxy for Proxy use identity }

/// An instance of the built-in RegExp constructor (ECMA-262, 15.10).
#[repr(C)]
#[derive(Debug)]
pub struct RegExp(Opaque);

impl_deref! { Object for RegExp }
impl_try_from! { Data for RegExp if v => v.is_value() && cast::<Value>(v).is_reg_exp() }
impl_try_from! { Value for RegExp if v => v.is_reg_exp() }
impl_try_from! { Object for RegExp if v => v.is_reg_exp() }
impl_eq! { for RegExp }
impl_hash! { for RegExp use get_identity_hash }
impl_partial_eq! { Data for RegExp use identity }
impl_partial_eq! { Value for RegExp use identity }
impl_partial_eq! { Object for RegExp use identity }
impl_partial_eq! { RegExp for RegExp use identity }

/// An instance of the built-in Set constructor (ECMA-262, 6th Edition, 23.2.1).
#[repr(C)]
#[derive(Debug)]
pub struct Set(Opaque);

impl_deref! { Object for Set }
impl_try_from! { Data for Set if v => v.is_value() && cast::<Value>(v).is_set() }
impl_try_from! { Value for Set if v => v.is_set() }
impl_try_from! { Object for Set if v => v.is_set() }
impl_eq! { for Set }
impl_hash! { for Set use get_identity_hash }
impl_partial_eq! { Data for Set use identity }
impl_partial_eq! { Value for Set use identity }
impl_partial_eq! { Object for Set use identity }
impl_partial_eq! { Set for Set use identity }

/// An instance of the built-in SharedArrayBuffer constructor.
#[repr(C)]
#[derive(Debug)]
pub struct SharedArrayBuffer(Opaque);

impl_deref! { Object for SharedArrayBuffer }
impl_try_from! { Data for SharedArrayBuffer if v => v.is_value() && cast::<Value>(v).is_shared_array_buffer() }
impl_try_from! { Value for SharedArrayBuffer if v => v.is_shared_array_buffer() }
impl_try_from! { Object for SharedArrayBuffer if v => v.is_shared_array_buffer() }
impl_eq! { for SharedArrayBuffer }
impl_hash! { for SharedArrayBuffer use get_identity_hash }
impl_partial_eq! { Data for SharedArrayBuffer use identity }
impl_partial_eq! { Value for SharedArrayBuffer use identity }
impl_partial_eq! { Object for SharedArrayBuffer use identity }
impl_partial_eq! { SharedArrayBuffer for SharedArrayBuffer use identity }

/// A String object (ECMA-262, 4.3.18).
#[repr(C)]
#[derive(Debug)]
pub struct StringObject(Opaque);

impl_deref! { Object for StringObject }
impl_try_from! { Data for StringObject if v => v.is_value() && cast::<Value>(v).is_string_object() }
impl_try_from! { Value for StringObject if v => v.is_string_object() }
impl_try_from! { Object for StringObject if v => v.is_string_object() }
impl_eq! { for StringObject }
impl_hash! { for StringObject use get_identity_hash }
impl_partial_eq! { Data for StringObject use identity }
impl_partial_eq! { Value for StringObject use identity }
impl_partial_eq! { Object for StringObject use identity }
impl_partial_eq! { StringObject for StringObject use identity }

/// A Symbol object (ECMA-262 edition 6).
#[repr(C)]
#[derive(Debug)]
pub struct SymbolObject(Opaque);

impl_deref! { Object for SymbolObject }
impl_try_from! { Data for SymbolObject if v => v.is_value() && cast::<Value>(v).is_symbol_object() }
impl_try_from! { Value for SymbolObject if v => v.is_symbol_object() }
impl_try_from! { Object for SymbolObject if v => v.is_symbol_object() }
impl_eq! { for SymbolObject }
impl_hash! { for SymbolObject use get_identity_hash }
impl_partial_eq! { Data for SymbolObject use identity }
impl_partial_eq! { Value for SymbolObject use identity }
impl_partial_eq! { Object for SymbolObject use identity }
impl_partial_eq! { SymbolObject for SymbolObject use identity }

#[repr(C)]
#[derive(Debug)]
pub struct WasmMemoryObject(Opaque);

impl_deref! { Object for WasmMemoryObject }
impl_try_from! { Data for WasmMemoryObject if v => v.is_value() && cast::<Value>(v).is_wasm_memory_object() }
impl_try_from! { Value for WasmMemoryObject if v => v.is_wasm_memory_object() }
impl_try_from! { Object for WasmMemoryObject if v => v.is_wasm_memory_object() }
impl_eq! { for WasmMemoryObject }
impl_hash! { for WasmMemoryObject use get_identity_hash }
impl_partial_eq! { Data for WasmMemoryObject use identity }
impl_partial_eq! { Value for WasmMemoryObject use identity }
impl_partial_eq! { Object for WasmMemoryObject use identity }
impl_partial_eq! { WasmMemoryObject for WasmMemoryObject use identity }

#[repr(C)]
#[derive(Debug)]
pub struct WasmModuleObject(Opaque);

impl_deref! { Object for WasmModuleObject }
impl_try_from! { Data for WasmModuleObject if v => v.is_value() && cast::<Value>(v).is_wasm_module_object() }
impl_try_from! { Value for WasmModuleObject if v => v.is_wasm_module_object() }
impl_try_from! { Object for WasmModuleObject if v => v.is_wasm_module_object() }
impl_eq! { for WasmModuleObject }
impl_hash! { for WasmModuleObject use get_identity_hash }
impl_partial_eq! { Data for WasmModuleObject use identity }
impl_partial_eq! { Value for WasmModuleObject use identity }
impl_partial_eq! { Object for WasmModuleObject use identity }
impl_partial_eq! { WasmModuleObject for WasmModuleObject use identity }

/// The superclass of primitive values. See ECMA-262 4.3.2.
#[repr(C)]
#[derive(Debug)]
pub struct Primitive(Opaque);

impl_deref! { Value for Primitive }
impl_try_from! { Data for Primitive if v => v.is_primitive() }
impl_try_from! { Value for Primitive if v => v.is_primitive() }
impl_from! { BigInt for Primitive }
impl_from! { Boolean for Primitive }
impl_from! { Name for Primitive }
impl_from! { String for Primitive }
impl_from! { Symbol for Primitive }
impl_from! { Number for Primitive }
impl_from! { Integer for Primitive }
impl_from! { Int32 for Primitive }
impl_from! { Uint32 for Primitive }
impl_eq! { for Primitive }
impl_hash! { for Primitive use get_hash }
impl_partial_eq! { Value for Primitive use same_value_zero }
impl_partial_eq! { Primitive for Primitive use same_value_zero }
impl_partial_eq! { BigInt for Primitive use same_value_zero }
impl_partial_eq! { Boolean for Primitive use identity }
impl_partial_eq! { Name for Primitive use same_value_zero }
impl_partial_eq! { String for Primitive use same_value_zero }
impl_partial_eq! { Symbol for Primitive use identity }
impl_partial_eq! { Number for Primitive use same_value_zero }
impl_partial_eq! { Integer for Primitive use same_value_zero }
impl_partial_eq! { Int32 for Primitive use same_value_zero }
impl_partial_eq! { Uint32 for Primitive use same_value_zero }

/// A JavaScript BigInt value (https://tc39.github.io/proposal-bigint)
#[repr(C)]
#[derive(Debug)]
pub struct BigInt(Opaque);

impl_deref! { Primitive for BigInt }
impl_try_from! { Data for BigInt if v => v.is_big_int() }
impl_try_from! { Value for BigInt if v => v.is_big_int() }
impl_try_from! { Primitive for BigInt if v => v.is_big_int() }
impl_eq! { for BigInt }
impl_hash! { for BigInt use get_hash }
impl_partial_eq! { Value for BigInt use same_value_zero }
impl_partial_eq! { Primitive for BigInt use same_value_zero }
impl_partial_eq! { BigInt for BigInt use strict_equals }

/// A primitive boolean value (ECMA-262, 4.3.14). Either the true
/// or false value.
#[repr(C)]
#[derive(Debug)]
pub struct Boolean(Opaque);

impl_deref! { Primitive for Boolean }
impl_try_from! { Data for Boolean if v => v.is_boolean() }
impl_try_from! { Value for Boolean if v => v.is_boolean() }
impl_try_from! { Primitive for Boolean if v => v.is_boolean() }
impl_eq! { for Boolean }
impl_hash! { for Boolean use get_hash }
impl_partial_eq! { Data for Boolean use identity }
impl_partial_eq! { Value for Boolean use identity }
impl_partial_eq! { Primitive for Boolean use identity }
impl_partial_eq! { Boolean for Boolean use identity }

/// A superclass for symbols and strings.
#[repr(C)]
#[derive(Debug)]
pub struct Name(Opaque);

impl_deref! { Primitive for Name }
impl_try_from! { Data for Name if v => v.is_name() }
impl_try_from! { Value for Name if v => v.is_name() }
impl_try_from! { Primitive for Name if v => v.is_name() }
impl_from! { String for Name }
impl_from! { Symbol for Name }
impl_eq! { for Name }
impl_hash! { for Name use get_identity_hash }
impl_partial_eq! { Value for Name use same_value_zero }
impl_partial_eq! { Primitive for Name use same_value_zero }
impl_partial_eq! { Name for Name use strict_equals }
impl_partial_eq! { String for Name use strict_equals }
impl_partial_eq! { Symbol for Name use identity }

/// A JavaScript string value (ECMA-262, 4.3.17).
#[repr(C)]
#[derive(Debug)]
pub struct String(Opaque);

impl_deref! { Name for String }
impl_try_from! { Data for String if v => v.is_string() }
impl_try_from! { Value for String if v => v.is_string() }
impl_try_from! { Primitive for String if v => v.is_string() }
impl_try_from! { Name for String if v => v.is_string() }
impl_eq! { for String }
impl_hash! { for String use get_identity_hash }
impl_partial_eq! { Value for String use same_value_zero }
impl_partial_eq! { Primitive for String use same_value_zero }
impl_partial_eq! { Name for String use strict_equals }
impl_partial_eq! { String for String use strict_equals }

/// A JavaScript symbol (ECMA-262 edition 6)
#[repr(C)]
#[derive(Debug)]
pub struct Symbol(Opaque);

impl_deref! { Name for Symbol }
impl_try_from! { Data for Symbol if v => v.is_symbol() }
impl_try_from! { Value for Symbol if v => v.is_symbol() }
impl_try_from! { Primitive for Symbol if v => v.is_symbol() }
impl_try_from! { Name for Symbol if v => v.is_symbol() }
impl_eq! { for Symbol }
impl_hash! { for Symbol use get_identity_hash }
impl_partial_eq! { Data for Symbol use identity }
impl_partial_eq! { Value for Symbol use identity }
impl_partial_eq! { Primitive for Symbol use identity }
impl_partial_eq! { Name for Symbol use identity }
impl_partial_eq! { Symbol for Symbol use identity }

/// A JavaScript number value (ECMA-262, 4.3.20)
#[repr(C)]
#[derive(Debug)]
pub struct Number(Opaque);

impl_deref! { Primitive for Number }
impl_try_from! { Data for Number if v => v.is_number() }
impl_try_from! { Value for Number if v => v.is_number() }
impl_try_from! { Primitive for Number if v => v.is_number() }
impl_from! { Integer for Number }
impl_from! { Int32 for Number }
impl_from! { Uint32 for Number }
impl_eq! { for Number }
impl_hash! { for Number use get_hash }
impl_partial_eq! { Value for Number use same_value_zero }
impl_partial_eq! { Primitive for Number use same_value_zero }
impl_partial_eq! { Number for Number use same_value_zero }
impl_partial_eq! { Integer for Number use same_value_zero }
impl_partial_eq! { Int32 for Number use same_value_zero }
impl_partial_eq! { Uint32 for Number use same_value_zero }

/// A JavaScript value representing a signed integer.
#[repr(C)]
#[derive(Debug)]
pub struct Integer(Opaque);

impl_deref! { Number for Integer }
impl_try_from! { Data for Integer if v => v.is_number() && (cast::<Number>(v).is_int32() || cast::<Number>(v).is_uint32()) }
impl_try_from! { Value for Integer if v => v.is_int32() || v.is_uint32() }
impl_try_from! { Primitive for Integer if v => v.is_int32() || v.is_uint32() }
impl_try_from! { Number for Integer if v => v.is_int32() || v.is_uint32() }
impl_from! { Int32 for Integer }
impl_from! { Uint32 for Integer }
impl_eq! { for Integer }
impl_hash! { for Integer use get_hash }
impl_partial_eq! { Value for Integer use same_value_zero }
impl_partial_eq! { Primitive for Integer use same_value_zero }
impl_partial_eq! { Number for Integer use same_value_zero }
impl_partial_eq! { Integer for Integer use strict_equals }
impl_partial_eq! { Int32 for Integer use strict_equals }
impl_partial_eq! { Uint32 for Integer use strict_equals }

/// A JavaScript value representing a 32-bit signed integer.
#[repr(C)]
#[derive(Debug)]
pub struct Int32(Opaque);

impl_deref! { Integer for Int32 }
impl_try_from! { Data for Int32 if v => v.is_number() && cast::<Number>(v).is_int32() }
impl_try_from! { Value for Int32 if v => v.is_int32() }
impl_try_from! { Primitive for Int32 if v => v.is_int32() }
impl_try_from! { Number for Int32 if v => v.is_int32() }
impl_try_from! { Integer for Int32 if v => v.is_int32() }
impl_eq! { for Int32 }
impl_hash! { for Int32 use get_hash }
impl_partial_eq! { Value for Int32 use same_value_zero }
impl_partial_eq! { Primitive for Int32 use same_value_zero }
impl_partial_eq! { Number for Int32 use same_value_zero }
impl_partial_eq! { Integer for Int32 use strict_equals }
impl_partial_eq! { Int32 for Int32 use strict_equals }

/// A JavaScript value representing a 32-bit unsigned integer.
#[repr(C)]
#[derive(Debug)]
pub struct Uint32(Opaque);

impl_deref! { Integer for Uint32 }
impl_try_from! { Data for Uint32 if v => v.is_number() && cast::<Number>(v).is_uint32() }
impl_try_from! { Value for Uint32 if v => v.is_uint32() }
impl_try_from! { Primitive for Uint32 if v => v.is_uint32() }
impl_try_from! { Number for Uint32 if v => v.is_uint32() }
impl_try_from! { Integer for Uint32 if v => v.is_uint32() }
impl_eq! { for Uint32 }
impl_hash! { for Uint32 use get_hash }
impl_partial_eq! { Value for Uint32 use same_value_zero }
impl_partial_eq! { Primitive for Uint32 use same_value_zero }
impl_partial_eq! { Number for Uint32 use same_value_zero }
impl_partial_eq! { Integer for Uint32 use strict_equals }
impl_partial_eq! { Uint32 for Uint32 use strict_equals }
