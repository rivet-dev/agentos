use crate::BigInt;
use crate::Boolean;
use crate::Context;
use crate::HandleScope;
use crate::Int32;
use crate::Integer;
use crate::Isolate;
use crate::Local;
use crate::Number;
use crate::Object;
use crate::String;
use crate::Uint32;
use crate::Value;
use crate::support::Maybe;

unsafe extern "C" {
  fn v8__Value__IsUndefined(this: *const Value) -> bool;
  fn v8__Value__IsNull(this: *const Value) -> bool;
  fn v8__Value__IsNullOrUndefined(this: *const Value) -> bool;
  fn v8__Value__IsTrue(this: *const Value) -> bool;
  fn v8__Value__IsFalse(this: *const Value) -> bool;
  fn v8__Value__IsName(this: *const Value) -> bool;
  fn v8__Value__IsString(this: *const Value) -> bool;
  fn v8__Value__IsSymbol(this: *const Value) -> bool;
  fn v8__Value__IsFunction(this: *const Value) -> bool;
  fn v8__Value__IsArray(this: *const Value) -> bool;
  fn v8__Value__IsObject(this: *const Value) -> bool;
  fn v8__Value__IsBigInt(this: *const Value) -> bool;
  fn v8__Value__IsBoolean(this: *const Value) -> bool;
  fn v8__Value__IsNumber(this: *const Value) -> bool;
  fn v8__Value__IsExternal(this: *const Value) -> bool;
  fn v8__Value__IsInt32(this: *const Value) -> bool;
  fn v8__Value__IsUint32(this: *const Value) -> bool;
  fn v8__Value__IsDate(this: *const Value) -> bool;
  fn v8__Value__IsArgumentsObject(this: *const Value) -> bool;
  fn v8__Value__IsBigIntObject(this: *const Value) -> bool;
  fn v8__Value__IsBooleanObject(this: *const Value) -> bool;
  fn v8__Value__IsNumberObject(this: *const Value) -> bool;
  fn v8__Value__IsStringObject(this: *const Value) -> bool;
  fn v8__Value__IsSymbolObject(this: *const Value) -> bool;
  fn v8__Value__IsNativeError(this: *const Value) -> bool;
  fn v8__Value__IsRegExp(this: *const Value) -> bool;
  fn v8__Value__IsAsyncFunction(this: *const Value) -> bool;
  fn v8__Value__IsGeneratorFunction(this: *const Value) -> bool;
  #[allow(dead_code)]
  fn v8__Value__IsGeneratorObject(this: *const Value) -> bool;
  fn v8__Value__IsPromise(this: *const Value) -> bool;
  fn v8__Value__IsMap(this: *const Value) -> bool;
  fn v8__Value__IsSet(this: *const Value) -> bool;
  fn v8__Value__IsMapIterator(this: *const Value) -> bool;
  fn v8__Value__IsSetIterator(this: *const Value) -> bool;
  fn v8__Value__IsSetGeneratorObject(this: *const Value) -> bool;
  fn v8__Value__IsWeakMap(this: *const Value) -> bool;
  fn v8__Value__IsWeakSet(this: *const Value) -> bool;
  fn v8__Value__IsArrayBuffer(this: *const Value) -> bool;
  fn v8__Value__IsArrayBufferView(this: *const Value) -> bool;
  fn v8__Value__IsTypedArray(this: *const Value) -> bool;
  fn v8__Value__IsUint8Array(this: *const Value) -> bool;
  fn v8__Value__IsUint8ClampedArray(this: *const Value) -> bool;
  fn v8__Value__IsInt8Array(this: *const Value) -> bool;
  fn v8__Value__IsUint16Array(this: *const Value) -> bool;
  fn v8__Value__IsInt16Array(this: *const Value) -> bool;
  fn v8__Value__IsUint32Array(this: *const Value) -> bool;
  fn v8__Value__IsInt32Array(this: *const Value) -> bool;
  fn v8__Value__IsFloat32Array(this: *const Value) -> bool;
  fn v8__Value__IsFloat64Array(this: *const Value) -> bool;
  fn v8__Value__IsBigInt64Array(this: *const Value) -> bool;
  fn v8__Value__IsBigUint64Array(this: *const Value) -> bool;
  fn v8__Value__IsDataView(this: *const Value) -> bool;
  fn v8__Value__IsSharedArrayBuffer(this: *const Value) -> bool;
  fn v8__Value__IsProxy(this: *const Value) -> bool;
  fn v8__Value__IsWasmMemoryObject(this: *const Value) -> bool;
  fn v8__Value__IsWasmModuleObject(this: *const Value) -> bool;
  fn v8__Value__IsModuleNamespaceObject(this: *const Value) -> bool;
  fn v8__Value__StrictEquals(this: *const Value, that: *const Value) -> bool;
  fn v8__Value__SameValue(this: *const Value, that: *const Value) -> bool;
  fn v8__Value__InstanceOf(
    this: *const Value,
    context: *const Context,
    object: *const Object,
    out: *mut Maybe<bool>,
  );
  fn v8__Value__ToBigInt(
    this: *const Value,
    context: *const Context,
  ) -> *const BigInt;
  fn v8__Value__ToNumber(
    this: *const Value,
    context: *const Context,
  ) -> *const Number;
  fn v8__Value__ToString(
    this: *const Value,
    context: *const Context,
  ) -> *const String;
  fn v8__Value__ToDetailString(
    this: *const Value,
    context: *const Context,
  ) -> *const String;
  fn v8__Value__ToObject(
    this: *const Value,
    context: *const Context,
  ) -> *const Object;
  fn v8__Value__ToInteger(
    this: *const Value,
    context: *const Context,
  ) -> *const Integer;
  fn v8__Value__ToUint32(
    this: *const Value,
    context: *const Context,
  ) -> *const Uint32;
  fn v8__Value__ToInt32(
    this: *const Value,
    context: *const Context,
  ) -> *const Int32;
  fn v8__Value__ToBoolean(
    this: *const Value,
    isolate: *mut Isolate,
  ) -> *const Boolean;

  fn v8__Value__NumberValue(
    this: *const Value,
    context: *const Context,
    out: *mut Maybe<f64>,
  );
  fn v8__Value__IntegerValue(
    this: *const Value,
    context: *const Context,
    out: *mut Maybe<i64>,
  );
  fn v8__Value__Uint32Value(
    this: *const Value,
    context: *const Context,
    out: *mut Maybe<u32>,
  );
  fn v8__Value__Int32Value(
    this: *const Value,
    context: *const Context,
    out: *mut Maybe<i32>,
  );
  fn v8__Value__BooleanValue(this: *const Value, isolate: *mut Isolate)
  -> bool;
  fn v8__Value__GetHash(this: *const Value) -> u32;
  fn v8__Value__TypeOf(
    this: *const Value,
    isolate: *mut Isolate,
  ) -> *const String;
}

