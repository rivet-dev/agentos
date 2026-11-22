use crate::ConstructorBehavior;
use crate::Context;
use crate::Function;
use crate::FunctionBuilder;
use crate::FunctionCallback;
use crate::IndexedDefinerCallback;
use crate::IndexedDeleterCallback;
use crate::IndexedGetterCallback;
use crate::IndexedQueryCallback;
use crate::IndexedSetterCallback;
use crate::Local;
use crate::NamedDefinerCallback;
use crate::NamedDeleterCallback;
use crate::NamedGetterCallback;
use crate::NamedGetterCallbackForAccessor;
use crate::NamedQueryCallback;
use crate::NamedSetterCallback;
use crate::NamedSetterCallbackForAccessor;
use crate::Object;
use crate::PropertyAttribute;
use crate::PropertyEnumeratorCallback;
use crate::PropertyHandlerFlags;
use crate::SideEffectType;
use crate::Signature;
use crate::String;
use crate::Value;
pub use crate::binding::v8__Intercepted as Intercepted;
use crate::data::Data;
use crate::data::DictionaryTemplate;
use crate::data::FunctionTemplate;
use crate::data::Name;
use crate::data::ObjectTemplate;
use crate::data::Template;
use crate::fast_api::CFunction;
use crate::isolate::RealIsolate;
use crate::scope::PinScope;
use crate::support::MapFnTo;
use crate::support::int;
use std::convert::TryFrom;
use std::ffi::c_char;
use std::ptr::null;

unsafe extern "C" {
  fn v8__Template__Set(
    this: *const Template,
    key: *const Name,
    value: *const Data,
    attr: PropertyAttribute,
  );
  fn v8__Template__SetIntrinsicDataProperty(
    this: *const Template,
    key: *const Name,
    intrinsic: Intrinsic,
    attr: PropertyAttribute,
  );
  fn v8__Template__SetNativeDataProperty(
    this: *const Template,
    key: *const Name,
    getter: AccessorNameGetterCallback,
    setter: Option<AccessorNameSetterCallback>,
    data_or_null: *const Value,
    attr: PropertyAttribute,
  );
  fn v8__Template__SetLazyDataProperty(
    this: *const Template,
    key: *const Name,
    getter: AccessorNameGetterCallback,
    data_or_null: *const Value,
    attr: PropertyAttribute,
  );

  fn v8__Signature__New(
    isolate: *mut RealIsolate,
    templ: *const FunctionTemplate,
  ) -> *const Signature;
  fn v8__FunctionTemplate__New(
    isolate: *mut RealIsolate,
    callback: Option<FunctionCallback>,
    data_or_null: *const Value,
    signature_or_null: *const Signature,
    length: i32,
    constructor_behavior: ConstructorBehavior,
    side_effect_type: SideEffectType,
    c_functions: *const CFunction,
    c_functions_len: usize,
  ) -> *const FunctionTemplate;
  fn v8__FunctionTemplate__GetFunction(
    this: *const FunctionTemplate,
    context: *const Context,
  ) -> *const Function;
  fn v8__FunctionTemplate__PrototypeTemplate(
    this: *const FunctionTemplate,
  ) -> *const ObjectTemplate;
  fn v8__FunctionTemplate__InstanceTemplate(
    this: *const FunctionTemplate,
  ) -> *const ObjectTemplate;
  fn v8__FunctionTemplate__SetClassName(
    this: *const FunctionTemplate,
    name: *const String,
  );
  fn v8__FunctionTemplate__SetAccessorProperty(
    this: *const FunctionTemplate,
    key: *const Name,
    getter: *const FunctionTemplate,
    setter: *const FunctionTemplate,
    attr: PropertyAttribute,
  );
  fn v8__FunctionTemplate__Inherit(
    this: *const FunctionTemplate,
    parent: *const FunctionTemplate,
  );
  fn v8__FunctionTemplate__ReadOnlyPrototype(this: *const FunctionTemplate);
  fn v8__FunctionTemplate__RemovePrototype(this: *const FunctionTemplate);
  fn v8__FunctionTemplate__HasInstance(
    this: *const FunctionTemplate,
    object: *const Value,
  ) -> bool;

  fn v8__DictionaryTemplate__New(
    isolate: *mut RealIsolate,
    names: *const *const c_char,
    name_lengths: *const usize,
    names_len: usize,
  ) -> *const DictionaryTemplate;
  fn v8__DictionaryTemplate__NewInstance(
    this: *const DictionaryTemplate,
    context: *const Context,
    values: *const *const Value,
    values_len: usize,
  ) -> *const Object;

  fn v8__ObjectTemplate__New(
    isolate: *mut RealIsolate,
    templ: *const FunctionTemplate,
  ) -> *const ObjectTemplate;
  fn v8__ObjectTemplate__NewInstance(
    this: *const ObjectTemplate,
    context: *const Context,
  ) -> *const Object;
  fn v8__ObjectTemplate__InternalFieldCount(this: *const ObjectTemplate)
  -> int;
  fn v8__ObjectTemplate__SetInternalFieldCount(
    this: *const ObjectTemplate,
    value: int,
  );

  fn v8__ObjectTemplate__SetNativeDataProperty(
    this: *const ObjectTemplate,
    key: *const Name,
    getter: AccessorNameGetterCallback,
    setter: Option<AccessorNameSetterCallback>,
    data_or_null: *const Value,
    attr: PropertyAttribute,
  );
  fn v8__ObjectTemplate__SetAccessorProperty(
    this: *const ObjectTemplate,
    key: *const Name,
    getter: *const FunctionTemplate,
    setter: *const FunctionTemplate,
    attr: PropertyAttribute,
  );

  fn v8__ObjectTemplate__SetNamedPropertyHandler(
    this: *const ObjectTemplate,
    getter: Option<NamedPropertyGetterCallback>,
    setter: Option<NamedPropertySetterCallback>,
    query: Option<NamedPropertyQueryCallback>,
    deleter: Option<NamedPropertyDeleterCallback>,
    enumerator: Option<NamedPropertyEnumeratorCallback>,
    definer: Option<NamedPropertyDefinerCallback>,
    descriptor: Option<NamedPropertyDescriptorCallback>,
    data_or_null: *const Value,
    flags: PropertyHandlerFlags,
  );

  fn v8__ObjectTemplate__SetIndexedPropertyHandler(
    this: *const ObjectTemplate,
    getter: Option<IndexedPropertyGetterCallback>,
    setter: Option<IndexedPropertySetterCallback>,
    query: Option<IndexedPropertyQueryCallback>,
    deleter: Option<IndexedPropertyDeleterCallback>,
    enumerator: Option<IndexedPropertyEnumeratorCallback>,
    definer: Option<IndexedPropertyDefinerCallback>,
    descriptor: Option<IndexedPropertyDescriptorCallback>,
    data_or_null: *const Value,
    flags: PropertyHandlerFlags,
  );

  fn v8__ObjectTemplate__SetImmutableProto(this: *const ObjectTemplate);

  fn v8__ObjectTemplate__SetCallAsFunctionHandler(
    this: *const ObjectTemplate,
    callback: FunctionCallback,
    data_or_null: *const Value,
  );
}

