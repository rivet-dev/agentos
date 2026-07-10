use crate::AccessorConfiguration;
use crate::AccessorNameGetterCallback;
use crate::AccessorNameSetterCallback;
use crate::Array;
use crate::Context;
use crate::Data;
use crate::GetPropertyNamesArgs;
use crate::HandleScope;
use crate::IndexFilter;
use crate::KeyCollectionMode;
use crate::KeyConversionMode;
use crate::Local;
use crate::Map;
use crate::Name;
use crate::Object;
use crate::Private;
use crate::PropertyAttribute;
use crate::PropertyDescriptor;
use crate::PropertyFilter;
use crate::Set;
use crate::String;
use crate::Value;
use crate::binding::RustObj;
use crate::cppgc::GarbageCollected;
use crate::cppgc::GetRustObj;
use crate::cppgc::Ptr;
use crate::isolate::Isolate;
use crate::support::MapFnTo;
use crate::support::Maybe;
use crate::support::MaybeBool;
use crate::support::int;
use std::convert::TryFrom;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::num::NonZeroI32;
use std::ptr::null;

unsafe extern "C" {
  fn v8__Object__New(isolate: *mut Isolate) -> *const Object;
  fn v8__Object__New__with_prototype_and_properties(
    isolate: *mut Isolate,
    prototype_or_null: *const Value,
    names: *const *const Name,
    values: *const *const Value,
    length: usize,
  ) -> *const Object;
  fn v8__Object__SetAccessor(
    this: *const Object,
    context: *const Context,
    key: *const Name,
    getter: AccessorNameGetterCallback,
    setter: Option<AccessorNameSetterCallback>,
    data_or_null: *const Value,
    attr: PropertyAttribute,
  ) -> MaybeBool;
  fn v8__Object__Get(
    this: *const Object,
    context: *const Context,
    key: *const Value,
  ) -> *const Value;
  fn v8__Object__GetWithReceiver(
    this: *const Object,
    context: *const Context,
    key: *const Value,
    receiver: *const Object,
  ) -> *const Value;
  fn v8__Object__GetIndex(
    this: *const Object,
    context: *const Context,
    index: u32,
  ) -> *const Value;
  fn v8__Object__GetPrototype(this: *const Object) -> *const Value;
  fn v8__Object__Set(
    this: *const Object,
    context: *const Context,
    key: *const Value,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Object__SetWithReceiver(
    this: *const Object,
    context: *const Context,
    key: *const Value,
    value: *const Value,
    receiver: *const Object,
  ) -> MaybeBool;
  fn v8__Object__SetIndex(
    this: *const Object,
    context: *const Context,
    index: u32,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Object__SetPrototype(
    this: *const Object,
    context: *const Context,
    prototype: *const Value,
  ) -> MaybeBool;
  fn v8__Object__GetConstructorName(this: *const Object) -> *const String;
  fn v8__Object__CreateDataProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Object__DefineOwnProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
    value: *const Value,
    attr: PropertyAttribute,
  ) -> MaybeBool;
  fn v8__Object__DefineProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
    desc: *const PropertyDescriptor,
  ) -> MaybeBool;
  fn v8__Object__GetIdentityHash(this: *const Object) -> int;
  fn v8__Object__GetCreationContext(this: *const Object) -> *const Context;
  fn v8__Object__GetOwnPropertyNames(
    this: *const Object,
    context: *const Context,
    filter: PropertyFilter,
    key_conversion: KeyConversionMode,
  ) -> *const Array;
  fn v8__Object__GetPropertyNames(
    this: *const Object,
    context: *const Context,
    mode: KeyCollectionMode,
    property_filter: PropertyFilter,
    index_filter: IndexFilter,
    key_conversion: KeyConversionMode,
  ) -> *const Array;
  fn v8__Object__Has(
    this: *const Object,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Object__HasIndex(
    this: *const Object,
    context: *const Context,
    index: u32,
  ) -> MaybeBool;
  fn v8__Object__HasOwnProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
  ) -> MaybeBool;
  fn v8__Object__Delete(
    this: *const Object,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Object__DeleteIndex(
    this: *const Object,
    context: *const Context,
    index: u32,
  ) -> MaybeBool;
  fn v8__Object__InternalFieldCount(this: *const Object) -> int;
  fn v8__Object__GetInternalField(
    this: *const Object,
    index: int,
  ) -> *const Data;
  fn v8__Object__GetAlignedPointerFromInternalField(
    this: *const Object,
    index: int,
  ) -> *const c_void;
  fn v8__Object__SetAlignedPointerInInternalField(
    this: *const Object,
    index: int,
    value: *const c_void,
  );
  fn v8__Object__SetIntegrityLevel(
    this: *const Object,
    context: *const Context,
    level: IntegrityLevel,
  ) -> MaybeBool;
  fn v8__Object__SetInternalField(
    this: *const Object,
    index: int,
    data: *const Data,
  );
  fn v8__Object__GetPrivate(
    this: *const Object,
    context: *const Context,
    key: *const Private,
  ) -> *const Value;
  fn v8__Object__SetPrivate(
    this: *const Object,
    context: *const Context,
    key: *const Private,
    value: *const Value,
  ) -> MaybeBool;
  fn v8__Object__DeletePrivate(
    this: *const Object,
    context: *const Context,
    key: *const Private,
  ) -> MaybeBool;
  fn v8__Object__HasPrivate(
    this: *const Object,
    context: *const Context,
    key: *const Private,
  ) -> MaybeBool;
  fn v8__Object__GetPropertyAttributes(
    this: *const Object,
    context: *const Context,
    key: *const Value,
    out: *mut Maybe<PropertyAttribute>,
  );
  fn v8__Object__GetOwnPropertyDescriptor(
    this: *const Object,
    context: *const Context,
    key: *const Name,
  ) -> *const Value;
  fn v8__Object__PreviewEntries(
    this: *const Object,
    is_key_value: *mut bool,
  ) -> *const Array;
  fn v8__Object__GetRealNamedProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
  ) -> *const Value;
  fn v8__Object__HasRealNamedProperty(
    this: *const Object,
    context: *const Context,
    key: *const Name,
  ) -> MaybeBool;
  fn v8__Object__GetRealNamedPropertyAttributes(
    this: *const Object,
    context: *const Context,
    key: *const Name,
    out: *mut Maybe<PropertyAttribute>,
  );
  fn v8__Object__Wrap(
    isolate: *const Isolate,
    wrapper: *const Object,
    value: *const RustObj,
    tag: u16,
  );
  fn v8__Object__Unwrap(
    isolate: *const Isolate,
    wrapper: *const Object,
    tag: u16,
  ) -> *mut RustObj;
  fn v8__Object__IsApiWrapper(this: *const Object) -> bool;

  fn v8__Array__New(isolate: *mut Isolate, length: int) -> *const Array;
  fn v8__Array__New_with_elements(
    isolate: *mut Isolate,
    elements: *const *const Value,
    length: usize,
  ) -> *const Array;
  fn v8__Array__Length(array: *const Array) -> u32;
  fn v8__Map__New(isolate: *mut Isolate) -> *const Map;
  fn v8__Map__Clear(this: *const Map);
  fn v8__Map__Get(
    this: *const Map,
    context: *const Context,
    key: *const Value,
  ) -> *const Value;
  fn v8__Map__Set(
    this: *const Map,
    context: *const Context,
    key: *const Value,
    value: *const Value,
  ) -> *const Map;
  fn v8__Map__Has(
    this: *const Map,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Map__Delete(
    this: *const Map,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Map__Size(map: *const Map) -> usize;
  fn v8__Map__As__Array(this: *const Map) -> *const Array;
  fn v8__Set__New(isolate: *mut Isolate) -> *const Set;
  fn v8__Set__Clear(this: *const Set);
  fn v8__Set__Add(
    this: *const Set,
    context: *const Context,
    key: *const Value,
  ) -> *const Set;
  fn v8__Set__Has(
    this: *const Set,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Set__Delete(
    this: *const Set,
    context: *const Context,
    key: *const Value,
  ) -> MaybeBool;
  fn v8__Set__Size(map: *const Set) -> usize;
  fn v8__Set__As__Array(this: *const Set) -> *const Array;
}

const LAST_TAG: u16 = 0x7fff;

impl Object {
  /// Creates an empty object.
  #[inline(always)]
  pub fn new<'s>(scope: &mut HandleScope<'s>) -> Local<'s, Object> {
    unsafe { scope.cast_local(|sd| v8__Object__New(sd.get_isolate_ptr())) }
      .unwrap()
  }

  /// Creates a JavaScript object with the given properties, and the given
  /// prototype_or_null (which can be any JavaScript value, and if it's null,
  /// the newly created object won't have a prototype at all). This is similar
  /// to Object.create(). All properties will be created as enumerable,
  /// configurable and writable properties.
  #[inline(always)]
  pub fn with_prototype_and_properties<'s>(
    scope: &mut HandleScope<'s>,
    prototype_or_null: Local<'s, Value>,
    names: &[Local<Name>],
    values: &[Local<Value>],
  ) -> Local<'s, Object> {
    assert_eq!(names.len(), values.len());
    let names = Local::slice_into_raw(names);
    let values = Local::slice_into_raw(values);
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__New__with_prototype_and_properties(
          sd.get_isolate_ptr(),
          &*prototype_or_null,
          names.as_ptr(),
          values.as_ptr(),
          names.len(),
        )
      })
    }
    .unwrap()
  }

  /// Set only return Just(true) or Empty(), so if it should never fail, use
  /// result.Check().
  #[inline(always)]
  pub fn set(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
    value: Local<Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__Set(self, &*scope.get_current_context(), &*key, &*value)
    }
    .into()
  }

  /// SetWithReceiver only return Just(true) or Empty(), so if it should never fail, use
  /// result.Check().
  #[inline(always)]
  pub fn set_with_receiver(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
    value: Local<Value>,
    receiver: Local<Object>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetWithReceiver(
        self,
        &*scope.get_current_context(),
        &*key,
        &*value,
        &*receiver,
      )
    }
    .into()
  }

  /// Set only return Just(true) or Empty(), so if it should never fail, use
  /// result.Check().
  #[inline(always)]
  pub fn set_index(
    &self,
    scope: &mut HandleScope,
    index: u32,
    value: Local<Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetIndex(self, &*scope.get_current_context(), index, &*value)
    }
    .into()
  }

  /// Set the prototype object. This does not skip objects marked to be
  /// skipped by proto and it does not consult the security handler.
  #[inline(always)]
  pub fn set_prototype(
    &self,
    scope: &mut HandleScope,
    prototype: Local<Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetPrototype(self, &*scope.get_current_context(), &*prototype)
    }
    .into()
  }

  /// Returns the name of the function invoked as a constructor for this object.
  #[inline(always)]
  pub fn get_constructor_name(&self) -> Local<String> {
    unsafe { Local::from_raw(v8__Object__GetConstructorName(self)) }.unwrap()
  }

  /// Implements CreateDataProperty (ECMA-262, 7.3.4).
  ///
  /// Defines a configurable, writable, enumerable property with the given value
  /// on the object unless the property already exists and is not configurable
  /// or the object is not extensible.
  ///
  /// Returns true on success.
  #[inline(always)]
  pub fn create_data_property(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
    value: Local<Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__CreateDataProperty(
        self,
        &*scope.get_current_context(),
        &*key,
        &*value,
      )
    }
    .into()
  }

  /// Implements DefineOwnProperty.
  ///
  /// In general, CreateDataProperty will be faster, however, does not allow
  /// for specifying attributes.
  ///
  /// Returns true on success.
  #[inline(always)]
  pub fn define_own_property(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
    value: Local<Value>,
    attr: PropertyAttribute,
  ) -> Option<bool> {
    unsafe {
      v8__Object__DefineOwnProperty(
        self,
        &*scope.get_current_context(),
        &*key,
        &*value,
        attr,
      )
    }
    .into()
  }

  #[inline(always)]
  pub fn define_property(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
    descriptor: &PropertyDescriptor,
  ) -> Option<bool> {
    unsafe {
      v8__Object__DefineProperty(
        self,
        &*scope.get_current_context(),
        &*key,
        descriptor,
      )
      .into()
    }
  }

  #[inline(always)]
  pub fn get<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Value>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope
        .cast_local(|sd| v8__Object__Get(self, sd.get_current_context(), &*key))
    }
  }

  #[inline(always)]
  pub fn get_with_receiver<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Value>,
    receiver: Local<Object>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetWithReceiver(
          self,
          sd.get_current_context(),
          &*key,
          &*receiver,
        )
      })
    }
  }

  #[inline(always)]
  pub fn get_index<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    index: u32,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetIndex(self, sd.get_current_context(), index)
      })
    }
  }

  /// Get the prototype object. This does not skip objects marked to be
  /// skipped by proto and it does not consult the security handler.
  #[inline(always)]
  pub fn get_prototype<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Value>> {
    unsafe { scope.cast_local(|_| v8__Object__GetPrototype(self)) }
  }

  /// Note: SideEffectType affects the getter only, not the setter.
  #[inline(always)]
  pub fn set_accessor(
    &self,
    scope: &mut HandleScope,
    name: Local<Name>,
    getter: impl for<'s> MapFnTo<AccessorNameGetterCallback<'s>>,
  ) -> Option<bool> {
    self.set_accessor_with_configuration(
      scope,
      name,
      AccessorConfiguration::new(getter),
    )
  }

  #[inline(always)]
  pub fn set_accessor_with_setter(
    &self,
    scope: &mut HandleScope,
    name: Local<Name>,
    getter: impl for<'s> MapFnTo<AccessorNameGetterCallback<'s>>,
    setter: impl for<'s> MapFnTo<AccessorNameSetterCallback<'s>>,
  ) -> Option<bool> {
    self.set_accessor_with_configuration(
      scope,
      name,
      AccessorConfiguration::new(getter).setter(setter),
    )
  }
  #[inline(always)]
  pub fn set_accessor_with_configuration(
    &self,
    scope: &mut HandleScope,
    name: Local<Name>,
    configuration: AccessorConfiguration,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetAccessor(
        self,
        &*scope.get_current_context(),
        &*name,
        configuration.getter,
        configuration.setter,
        configuration.data.map_or_else(null, |p| &*p),
        configuration.property_attribute,
      )
    }
    .into()
  }

  /// Returns the V8 hash value for this value. The current implementation
  /// uses a hidden property to store the identity hash.
  ///
  /// The return value will never be 0. Also, it is not guaranteed to be
  /// unique.
  #[inline(always)]
  pub fn get_identity_hash(&self) -> NonZeroI32 {
    unsafe { NonZeroI32::new_unchecked(v8__Object__GetIdentityHash(self)) }
  }

  /// Returns the context in which the object was created.
  #[inline(always)]
  pub fn get_creation_context<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> Option<Local<'s, Context>> {
    unsafe { scope.cast_local(|_| v8__Object__GetCreationContext(self)) }
  }

  /// This function has the same functionality as GetPropertyNames but the
  /// returned array doesn't contain the names of properties from prototype
  /// objects.
  #[inline(always)]
  pub fn get_own_property_names<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    args: GetPropertyNamesArgs,
  ) -> Option<Local<'s, Array>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetOwnPropertyNames(
          self,
          sd.get_current_context(),
          args.property_filter,
          args.key_conversion,
        )
      })
    }
  }

  /// Returns an array containing the names of the filtered properties of this
  /// object, including properties from prototype objects. The array returned by
  /// this method contains the same values as would be enumerated by a for-in
  /// statement over this object.
  #[inline(always)]
  pub fn get_property_names<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    args: GetPropertyNamesArgs,
  ) -> Option<Local<'s, Array>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetPropertyNames(
          self,
          sd.get_current_context(),
          args.mode,
          args.property_filter,
          args.index_filter,
          args.key_conversion,
        )
      })
    }
  }

  // Calls the abstract operation HasProperty(O, P) described in ECMA-262,
  // 7.3.10. Returns true, if the object has the property, either own or on the
  // prototype chain. Interceptors, i.e., PropertyQueryCallbacks, are called if
  // present.
  //
  // This function has the same side effects as JavaScript's variable in object.
  // For example, calling this on a revoked proxy will throw an exception.
  //
  // Note: This function converts the key to a name, which possibly calls back
  // into JavaScript.
  #[inline(always)]
  pub fn has(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Object__Has(self, &*scope.get_current_context(), &*key) }
      .into()
  }

  #[inline(always)]
  pub fn has_index(&self, scope: &mut HandleScope, index: u32) -> Option<bool> {
    unsafe { v8__Object__HasIndex(self, &*scope.get_current_context(), index) }
      .into()
  }

  /// HasOwnProperty() is like JavaScript's Object.prototype.hasOwnProperty().
  #[inline(always)]
  pub fn has_own_property(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__HasOwnProperty(self, &*scope.get_current_context(), &*key)
    }
    .into()
  }

  #[inline(always)]
  pub fn delete(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Object__Delete(self, &*scope.get_current_context(), &*key) }
      .into()
  }

  pub fn delete_index(
    &self,
    scope: &mut HandleScope,
    index: u32,
  ) -> Option<bool> {
    unsafe {
      v8__Object__DeleteIndex(self, &*scope.get_current_context(), index)
    }
    .into()
  }

  /// Gets the number of internal fields for this Object.
  #[inline(always)]
  pub fn internal_field_count(&self) -> usize {
    let count = unsafe { v8__Object__InternalFieldCount(self) };
    usize::try_from(count).expect("bad internal field count") // Can't happen.
  }

  /// Gets the data from an internal field.
  #[inline(always)]
  pub fn get_internal_field<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    index: usize,
  ) -> Option<Local<'s, Data>> {
    // Trying to access out-of-bounds internal fields makes V8 abort
    // in debug mode and access out-of-bounds memory in release mode.
    // The C++ API takes an i32 but doesn't check for indexes < 0, which
    // results in an out-of-bounds access in both debug and release mode.
    if index < self.internal_field_count() {
      if let Ok(index) = int::try_from(index) {
        return unsafe {
          scope.cast_local(|_| v8__Object__GetInternalField(self, index))
        };
      }
    }
    None
  }

  /// Gets a 2-byte-aligned native pointer from an internal field.
  ///
  /// # Safety
  /// This field must have been set by SetAlignedPointerInInternalField, everything else leads to undefined behavior.
  #[inline(always)]
  pub unsafe fn get_aligned_pointer_from_internal_field(
    &self,
    index: i32,
  ) -> *const c_void {
    unsafe { v8__Object__GetAlignedPointerFromInternalField(self, index) }
  }

  /// Sets a 2-byte-aligned native pointer in an internal field.
  /// To retrieve such a field, GetAlignedPointerFromInternalField must be used.
  #[allow(clippy::not_unsafe_ptr_arg_deref)]
  #[inline(always)]
  pub fn set_aligned_pointer_in_internal_field(
    &self,
    index: i32,
    value: *const c_void,
  ) {
    unsafe { v8__Object__SetAlignedPointerInInternalField(self, index, value) }
  }

  /// Wraps a JS wrapper with a C++ instance.
  ///
  /// # Safety
  ///
  /// The `TAG` must be unique to the caller within the heap.
  #[allow(clippy::not_unsafe_ptr_arg_deref)]
  #[inline(always)]
  pub unsafe fn wrap<const TAG: u16, T: GarbageCollected>(
    isolate: &mut Isolate,
    wrapper: Local<Object>,
    value: &impl GetRustObj<T>,
  ) {
    const {
      assert!(TAG < LAST_TAG);
    }
    let ptr = value.get_rust_obj();
    unsafe { v8__Object__Wrap(isolate as *mut _, &*wrapper, ptr, TAG) }
  }

  /// Unwraps a JS wrapper object.
  ///
  /// # Safety
  ///
  /// The caller must ensure that the returned pointer is always stored on
  /// the stack, or is safely moved into one of the other cppgc pointer types.
  #[inline(always)]
  pub unsafe fn unwrap<const TAG: u16, T: GarbageCollected>(
    isolate: &mut Isolate,
    wrapper: Local<Object>,
  ) -> Option<Ptr<T>> {
    const {
      assert!(TAG < LAST_TAG);
    }
    let ptr = unsafe { v8__Object__Unwrap(isolate as *mut _, &*wrapper, TAG) };
    unsafe { Ptr::new(&ptr) }
  }

  /// Returns true if this object can be generally used to wrap object objects.
  /// This means that the object either follows the convention of using embedder
  /// fields to denote type/instance pointers or is using the Wrap()/Unwrap()
  /// APIs for the same purpose. Returns false otherwise.
  ///
  /// Note that there may be other objects that use embedder fields but are not
  /// used as API wrapper objects. E.g., v8::Promise may in certain configuration
  /// use embedder fields but promises are not generally supported as API
  /// wrappers. The method will return false in those cases.
  #[inline(always)]
  pub fn is_api_wrapper(&self) -> bool {
    unsafe { v8__Object__IsApiWrapper(self) }
  }

  /// Sets the integrity level of the object.
  #[inline(always)]
  pub fn set_integrity_level(
    &self,
    scope: &mut HandleScope,
    level: IntegrityLevel,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetIntegrityLevel(self, &*scope.get_current_context(), level)
    }
    .into()
  }

  /// Sets the data in an internal field. Returns false when the index
  /// is out of bounds, true otherwise.
  #[inline(always)]
  pub fn set_internal_field(&self, index: usize, data: Local<Data>) -> bool {
    // Trying to access out-of-bounds internal fields makes V8 abort
    // in debug mode and access out-of-bounds memory in release mode.
    // The C++ API takes an i32 but doesn't check for indexes < 0, which
    // results in an out-of-bounds access in both debug and release mode.
    if index < self.internal_field_count() {
      if let Ok(index) = int::try_from(index) {
        unsafe { v8__Object__SetInternalField(self, index, &*data) };
        return true;
      }
    }
    false
  }

  /// Functionality for private properties.
  /// This is an experimental feature, use at your own risk.
  /// Note: Private properties are not inherited. Do not rely on this, since it
  /// may change.
  #[inline(always)]
  pub fn get_private<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Private>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetPrivate(self, sd.get_current_context(), &*key)
      })
    }
  }

  /// Functionality for private properties.
  /// This is an experimental feature, use at your own risk.
  /// Note: Private properties are not inherited. Do not rely on this, since it
  /// may change.
  #[inline(always)]
  pub fn set_private(
    &self,
    scope: &mut HandleScope,
    key: Local<Private>,
    value: Local<Value>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__SetPrivate(
        self,
        &*scope.get_current_context(),
        &*key,
        &*value,
      )
    }
    .into()
  }

  /// Functionality for private properties.
  /// This is an experimental feature, use at your own risk.
  /// Note: Private properties are not inherited. Do not rely on this, since it
  /// may change.
  #[inline(always)]
  pub fn delete_private(
    &self,
    scope: &mut HandleScope,
    key: Local<Private>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__DeletePrivate(self, &*scope.get_current_context(), &*key)
    }
    .into()
  }

  /// Functionality for private properties.
  /// This is an experimental feature, use at your own risk.
  /// Note: Private properties are not inherited. Do not rely on this, since it
  /// may change.
  #[inline(always)]
  pub fn has_private(
    &self,
    scope: &mut HandleScope,
    key: Local<Private>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__HasPrivate(self, &*scope.get_current_context(), &*key)
    }
    .into()
  }

  /// Gets the property attributes of a property which can be
  /// [PropertyAttribute::NONE] or any combination of
  /// [PropertyAttribute::READ_ONLY], [PropertyAttribute::DONT_ENUM] and
  /// [PropertyAttribute::DONT_DELETE].
  /// Returns [PropertyAttribute::NONE] when the property doesn't exist.
  pub fn get_property_attributes(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<PropertyAttribute> {
    let mut out = Maybe::<PropertyAttribute>::default();
    unsafe {
      v8__Object__GetPropertyAttributes(
        self,
        &*scope.get_current_context(),
        &*key,
        &mut out,
      );
    };
    out.into()
  }

  /// Implements Object.getOwnPropertyDescriptor(O, P), see
  /// https://tc39.es/ecma262/#sec-object.getownpropertydescriptor.
  pub fn get_own_property_descriptor<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Name>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetOwnPropertyDescriptor(
          self,
          sd.get_current_context(),
          &*key,
        )
      })
    }
  }

  /// If this object is a Set, Map, WeakSet or WeakMap, this returns a
  /// representation of the elements of this object as an array.
  /// If this object is a SetIterator or MapIterator, this returns all elements
  /// of the underlying collection, starting at the iterator's current position.
  ///
  /// Also returns a boolean, indicating whether the returned array contains
  /// key & values (for example when the value is Set.entries()).
  pub fn preview_entries<'s>(
    &self,
    scope: &mut HandleScope<'s>,
  ) -> (Option<Local<'s, Array>>, bool) {
    let mut is_key_value = MaybeUninit::uninit();
    unsafe {
      let val = scope.cast_local(|_| {
        v8__Object__PreviewEntries(self, is_key_value.as_mut_ptr())
      });
      let is_key_value = is_key_value.assume_init();

      (val, is_key_value)
    }
  }

  /// If result.IsEmpty() no real property was located on the object or
  /// in the prototype chain.
  /// This means interceptors in the prototype chain are not called.
  #[inline(always)]
  pub fn get_real_named_property<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Name>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Object__GetRealNamedProperty(self, sd.get_current_context(), &*key)
      })
    }
  }

  #[inline(always)]
  pub fn has_real_named_property(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
  ) -> Option<bool> {
    unsafe {
      v8__Object__HasRealNamedProperty(
        self,
        &*scope.get_current_context(),
        &*key,
      )
    }
    .into()
  }

  /// Gets the property attributes of a real property which can be
  /// None or any combination of ReadOnly, DontEnum and DontDelete.
  /// Interceptors in the prototype chain are not called.
  #[inline(always)]
  pub fn get_real_named_property_attributes(
    &self,
    scope: &mut HandleScope,
    key: Local<Name>,
  ) -> Option<PropertyAttribute> {
    let mut out = Maybe::<PropertyAttribute>::default();
    unsafe {
      v8__Object__GetRealNamedPropertyAttributes(
        self,
        &*scope.get_current_context(),
        &*key,
        &mut out,
      );
    }
    out.into()
  }
}