impl Value {
  /// Returns true if this value is the undefined value.  See ECMA-262 4.3.10.
  #[inline(always)]
  pub fn is_undefined(&self) -> bool {
    unsafe { v8__Value__IsUndefined(self) }
  }

  /// Returns true if this value is the null value.  See ECMA-262 4.3.11.
  #[inline(always)]
  pub fn is_null(&self) -> bool {
    unsafe { v8__Value__IsNull(self) }
  }

  /// Returns true if this value is either the null or the undefined value.
  /// See ECMA-262 4.3.11. and 4.3.12
  #[inline(always)]
  pub fn is_null_or_undefined(&self) -> bool {
    unsafe { v8__Value__IsNullOrUndefined(self) }
  }

  /// Returns true if this value is true.
  /// This is not the same as `BooleanValue()`. The latter performs a
  /// conversion to boolean, i.e. the result of `Boolean(value)` in JS, whereas
  /// this checks `value === true`.
  #[inline(always)]
  pub fn is_true(&self) -> bool {
    unsafe { v8__Value__IsTrue(self) }
  }

  /// Returns true if this value is false.
  /// This is not the same as `!BooleanValue()`. The latter performs a
  /// conversion to boolean, i.e. the result of `!Boolean(value)` in JS, whereas
  /// this checks `value === false`.
  #[inline(always)]
  pub fn is_false(&self) -> bool {
    unsafe { v8__Value__IsFalse(self) }
  }

  /// Returns true if this value is a symbol or a string.
  /// This is equivalent to
  /// `typeof value === 'string' || typeof value === 'symbol'` in JS.
  #[inline(always)]
  pub fn is_name(&self) -> bool {
    unsafe { v8__Value__IsName(self) }
  }

  /// Returns true if this value is an instance of the String type.
  /// See ECMA-262 8.4.
  #[inline(always)]
  pub fn is_string(&self) -> bool {
    unsafe { v8__Value__IsString(self) }
  }