pub type AccessorNameGetterCallback = NamedGetterCallbackForAccessor;

/// Note: [ReturnValue] is ignored for accessors.
pub type AccessorNameSetterCallback = NamedSetterCallbackForAccessor;

/// Interceptor for get requests on an object.
///
/// Use [ReturnValue] to set the return value of the intercepted get request. If
/// the property does not exist the callback should not set the result and must
/// not produce side effects.
///
/// See also [ObjectTemplate::set_handler].
pub type NamedPropertyGetterCallback = NamedGetterCallback;

/// Interceptor for set requests on an object.
///
/// Use [ReturnValue] to indicate whether the request was intercepted or not. If
/// the setter successfully intercepts the request, i.e., if the request should
/// not be further executed, call [ReturnValue::set]. If the setter did not
/// intercept the request, i.e., if the request should be handled as if no
/// interceptor is present, do not not call set() and do not produce side
/// effects.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertySetterCallback = NamedSetterCallback;

/// Intercepts all requests that query the attributes of the property, e.g.,
/// getOwnPropertyDescriptor(), propertyIsEnumerable(), and defineProperty().
///
/// Use [ReturnValue::set] to set the property attributes. The value is an
/// integer encoding a [PropertyAttribute]. If the property does not exist the
/// callback should not set the result and must not produce side effects.
///
/// Note: Some functions query the property attributes internally, even though
/// they do not return the attributes. For example, hasOwnProperty() can trigger
/// this interceptor depending on the state of the object.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertyQueryCallback = NamedQueryCallback;

/// Interceptor for delete requests on an object.
///
/// Use [ReturnValue] to indicate whether the request was intercepted or not. If
/// the deleter successfully intercepts the request, i.e., if the request should
/// not be further executed, call [ReturnValue::set] with a boolean value. The
/// value is used as the return value of delete. If the deleter does not
/// intercept the request then it should not set the result and must not produce
/// side effects.
///
/// Note: If you need to mimic the behavior of delete, i.e., throw in strict
/// mode instead of returning false, use
/// [PropertyCallbackArguments::should_throw_on_error] to determine if you are
/// in strict mode.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertyDeleterCallback = NamedDeleterCallback;

/// Returns an array containing the names of the properties the named property getter intercepts.
///
/// Note: The values in the array must be of type v8::Name.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertyEnumeratorCallback = PropertyEnumeratorCallback;

/// Interceptor for defineProperty requests on an object.
///
/// Use [ReturnValue] to indicate whether the request was intercepted or not. If
/// the definer successfully intercepts the request, i.e., if the request should
/// not be further executed, call [ReturnValue::set]. If the definer did not
/// intercept the request, i.e., if the request should be handled as if no
/// interceptor is present, do not not call set() and do not produce side
/// effects.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertyDefinerCallback = NamedDefinerCallback;