/// Object integrity levels can be used to restrict what can be done to an
/// object's properties.
#[derive(Debug)]
#[repr(C)]
pub enum IntegrityLevel {
  /// Frozen objects are like Sealed objects, except all existing properties are
  /// also made non-writable.
  Frozen,
  /// Sealed objects prevent addition of any new property on the object, makes
  /// all existing properties non-configurable, meaning they cannot be deleted,
  /// have their enumerability, configurability, or writability changed.
  Sealed,
}

impl Array {
  /// Creates a JavaScript array with the given length. If the length
  /// is negative the returned array will have length 0.
  #[inline(always)]
  pub fn new<'s>(scope: &mut HandleScope<'s>, length: i32) -> Local<'s, Array> {
    unsafe {
      scope.cast_local(|sd| v8__Array__New(sd.get_isolate_ptr(), length))
    }
    .unwrap()
  }

  /// Creates a JavaScript array out of a Local<Value> array with a known
  /// length.
  #[inline(always)]
  pub fn new_with_elements<'s>(
    scope: &mut HandleScope<'s>,
    elements: &[Local<Value>],
  ) -> Local<'s, Array> {
    if elements.is_empty() {
      return Self::new(scope, 0);
    }
    let elements = Local::slice_into_raw(elements);
    unsafe {
      scope.cast_local(|sd| {
        v8__Array__New_with_elements(
          sd.get_isolate_ptr(),
          elements.as_ptr(),
          elements.len(),
        )
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn length(&self) -> u32 {
    unsafe { v8__Array__Length(self) }
  }
}

impl Map {
  #[inline(always)]
  pub fn new<'s>(scope: &mut HandleScope<'s>) -> Local<'s, Map> {
    unsafe { scope.cast_local(|sd| v8__Map__New(sd.get_isolate_ptr())) }
      .unwrap()
  }

  #[inline(always)]
  pub fn size(&self) -> usize {
    unsafe { v8__Map__Size(self) }
  }

  #[inline(always)]
  pub fn clear(&self) {
    unsafe { v8__Map__Clear(self) }
  }

  #[inline(always)]
  pub fn get<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Value>,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|sd| v8__Map__Get(self, sd.get_current_context(), &*key))
    }
  }

  #[inline(always)]
  pub fn set<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Value>,
    value: Local<Value>,
  ) -> Option<Local<'s, Map>> {
    unsafe {
      scope.cast_local(|sd| {
        v8__Map__Set(self, sd.get_current_context(), &*key, &*value)
      })
    }
  }

  #[inline(always)]
  pub fn has(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Map__Has(self, &*scope.get_current_context(), &*key) }.into()
  }

  #[inline(always)]
  pub fn delete(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Map__Delete(self, &*scope.get_current_context(), &*key) }
      .into()
  }

  /// Returns an array of length size() * 2, where index N is the Nth key and
  /// index N + 1 is the Nth value.
  #[inline(always)]
  pub fn as_array<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, Array> {
    unsafe { scope.cast_local(|_| v8__Map__As__Array(self)) }.unwrap()
  }
}