  /// Returns true if this value is a symbol.
  /// This is equivalent to `typeof value === 'symbol'` in JS.
  #[inline(always)]
  pub fn is_symbol(&self) -> bool {
    unsafe { v8__Value__IsSymbol(self) }
  }

  /// Returns true if this value is a function.
  #[inline(always)]
  pub fn is_function(&self) -> bool {
    unsafe { v8__Value__IsFunction(self) }
  }

  /// Returns true if this value is an array. Note that it will return false for
  /// an Proxy for an array.
  #[inline(always)]
  pub fn is_array(&self) -> bool {
    unsafe { v8__Value__IsArray(self) }
  }

  /// Returns true if this value is an object.
  #[inline(always)]
  pub fn is_object(&self) -> bool {
    unsafe { v8__Value__IsObject(self) }
  }

  /// Returns true if this value is a bigint.
  /// This is equivalent to `typeof value === 'bigint'` in JS.
  #[inline(always)]
  pub fn is_big_int(&self) -> bool {
    unsafe { v8__Value__IsBigInt(self) }
  }

  /// Returns true if this value is boolean.
  /// This is equivalent to `typeof value === 'boolean'` in JS.
  #[inline(always)]
  pub fn is_boolean(&self) -> bool {
    unsafe { v8__Value__IsBoolean(self) }
  }

  /// Returns true if this value is a number.
  #[inline(always)]
  pub fn is_number(&self) -> bool {
    unsafe { v8__Value__IsNumber(self) }
  }

  /// Returns true if this value is an `External` object.
  #[inline(always)]
  pub fn is_external(&self) -> bool {
    unsafe { v8__Value__IsExternal(self) }
  }

  /// Returns true if this value is a 32-bit signed integer.
  #[inline(always)]
  pub fn is_int32(&self) -> bool {
    unsafe { v8__Value__IsInt32(self) }
  }

  /// Returns true if this value is a 32-bit unsigned integer.
  #[inline(always)]
  pub fn is_uint32(&self) -> bool {
    unsafe { v8__Value__IsUint32(self) }
  }

  /// Returns true if this value is a Date.
  #[inline(always)]
  pub fn is_date(&self) -> bool {
    unsafe { v8__Value__IsDate(self) }
  }

  /// Returns true if this value is an Arguments object.
  #[inline(always)]
  pub fn is_arguments_object(&self) -> bool {
    unsafe { v8__Value__IsArgumentsObject(self) }
  }

  /// Returns true if this value is a BigInt object.
  #[inline(always)]
  pub fn is_big_int_object(&self) -> bool {
    unsafe { v8__Value__IsBigIntObject(self) }
  }

  /// Returns true if this value is a Boolean object.
  #[inline(always)]
  pub fn is_boolean_object(&self) -> bool {
    unsafe { v8__Value__IsBooleanObject(self) }
  }

  /// Returns true if this value is a Number object.
  #[inline(always)]
  pub fn is_number_object(&self) -> bool {
    unsafe { v8__Value__IsNumberObject(self) }
  }

  /// Returns true if this value is a String object.
  #[inline(always)]
  pub fn is_string_object(&self) -> bool {
    unsafe { v8__Value__IsStringObject(self) }
  }

  /// Returns true if this value is a Symbol object.
  #[inline(always)]
  pub fn is_symbol_object(&self) -> bool {
    unsafe { v8__Value__IsSymbolObject(self) }
  }

  /// Returns true if this value is a NativeError.
  #[inline(always)]
  pub fn is_native_error(&self) -> bool {
    unsafe { v8__Value__IsNativeError(self) }
  }

  /// Returns true if this value is a RegExp.
  #[inline(always)]
  pub fn is_reg_exp(&self) -> bool {
    unsafe { v8__Value__IsRegExp(self) }
  }

  /// Returns true if this value is an async function.
  #[inline(always)]
  pub fn is_async_function(&self) -> bool {
    unsafe { v8__Value__IsAsyncFunction(self) }
  }

  /// Returns true if this value is a Generator function.
  #[inline(always)]
  pub fn is_generator_function(&self) -> bool {
    unsafe { v8__Value__IsGeneratorFunction(self) }
  }

  /// Returns true if this value is a Promise.
  #[inline(always)]
  pub fn is_promise(&self) -> bool {
    unsafe { v8__Value__IsPromise(self) }
  }

  /// Returns true if this value is a Map.
  #[inline(always)]
  pub fn is_map(&self) -> bool {
    unsafe { v8__Value__IsMap(self) }
  }