/// Interceptor for getOwnPropertyDescriptor requests on an object.
///
/// Use [ReturnValue::set] to set the return value of the intercepted request.
/// The return value must be an object that can be converted to a
/// [PropertyDescriptor], e.g., a [Value] returned from
/// `Object.getOwnPropertyDescriptor()`.
///
/// Note: If GetOwnPropertyDescriptor is intercepted, it will always return
/// true, i.e., indicate that the property was found.
///
/// See also [ObjectTemplate::set_named_property_handler].
pub type NamedPropertyDescriptorCallback = NamedGetterCallback;

/// See [GenericNamedPropertyGetterCallback].
pub type IndexedPropertyGetterCallback = IndexedGetterCallback;

/// See [GenericNamedPropertySetterCallback].
pub type IndexedPropertySetterCallback = IndexedSetterCallback;

/// See [GenericNamedPropertyQueryCallback].
pub type IndexedPropertyQueryCallback = IndexedQueryCallback;

/// See [GenericNamedPropertyDeleterCallback].
pub type IndexedPropertyDeleterCallback = IndexedDeleterCallback;

/// See [GenericNamedPropertyEnumeratorCallback].
pub type IndexedPropertyEnumeratorCallback = PropertyEnumeratorCallback;

/// See [GenericNamedPropertyDefinerCallback].
pub type IndexedPropertyDefinerCallback = IndexedDefinerCallback;

/// See [GenericNamedPropertyDescriptorCallback].
pub type IndexedPropertyDescriptorCallback = IndexedGetterCallback;

pub struct AccessorConfiguration<'s> {
  pub(crate) getter: AccessorNameGetterCallback,
  pub(crate) setter: Option<AccessorNameSetterCallback>,
  pub(crate) data: Option<Local<'s, Value>>,
  pub(crate) property_attribute: PropertyAttribute,
}

impl<'s> AccessorConfiguration<'s> {
  pub fn new(getter: impl MapFnTo<AccessorNameGetterCallback>) -> Self {
    Self {
      getter: getter.map_fn_to(),
      setter: None,
      data: None,
      property_attribute: PropertyAttribute::NONE,
    }
  }

  pub fn setter(
    mut self,
    setter: impl MapFnTo<AccessorNameSetterCallback>,
  ) -> Self {
    self.setter = Some(setter.map_fn_to());
    self
  }

  pub fn property_attribute(
    mut self,
    property_attribute: PropertyAttribute,
  ) -> Self {
    self.property_attribute = property_attribute;
    self
  }

  /// Set the associated data. The default is no associated data.
  pub fn data(mut self, data: Local<'s, Value>) -> Self {
    self.data = Some(data);
    self
  }
}

#[derive(Default)]
pub struct NamedPropertyHandlerConfiguration<'s> {
  pub(crate) getter: Option<NamedPropertyGetterCallback>,
  pub(crate) setter: Option<NamedPropertySetterCallback>,
  pub(crate) query: Option<NamedPropertyQueryCallback>,
  pub(crate) deleter: Option<NamedPropertyDeleterCallback>,
  pub(crate) enumerator: Option<NamedPropertyEnumeratorCallback>,
  pub(crate) definer: Option<NamedPropertyDefinerCallback>,
  pub(crate) descriptor: Option<NamedPropertyDescriptorCallback>,
  pub(crate) data: Option<Local<'s, Value>>,
  pub(crate) flags: PropertyHandlerFlags,
}

impl<'s> NamedPropertyHandlerConfiguration<'s> {
  pub fn new() -> Self {
    Self {
      getter: None,
      setter: None,
      query: None,
      deleter: None,
      enumerator: None,
      definer: None,
      descriptor: None,
      data: None,
      flags: PropertyHandlerFlags::NONE,
    }
  }

  pub fn is_some(&self) -> bool {
    self.getter.is_some()
      || self.setter.is_some()
      || self.query.is_some()
      || self.deleter.is_some()
      || self.enumerator.is_some()
      || self.definer.is_some()
      || self.descriptor.is_some()
      || !self.flags.is_none()
  }

  pub fn getter(
    mut self,
    getter: impl MapFnTo<NamedPropertyGetterCallback>,
  ) -> Self {
    self.getter = Some(getter.map_fn_to());
    self
  }

  pub fn getter_raw(mut self, getter: NamedPropertyGetterCallback) -> Self {
    self.getter = Some(getter);
    self
  }

  pub fn setter(
    mut self,
    setter: impl MapFnTo<NamedPropertySetterCallback>,
  ) -> Self {
    self.setter = Some(setter.map_fn_to());
    self
  }

  pub fn setter_raw(mut self, setter: NamedPropertySetterCallback) -> Self {
    self.setter = Some(setter);
    self
  }

  pub fn query(
    mut self,
    query: impl MapFnTo<NamedPropertyQueryCallback>,
  ) -> Self {
    self.query = Some(query.map_fn_to());
    self
  }

  pub fn query_raw(mut self, query: NamedPropertyQueryCallback) -> Self {
    self.query = Some(query);
    self
  }