impl Set {
  #[inline(always)]
  pub fn new<'s>(scope: &mut HandleScope<'s>) -> Local<'s, Set> {
    unsafe { scope.cast_local(|sd| v8__Set__New(sd.get_isolate_ptr())) }
      .unwrap()
  }

  #[inline(always)]
  pub fn size(&self) -> usize {
    unsafe { v8__Set__Size(self) }
  }

  #[inline(always)]
  pub fn clear(&self) {
    unsafe { v8__Set__Clear(self) }
  }

  #[inline(always)]
  pub fn add<'s>(
    &self,
    scope: &mut HandleScope<'s>,
    key: Local<Value>,
  ) -> Option<Local<'s, Set>> {
    unsafe {
      scope.cast_local(|sd| v8__Set__Add(self, sd.get_current_context(), &*key))
    }
  }

  #[inline(always)]
  pub fn has(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Set__Has(self, &*scope.get_current_context(), &*key) }.into()
  }

  #[inline(always)]
  pub fn delete(
    &self,
    scope: &mut HandleScope,
    key: Local<Value>,
  ) -> Option<bool> {
    unsafe { v8__Set__Delete(self, &*scope.get_current_context(), &*key) }
      .into()
  }

  /// Returns an array of length size() * 2, where index N is the Nth key and
  /// index N + 1 is the Nth value.
  #[inline(always)]
  pub fn as_array<'s>(&self, scope: &mut HandleScope<'s>) -> Local<'s, Array> {
    unsafe { scope.cast_local(|_| v8__Set__As__Array(self)) }.unwrap()
  }
}