  /// Returns true if this value is a Set.
  #[inline(always)]
  pub fn is_set(&self) -> bool {
    unsafe { v8__Value__IsSet(self) }
  }

  /// Returns true if this value is a Map Iterator.
  #[inline(always)]
  pub fn is_map_iterator(&self) -> bool {
    unsafe { v8__Value__IsMapIterator(self) }
  }

  /// Returns true if this value is a Set Iterator.
  #[inline(always)]
  pub fn is_set_iterator(&self) -> bool {
    unsafe { v8__Value__IsSetIterator(self) }
  }

  /// Returns true if this value is a Generator Object.
  #[inline(always)]
  pub fn is_generator_object(&self) -> bool {
    unsafe { v8__Value__IsSetGeneratorObject(self) }
  }

  /// Returns true if this value is a WeakMap.
  #[inline(always)]
  pub fn is_weak_map(&self) -> bool {
    unsafe { v8__Value__IsWeakMap(self) }
  }

  /// Returns true if this value is a WeakSet.
  #[inline(always)]
  pub fn is_weak_set(&self) -> bool {
    unsafe { v8__Value__IsWeakSet(self) }
  }

  /// Returns true if this value is an ArrayBuffer.
  #[inline(always)]
  pub fn is_array_buffer(&self) -> bool {
    unsafe { v8__Value__IsArrayBuffer(self) }
  }

  /// Returns true if this value is an ArrayBufferView.
  #[inline(always)]
  pub fn is_array_buffer_view(&self) -> bool {
    unsafe { v8__Value__IsArrayBufferView(self) }
  }

  /// Returns true if this value is one of TypedArrays.
  #[inline(always)]
  pub fn is_typed_array(&self) -> bool {
    unsafe { v8__Value__IsTypedArray(self) }
  }

  /// Returns true if this value is an Uint8Array.
  #[inline(always)]
  pub fn is_uint8_array(&self) -> bool {
    unsafe { v8__Value__IsUint8Array(self) }
  }

  /// Returns true if this value is an Uint8ClampedArray.
  #[inline(always)]
  pub fn is_uint8_clamped_array(&self) -> bool {
    unsafe { v8__Value__IsUint8ClampedArray(self) }
  }

  /// Returns true if this value is an Int8Array.
  #[inline(always)]
  pub fn is_int8_array(&self) -> bool {
    unsafe { v8__Value__IsInt8Array(self) }
  }

  /// Returns true if this value is an Uint16Array.
  #[inline(always)]
  pub fn is_uint16_array(&self) -> bool {
    unsafe { v8__Value__IsUint16Array(self) }
  }

  /// Returns true if this value is an Int16Array.
  #[inline(always)]
  pub fn is_int16_array(&self) -> bool {
    unsafe { v8__Value__IsInt16Array(self) }
  }

  /// Returns true if this value is an Uint32Array.
  #[inline(always)]
  pub fn is_uint32_array(&self) -> bool {
    unsafe { v8__Value__IsUint32Array(self) }
  }

  /// Returns true if this value is an Int32Array.
  #[inline(always)]
  pub fn is_int32_array(&self) -> bool {
    unsafe { v8__Value__IsInt32Array(self) }
  }

  /// Returns true if this value is a Float32Array.
  #[inline(always)]
  pub fn is_float32_array(&self) -> bool {
    unsafe { v8__Value__IsFloat32Array(self) }
  }

  /// Returns true if this value is a Float64Array.
  #[inline(always)]
  pub fn is_float64_array(&self) -> bool {
    unsafe { v8__Value__IsFloat64Array(self) }
  }

  /// Returns true if this value is a BigInt64Array.
  #[inline(always)]
  pub fn is_big_int64_array(&self) -> bool {
    unsafe { v8__Value__IsBigInt64Array(self) }
  }

  /// Returns true if this value is a BigUint64Array.
  #[inline(always)]
  pub fn is_big_uint64_array(&self) -> bool {
    unsafe { v8__Value__IsBigUint64Array(self) }
  }

  /// Returns true if this value is a DataView.
  #[inline(always)]
  pub fn is_data_view(&self) -> bool {
    unsafe { v8__Value__IsDataView(self) }
  }

  /// Returns true if this value is a SharedArrayBuffer.
  /// This is an experimental feature.
  #[inline(always)]
  pub fn is_shared_array_buffer(&self) -> bool {
    unsafe { v8__Value__IsSharedArrayBuffer(self) }
  }