  pub fn deleter(
    mut self,
    deleter: impl MapFnTo<NamedPropertyDeleterCallback>,
  ) -> Self {
    self.deleter = Some(deleter.map_fn_to());
    self
  }

  pub fn deleter_raw(mut self, deleter: NamedPropertyDeleterCallback) -> Self {
    self.deleter = Some(deleter);
    self
  }

  pub fn enumerator(
    mut self,
    enumerator: impl MapFnTo<NamedPropertyEnumeratorCallback>,
  ) -> Self {
    self.enumerator = Some(enumerator.map_fn_to());
    self
  }

  pub fn enumerator_raw(
    mut self,
    enumerator: NamedPropertyEnumeratorCallback,
  ) -> Self {
    self.enumerator = Some(enumerator);
    self
  }

  pub fn definer(
    mut self,
    definer: impl MapFnTo<NamedPropertyDefinerCallback>,
  ) -> Self {
    self.definer = Some(definer.map_fn_to());
    self
  }

  pub fn definer_raw(mut self, definer: NamedPropertyDefinerCallback) -> Self {
    self.definer = Some(definer);
    self
  }

  pub fn descriptor(
    mut self,
    descriptor: impl MapFnTo<NamedPropertyDescriptorCallback>,
  ) -> Self {
    self.descriptor = Some(descriptor.map_fn_to());
    self
  }

  pub fn descriptor_raw(
    mut self,
    descriptor: NamedPropertyDescriptorCallback,
  ) -> Self {
    self.descriptor = Some(descriptor);
    self
  }

  /// Set the associated data. The default is no associated data.
  pub fn data(mut self, data: Local<'s, Value>) -> Self {
    self.data = Some(data);
    self
  }

  /// Set the property handler flags. The default is PropertyHandlerFlags::NONE.
  pub fn flags(mut self, flags: PropertyHandlerFlags) -> Self {
    self.flags = flags;
    self
  }
}

#[derive(Default)]
pub struct IndexedPropertyHandlerConfiguration<'s> {
  pub(crate) getter: Option<IndexedPropertyGetterCallback>,
  pub(crate) setter: Option<IndexedPropertySetterCallback>,
  pub(crate) query: Option<IndexedPropertyQueryCallback>,
  pub(crate) deleter: Option<IndexedPropertyDeleterCallback>,
  pub(crate) enumerator: Option<IndexedPropertyEnumeratorCallback>,
  pub(crate) definer: Option<IndexedPropertyDefinerCallback>,
  pub(crate) descriptor: Option<IndexedPropertyDescriptorCallback>,
  pub(crate) data: Option<Local<'s, Value>>,
  pub(crate) flags: PropertyHandlerFlags,
}

impl<'s> IndexedPropertyHandlerConfiguration<'s> {
  pub fn new() -> Self {
    Self {
      getter: None,
      setter: None,
      query: None,
      deleter: None,
      enumerator: None,
      definer: None,
      descriptor: None,
      data: None,
      flags: PropertyHandlerFlags::NONE,
    }
  }

  pub fn is_some(&self) -> bool {
    self.getter.is_some()
      || self.setter.is_some()
      || self.query.is_some()
      || self.deleter.is_some()
      || self.enumerator.is_some()
      || self.definer.is_some()
      || self.descriptor.is_some()
      || !self.flags.is_none()
  }

  pub fn getter(
    mut self,
    getter: impl MapFnTo<IndexedPropertyGetterCallback>,
  ) -> Self {
    self.getter = Some(getter.map_fn_to());
    self
  }

  pub fn getter_raw(mut self, getter: IndexedPropertyGetterCallback) -> Self {
    self.getter = Some(getter);
    self
  }

  pub fn setter(
    mut self,
    setter: impl MapFnTo<IndexedPropertySetterCallback>,
  ) -> Self {
    self.setter = Some(setter.map_fn_to());
    self
  }

  pub fn setter_raw(mut self, setter: IndexedPropertySetterCallback) -> Self {
    self.setter = Some(setter);
    self
  }

  pub fn query(
    mut self,
    query: impl MapFnTo<IndexedPropertyQueryCallback>,
  ) -> Self {
    self.query = Some(query.map_fn_to());
    self
  }

  pub fn query_raw(mut self, query: IndexedPropertyQueryCallback) -> Self {
    self.query = Some(query);
    self
  }

  pub fn deleter(
    mut self,
    deleter: impl MapFnTo<IndexedPropertyDeleterCallback>,
  ) -> Self {
    self.deleter = Some(deleter.map_fn_to());
    self
  }

  pub fn deleter_raw(
    mut self,
    deleter: IndexedPropertyDeleterCallback,
  ) -> Self {
    self.deleter = Some(deleter);
    self
  }

  pub fn enumerator(
    mut self,
    enumerator: impl MapFnTo<IndexedPropertyEnumeratorCallback>,
  ) -> Self {
    self.enumerator = Some(enumerator.map_fn_to());
    self
  }

  pub fn enumerator_raw(
    mut self,
    enumerator: IndexedPropertyEnumeratorCallback,
  ) -> Self {
    self.enumerator = Some(enumerator);
    self
  }

  pub fn definer(
    mut self,
    definer: impl MapFnTo<IndexedPropertyDefinerCallback>,
  ) -> Self {
    self.definer = Some(definer.map_fn_to());
    self
  }

  pub fn definer_raw(
    mut self,
    definer: IndexedPropertyDefinerCallback,
  ) -> Self {
    self.definer = Some(definer);
    self
  }

  pub fn descriptor(
    mut self,
    descriptor: impl MapFnTo<IndexedPropertyDescriptorCallback>,
  ) -> Self {
    self.descriptor = Some(descriptor.map_fn_to());
    self
  }

  pub fn descriptor_raw(
    mut self,
    descriptor: IndexedPropertyDescriptorCallback,
  ) -> Self {
    self.descriptor = Some(descriptor);
    self
  }

  /// Set the associated data. The default is no associated data.
  pub fn data(mut self, data: Local<'s, Value>) -> Self {
    self.data = Some(data);
    self
  }

  /// Set the property handler flags. The default is PropertyHandlerFlags::NONE.
  pub fn flags(mut self, flags: PropertyHandlerFlags) -> Self {
    self.flags = flags;
    self
  }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub enum Intrinsic {
  ArrayProtoEntries,
  ArrayProtoForEach,
  ArrayProtoKeys,
  ArrayProtoValues,
  ArrayPrototype,
  AsyncIteratorPrototype,
  ErrorPrototype,
  IteratorPrototype,
  MapIteratorPrototype,
  ObjProtoValueOf,
  SetIteratorPrototype,
}

impl Template {
  /// Adds a property to each instance created by this template.
  #[inline(always)]
  pub fn set(&self, key: Local<Name>, value: Local<Data>) {
    self.set_with_attr(key, value, PropertyAttribute::NONE);
  }

  /// Adds a property to each instance created by this template with
  /// the specified property attributes.
  #[inline(always)]
  pub fn set_with_attr(
    &self,
    key: Local<Name>,
    value: Local<Data>,
    attr: PropertyAttribute,
  ) {
    unsafe { v8__Template__Set(self, &*key, &*value, attr) }
  }

  /// Sets a native data property on every instance created by this template.
  ///
  /// The `getter` callback is invoked whenever the property is read, and the
  /// optional `setter` callback whenever it is written. `data` is passed
  /// through to both callbacks on every invocation.
  ///
  /// Unlike [`Template::set_lazy_data_property`], the callbacks stay installed
  /// and are consulted on every access.
  #[inline(always)]
  pub fn set_native_data_property(
    &self,
    key: Local<Name>,
    configuration: AccessorConfiguration,
  ) {
    unsafe {
      v8__Template__SetNativeDataProperty(
        self,
        &*key,
        configuration.getter,
        configuration.setter,
        configuration.data.map_or_else(null, |p| &*p),
        configuration.property_attribute,
      );
    }
  }

  /// Like [`Template::set_native_data_property`], but V8 replaces the native
  /// data property with a real data property holding the returned value on
  /// first access, so the getter runs at most once per instance.
  #[inline(always)]
  pub fn set_lazy_data_property(
    &self,
    key: Local<Name>,
    getter: impl MapFnTo<AccessorNameGetterCallback>,
    data: Option<Local<Value>>,
    attr: PropertyAttribute,
  ) {
    unsafe {
      v8__Template__SetLazyDataProperty(
        self,
        &*key,
        getter.map_fn_to(),
        data.map_or_else(null, |p| &*p),
        attr,
      );
    }
  }

  /// During template instantiation, sets the value with the
  /// intrinsic property from the correct context.
  #[inline(always)]
  pub fn set_intrinsic_data_property(
    &self,
    key: Local<Name>,
    intrinsic: Intrinsic,
    attr: PropertyAttribute,
  ) {
    unsafe {
      v8__Template__SetIntrinsicDataProperty(self, &*key, intrinsic, attr);
    }
  }
}

impl<'s> FunctionBuilder<'s, FunctionTemplate> {
  /// Create a builder for a [`FunctionTemplate`] that has no call handler.
  ///
  /// This mirrors `v8::FunctionTemplate::New(isolate)` with a null callback,
  /// and is useful when the template only serves to describe a class, i.e.
  /// when only its instance template, prototype template or identity is of
  /// interest.
  #[inline(always)]
  pub fn new_without_callback() -> Self {
    FunctionBuilder::new_raw_optional(None)
  }

  /// Set the function call signature. The default is no signature.
  #[inline(always)]
  pub fn signature(mut self, signature: Local<'s, Signature>) -> Self {
    self.signature = Some(signature);
    self
  }