  /// Returns true if this value is a JavaScript Proxy.
  #[inline(always)]
  pub fn is_proxy(&self) -> bool {
    unsafe { v8__Value__IsProxy(self) }
  }

  /// Returns true if this value is a WasmMemoryObject.
  #[inline(always)]
  pub fn is_wasm_memory_object(&self) -> bool {
    unsafe { v8__Value__IsWasmMemoryObject(self) }
  }

  /// Returns true if this value is a WasmModuleObject.
  #[inline(always)]
  pub fn is_wasm_module_object(&self) -> bool {
    unsafe { v8__Value__IsWasmModuleObject(self) }
  }

  /// Returns true if the value is a Module Namespace Object.
  #[inline(always)]
  pub fn is_module_namespace_object(&self) -> bool {
    unsafe { v8__Value__IsModuleNamespaceObject(self) }
  }

  #[inline(always)]
  pub fn strict_equals(&self, that: Local<Value>) -> bool {
    unsafe { v8__Value__StrictEquals(self, &*that) }
  }

  #[inline(always)]
  pub fn same_value(&self, that: Local<Value>) -> bool {
    unsafe { v8__Value__SameValue(self, &*that) }
  }

  /// Implements the the abstract operation `SameValueZero`, which is defined in
  /// ECMA-262 6th edition § 7.2.10
  /// (http://ecma-international.org/ecma-262/6.0/#sec-samevaluezero).
  ///
  /// This operation is used to compare values for the purpose of insertion into
  /// a `Set`, or determining whether `Map` keys are equivalent. Its semantics
  /// are almost the same as `strict_equals()` and `same_value()`, with the
  /// following important distinctions:
  ///   - It considers `NaN` equal to `NaN` (unlike `strict_equals()`).
  ///   - It considers `-0` equal to `0` (unlike `same_value()`).
  #[inline(always)]
  pub fn same_value_zero(&self, that: Local<Value>) -> bool {
    // The SMI representation of zero is also zero. In debug builds, double
    // check this, so in the unlikely event that V8 changes its internal
    // representation of SMIs such that this invariant no longer holds, we'd
    // catch it.
    self.same_value(that) || {
      let zero = Integer::zero().into();
      self.strict_equals(zero) && that.strict_equals(zero)
    }
  }