  /// Creates the function template.
  #[inline(always)]
  pub fn build<'i>(
    self,
    scope: &PinScope<'s, 'i, ()>,
  ) -> Local<'s, FunctionTemplate> {
    unsafe {
      scope.cast_local(|sd| {
        v8__FunctionTemplate__New(
          sd.get_isolate_ptr(),
          self.callback,
          self.data.map_or_else(null, |p| &*p),
          self.signature.map_or_else(null, |p| &*p),
          self.length,
          self.constructor_behavior,
          self.side_effect_type,
          null(),
          0,
        )
      })
    }
    .unwrap()
  }

  /// It's not required to provide `CFunctionInfo` for the overloads - if they
  /// are omitted, then they will be automatically created. In some cases it is
  /// useful to pass them explicitly - eg. when you are snapshotting you'd provide
  /// the overloads and `CFunctionInfo` that would be placed in the external
  /// references array.
  ///
  /// # Lifetime invariant
  ///
  /// `overloads` is `&'static [CFunction]` because, since
  /// [crrev.com/c/7828135], V8 stores the raw `v8::CFunction` pointers it
  /// receives via `NewWithCFunctionOverloads` directly inside
  /// `FunctionTemplateInfo` (rather than copying them into a managed heap
  /// object). The pointed-to storage must therefore outlive every
  /// `FunctionTemplate` that references it. A `FunctionTemplate` may live
  /// until isolate disposal, so in practice this requires the slice (and its
  /// elements, including the `CFunctionInfo`s they reference) to have
  /// `'static` storage. Use a `const` slice such as
  ///
  /// ```ignore
  /// const OVERLOADS: &[v8::fast_api::CFunction] = &[FAST_TEST];
  /// ```
  ///
  /// or a `static` item; do **not** synthesize the slice from stack-local
  /// data.
  ///
  /// [crrev.com/c/7828135]: https://chromium-review.googlesource.com/c/v8/v8/+/7828135
  pub fn build_fast<'i>(
    self,
    scope: &PinScope<'s, 'i>,
    overloads: &'static [CFunction],
  ) -> Local<'s, FunctionTemplate> {
    unsafe {
      scope.cast_local(|sd| {
        v8__FunctionTemplate__New(
          sd.get_isolate_ptr(),
          self.callback,
          self.data.map_or_else(null, |p| &*p),
          self.signature.map_or_else(null, |p| &*p),
          self.length,
          ConstructorBehavior::Throw,
          self.side_effect_type,
          overloads.as_ptr(),
          overloads.len(),
        )
      })
    }
    .unwrap()
  }
}

/// A Signature specifies which receiver is valid for a function.
///
/// A receiver matches a given signature if the receiver (or any of its
/// hidden prototypes) was created from the signature's FunctionTemplate, or
/// from a FunctionTemplate that inherits directly or indirectly from the
/// signature's FunctionTemplate.
impl Signature {
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    templ: Local<FunctionTemplate>,
  ) -> Local<'s, Self> {
    unsafe {
      scope.cast_local(|sd| v8__Signature__New(sd.get_isolate_ptr(), &*templ))
    }
    .unwrap()
  }
}

impl FunctionTemplate {
  /// Creates a function template without a call handler.
  ///
  /// See [`FunctionBuilder::new_without_callback`].
  #[inline(always)]
  pub fn new_without_callback<'s>(
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, FunctionTemplate> {
    FunctionBuilder::<FunctionTemplate>::new_without_callback().build(scope)
  }

  /// Create a FunctionBuilder to configure a FunctionTemplate.
  /// This is the same as FunctionBuilder::<FunctionTemplate>::new().
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

  /// Creates a function template.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    callback: impl MapFnTo<FunctionCallback>,
  ) -> Local<'s, FunctionTemplate> {
    Self::builder(callback).build(scope)
  }

  #[inline(always)]
  pub fn new_raw<'s>(
    scope: &PinScope<'s, '_, ()>,
    callback: FunctionCallback,
  ) -> Local<'s, FunctionTemplate> {
    Self::builder_raw(callback).build(scope)
  }

  /// Returns the unique function instance in the current execution context.
  #[inline(always)]
  pub fn get_function<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Option<Local<'s, Function>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__FunctionTemplate__GetFunction(self, sd.get_current_context())
      })
    }
  }

  /// Set the class name of the FunctionTemplate. This is used for
  /// printing objects created with the function created from the
  /// FunctionTemplate as its constructor.
  #[inline(always)]
  pub fn set_class_name(&self, name: Local<String>) {
    unsafe { v8__FunctionTemplate__SetClassName(self, &*name) };
  }

  /// Returns the ObjectTemplate that is used by this
  /// FunctionTemplate as a PrototypeTemplate
  #[inline(always)]
  pub fn prototype_template<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, ObjectTemplate> {
    unsafe {
      scope.cast_local(|_sd| v8__FunctionTemplate__PrototypeTemplate(self))
    }
    .unwrap()
  }

  /// Returns the object template that is used for instances created when this function
  /// template is called as a constructor.
  #[inline(always)]
  pub fn instance_template<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, ObjectTemplate> {
    unsafe {
      scope.cast_local(|_sd| v8__FunctionTemplate__InstanceTemplate(self))
    }
    .unwrap()
  }

  /// Causes the function template to inherit from a parent function template.
  /// This means the function's prototype.__proto__ is set to the parent function's prototype.
  #[inline(always)]
  pub fn inherit(&self, parent: Local<FunctionTemplate>) {
    unsafe { v8__FunctionTemplate__Inherit(self, &*parent) };
  }

  /// Sets the ReadOnly flag in the attributes of the 'prototype' property
  /// of functions created from this FunctionTemplate to true.
  #[inline(always)]
  pub fn read_only_prototype(&self) {
    unsafe { v8__FunctionTemplate__ReadOnlyPrototype(self) };
  }

  /// Removes the prototype property from functions created from this FunctionTemplate.
  #[inline(always)]
  pub fn remove_prototype(&self) {
    unsafe { v8__FunctionTemplate__RemovePrototype(self) };
  }

  /// Sets an [accessor property](https://tc39.es/ecma262/#sec-property-attributes)
  /// on the function template (i.e. a static accessor on the constructor).
  ///
  /// # Panics
  ///
  /// Panics if both `getter` and `setter` are `None`.
  #[inline(always)]
  pub fn set_accessor_property(
    &self,
    key: Local<Name>,
    getter: Option<Local<FunctionTemplate>>,
    setter: Option<Local<FunctionTemplate>>,
    attr: PropertyAttribute,
  ) {
    assert!(getter.is_some() || setter.is_some());

    unsafe {
      let getter = getter.map_or_else(std::ptr::null, |v| &*v);
      let setter = setter.map_or_else(std::ptr::null, |v| &*v);
      v8__FunctionTemplate__SetAccessorProperty(
        self, &*key, getter, setter, attr,
      );
    }
  }

  /// Returns true if the given object is an instance of this function
  /// template.
  #[inline(always)]
  pub fn has_instance(&self, object: Local<Value>) -> bool {
    unsafe { v8__FunctionTemplate__HasInstance(self, &*object) }
  }
}

impl DictionaryTemplate {
  /// Creates a new template, declaring the data properties that can be passed
  /// on instantiation of the template. Properties can only be declared on
  /// construction and are then immutable. Their values are passed when
  /// creating an object via [`DictionaryTemplate::new_instance`].
  ///
  /// # Panics
  ///
  /// Since V8 15.8 the declared names must be ASCII; V8 internalizes them
  /// as one-byte strings and debug builds of V8 abort on any other byte.
  /// This function panics on a name containing a non-ASCII character.
  ///
  /// V8 also rejects — by aborting the process — any name that is a canonical
  /// array index, such as `"0"`.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    names: &[&str],
  ) -> Local<'s, DictionaryTemplate> {
    for name in names {
      if let Some(c) = name.chars().find(|c| !c.is_ascii()) {
        panic!(
          "DictionaryTemplate property name {name:?} contains U+{:04X}; \
           V8 only accepts ASCII names through this API",
          u32::from(c)
        );
      }
    }
    let bytes: Vec<&[u8]> = names.iter().map(|name| name.as_bytes()).collect();
    let ptrs: Vec<*const c_char> =
      bytes.iter().map(|n| n.as_ptr().cast()).collect();
    let lengths: Vec<usize> = bytes.iter().map(|n| n.len()).collect();
    unsafe {
      scope.cast_local(|sd| {
        v8__DictionaryTemplate__New(
          sd.get_isolate_ptr(),
          ptrs.as_ptr(),
          lengths.as_ptr(),
          names.len(),
        )
      })
    }
    .unwrap()
  }

  /// Creates a new instance of this template.
  ///
  /// `values` supplies the values of the properties declared when the template
  /// was created, in declaration order. A `None` entry signals a non-existent
  /// property, which is left out of the resulting object.
  ///
  /// `values` must have exactly one entry per declared property; V8 aborts the
  /// process if it does not.
  #[inline(always)]
  pub fn new_instance<'s>(
    &self,
    scope: &PinScope<'s, '_>,
    values: &[Option<Local<Value>>],
  ) -> Option<Local<'s, Object>> {
    let values: Vec<*const Value> = values
      .iter()
      .map(|v| v.map_or_else(null, |v| &*v as _))
      .collect();
    unsafe {
      scope.cast_local(|sd| {
        v8__DictionaryTemplate__NewInstance(
          self,
          sd.get_current_context(),
          values.as_ptr(),
          values.len(),
        )
      })
    }
  }
}

impl ObjectTemplate {
  /// Creates an object template.
  #[inline(always)]
  pub fn new<'s>(scope: &PinScope<'s, '_, ()>) -> Local<'s, ObjectTemplate> {
    unsafe {
      scope.cast_local(|sd| {
        v8__ObjectTemplate__New(sd.get_isolate_ptr(), std::ptr::null())
      })
    }
    .unwrap()
  }