  #[inline(always)]
  pub fn to_big_int<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, BigInt>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToBigInt(self, &*sd.get_current_context()))
    }
  }

  #[inline(always)]
  pub fn to_number<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Number>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToNumber(self, &*sd.get_current_context()))
    }
  }

  #[inline(always)]
  pub fn to_string<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToString(self, &*sd.get_current_context()))
    }
  }

  /// Convenience function not present in the original V8 API.
  #[inline(always)]
  pub fn to_rust_string_lossy(
    &self,
    scope: &mut HandleScope,
  ) -> std::string::String {
    self
      .to_string(scope)
      .map_or_else(std::string::String::new, |s| s.to_rust_string_lossy(scope))
  }

  #[inline(always)]
  pub fn to_detail_string<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, String>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Value__ToDetailString(self, &*sd.get_current_context())
      })
    }
  }

  #[inline(always)]
  pub fn to_object<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Object>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToObject(self, &*sd.get_current_context()))
    }
  }

  #[inline(always)]
  pub fn to_integer<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Integer>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToInteger(self, &*sd.get_current_context()))
    }
  }

  #[inline(always)]
  pub fn to_uint32<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Uint32>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToUint32(self, &*sd.get_current_context()))
    }
  }

  #[inline(always)]
  pub fn to_int32<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Int32>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Value__ToInt32(self, &*sd.get_current_context()))
    }
  }

  /// Perform the equivalent of Boolean(value) in JS. This can never fail.
  #[inline(always)]
  pub fn to_boolean<'s>(
    &self,
    scope: &mut HandleScope<'s, ()>,
  ) -> Local<'s, Boolean> {
    unsafe {
      scope.cast_local(|sd| v8__Value__ToBoolean(self, sd.get_isolate_ptr()))
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn instance_of(
    &self,
    scope: &mut HandleScope,
    object: Local<Object>,
  ) -> Option<bool> {
    let mut out = Maybe::<bool>::default();
    unsafe {
      v8__Value__InstanceOf(
        self,
        &*scope.get_current_context(),
        &*object,
        &mut out,
      );
    }
    out.into()
  }

  #[inline(always)]
  pub fn number_value(&self, scope: &mut HandleScope) -> Option<f64> {
    let mut out = Maybe::<f64>::default();
    unsafe {
      v8__Value__NumberValue(self, &*scope.get_current_context(), &mut out);
    };
    out.into()
  }

  #[inline(always)]
  pub fn integer_value(&self, scope: &mut HandleScope) -> Option<i64> {
    let mut out = Maybe::<i64>::default();
    unsafe {
      v8__Value__IntegerValue(self, &*scope.get_current_context(), &mut out);
    };
    out.into()
  }

  #[inline(always)]
  pub fn uint32_value(&self, scope: &mut HandleScope) -> Option<u32> {
    let mut out = Maybe::<u32>::default();
    unsafe {
      v8__Value__Uint32Value(self, &*scope.get_current_context(), &mut out);
    };
    out.into()
  }

  #[inline(always)]
  pub fn int32_value(&self, scope: &mut HandleScope) -> Option<i32> {
    let mut out = Maybe::<i32>::default();
    unsafe {
      v8__Value__Int32Value(self, &*scope.get_current_context(), &mut out);
    };
    out.into()
  }

  #[inline(always)]
  pub fn boolean_value(&self, scope: &mut HandleScope<'_, ()>) -> bool {
    unsafe { v8__Value__BooleanValue(self, scope.get_isolate_ptr()) }
  }

  /// Get the hash of this value. The hash is not guaranteed to be
  /// unique. For |Object| and |Name| instances the result is equal to
  /// |GetIdentityHash|. Hashes are not guaranteed to be stable across
  /// different isolates or processes.
  #[inline(always)]
  pub fn get_hash(&self) -> u32 {
    unsafe { v8__Value__GetHash(self) }
  }

  #[inline(always)]
  pub fn type_of<'s>(
    &self,
    scope: &mut HandleScope<'s, ()>,
  ) -> Local<'s, String> {
    unsafe {
      scope.cast_local(|sd| v8__Value__TypeOf(self, sd.get_isolate_ptr()))
    }
    .unwrap()
  }

  /// Utility method that returns human readable representation of the
  /// underlying value.
  pub fn type_repr(&self) -> &'static str {
    if self.is_module_namespace_object() {
      "Module"
    } else if self.is_wasm_module_object() {
      "WASM module"
    } else if self.is_wasm_memory_object() {
      "WASM memory object"
    } else if self.is_proxy() {
      "Proxy"
    } else if self.is_shared_array_buffer() {
      "SharedArrayBuffer"
    } else if self.is_data_view() {
      "DataView"
    } else if self.is_big_uint64_array() {
      "BigUint64Array"
    } else if self.is_big_int64_array() {
      "BigInt64Array"
    } else if self.is_float64_array() {
      "Float64Array"
    } else if self.is_float32_array() {
      "Float32Array"
    } else if self.is_int32_array() {
      "Int32Array"
    } else if self.is_uint32_array() {
      "Uint32Array"
    } else if self.is_int16_array() {
      "Int16Array"
    } else if self.is_uint16_array() {
      "Uint16Array"
    } else if self.is_int8_array() {
      "Int8Array"
    } else if self.is_uint8_clamped_array() {
      "Uint8ClampedArray"
    } else if self.is_uint8_array() {
      "Uint8Array"
    } else if self.is_typed_array() {
      "TypedArray"
    } else if self.is_array_buffer_view() {
      "ArrayBufferView"
    } else if self.is_array_buffer() {
      "ArrayBuffer"
    } else if self.is_weak_set() {
      "WeakSet"
    } else if self.is_weak_map() {
      "WeakMap"
    } else if self.is_set_iterator() {
      "Set Iterator"
    } else if self.is_map_iterator() {
      "Map Iterator"
    } else if self.is_set() {
      "Set"
    } else if self.is_map() {
      "Map"
    } else if self.is_promise() {
      "Promise"
    } else if self.is_generator_function() {
      "Generator function"
    } else if self.is_async_function() {
      "Async function"
    } else if self.is_reg_exp() {
      "RegExp"
    } else if self.is_date() {
      "Date"
    } else if self.is_number() {
      "Number"
    } else if self.is_boolean() {
      "Boolean"
    } else if self.is_big_int() {
      "bigint"
    } else if self.is_array() {
      "array"
    } else if self.is_function() {
      "function"
    } else if self.is_symbol() {
      "symbol"
    } else if self.is_string() {
      "string"
    } else if self.is_null() {
      "null"
    } else if self.is_undefined() {
      "undefined"
    } else {
      "unknown"
    }
  }
}