  /// Creates an object template from a function template.
  #[inline(always)]
  pub fn new_from_template<'s>(
    scope: &PinScope<'s, '_, ()>,
    templ: Local<FunctionTemplate>,
  ) -> Local<'s, ObjectTemplate> {
    unsafe {
      scope
        .cast_local(|sd| v8__ObjectTemplate__New(sd.get_isolate_ptr(), &*templ))
    }
    .unwrap()
  }

  /// Creates a new instance of this object template.
  #[inline(always)]
  pub fn new_instance<'s>(
    &self,
    scope: &PinScope<'s, '_>,
  ) -> Option<Local<'s, Object>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__ObjectTemplate__NewInstance(self, sd.get_current_context())
      })
    }
  }

  /// Gets the number of internal fields for objects generated from
  /// this template.
  #[inline(always)]
  pub fn internal_field_count(&self) -> usize {
    let count = unsafe { v8__ObjectTemplate__InternalFieldCount(self) };
    usize::try_from(count).expect("bad internal field count") // Can't happen.
  }

  /// Sets the number of internal fields for objects generated from
  /// this template.
  #[inline(always)]
  pub fn set_internal_field_count(&self, value: usize) -> bool {
    // The C++ API takes an i32 but trying to set a value < 0
    // results in unpredictable behavior, hence we disallow it.
    match int::try_from(value) {
      Err(_) => false,
      Ok(value) => {
        unsafe { v8__ObjectTemplate__SetInternalFieldCount(self, value) };
        true
      }
    }
  }

  #[inline(always)]
  pub fn set_accessor(
    &self,
    key: Local<Name>,
    getter: impl MapFnTo<AccessorNameGetterCallback>,
  ) {
    self
      .set_accessor_with_configuration(key, AccessorConfiguration::new(getter));
  }

  #[inline(always)]
  pub fn set_accessor_with_setter(
    &self,
    key: Local<Name>,
    getter: impl MapFnTo<AccessorNameGetterCallback>,
    setter: impl MapFnTo<AccessorNameSetterCallback>,
  ) {
    self.set_accessor_with_configuration(
      key,
      AccessorConfiguration::new(getter).setter(setter),
    );
  }

  #[inline(always)]
  pub fn set_accessor_with_configuration(
    &self,
    key: Local<Name>,
    configuration: AccessorConfiguration,
  ) {
    unsafe {
      v8__ObjectTemplate__SetNativeDataProperty(
        self,
        &*key,
        configuration.getter,
        configuration.setter,
        configuration.data.map_or_else(null, |p| &*p),
        configuration.property_attribute,
      );
    }
  }

  //Re uses the AccessorNameGetterCallback to avoid implementation conflicts since the declaration for
  //GenericNamedPropertyGetterCallback and  AccessorNameGetterCallback are the same
  pub fn set_named_property_handler(
    &self,
    configuration: NamedPropertyHandlerConfiguration,
  ) {
    assert!(configuration.is_some());
    unsafe {
      v8__ObjectTemplate__SetNamedPropertyHandler(
        self,
        configuration.getter,
        configuration.setter,
        configuration.query,
        configuration.deleter,
        configuration.enumerator,
        configuration.definer,
        configuration.descriptor,
        configuration.data.map_or_else(null, |p| &*p),
        configuration.flags,
      );
    }
  }

  pub fn set_indexed_property_handler(
    &self,
    configuration: IndexedPropertyHandlerConfiguration,
  ) {
    assert!(configuration.is_some());
    unsafe {
      v8__ObjectTemplate__SetIndexedPropertyHandler(
        self,
        configuration.getter,
        configuration.setter,
        configuration.query,
        configuration.deleter,
        configuration.enumerator,
        configuration.definer,
        configuration.descriptor,
        configuration.data.map_or_else(null, |p| &*p),
        configuration.flags,
      );
    }
  }

  /// Sets an [accessor property](https://tc39.es/ecma262/#sec-property-attributes)
  /// on the object template.
  ///
  /// # Panics
  ///
  /// Panics if both `getter` and `setter` are `None`.
  #[inline(always)]
  pub fn set_accessor_property(
    &self,
    key: Local<Name>,
    getter: Option<Local<FunctionTemplate>>,
    setter: Option<Local<FunctionTemplate>>,
    attr: PropertyAttribute,
  ) {
    assert!(getter.is_some() || setter.is_some());

    unsafe {
      let getter = getter.map_or_else(std::ptr::null, |v| &*v);
      let setter = setter.map_or_else(std::ptr::null, |v| &*v);
      v8__ObjectTemplate__SetAccessorProperty(
        self, &*key, getter, setter, attr,
      );
    }
  }

  /// Makes the ObjectTemplate for an immutable prototype exotic object,
  /// with an immutable proto.
  #[inline(always)]
  pub fn set_immutable_proto(&self) {
    unsafe { v8__ObjectTemplate__SetImmutableProto(self) };
  }

  /// Sets the callback to be used when calling instances created from this
  /// template as a function.
  ///
  /// If no callback is set, instances behave like normal [`Object`]s that
  /// cannot be called like [`Function`]s.
  #[inline(always)]
  pub fn set_call_as_function_handler(
    &self,
    callback: impl MapFnTo<FunctionCallback>,
    data: Option<Local<'_, Value>>,
  ) {
    unsafe {
      v8__ObjectTemplate__SetCallAsFunctionHandler(
        self,
        callback.map_fn_to(),
        data.map_or_else(null, |p| &*p),
      );
    }
  }
}
