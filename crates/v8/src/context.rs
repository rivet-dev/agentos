use crate::isolate::RealIsolate;
use crate::scope::PinScope;
// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.
use crate::Context;
use crate::Local;
use crate::MicrotaskQueue;
use crate::Object;
use crate::ObjectTemplate;
use crate::Value;
use crate::Weak;
use crate::handle::UnsafeRefHandle;
use crate::isolate::BuildTypeIdHasher;
use crate::isolate::RawSlot;
use crate::support::int;
use std::any::TypeId;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::ptr::{null, null_mut};
use std::rc::Rc;

/// Callback for getting high resolution timestamps in Temporal, as a number of
/// nanoseconds since the Unix epoch.
///
/// See [`Context::set_temporal_host_system_utc_epoch_nanoseconds_callback`].
pub type TemporalHostSystemUTCEpochNanosecondsCallback =
  unsafe extern "C" fn(context: Local<Context>) -> i64;

unsafe extern "C" {
  fn v8__Context__New(
    isolate: *mut RealIsolate,
    templ: *const ObjectTemplate,
    global_object: *const Value,
    microtask_queue: *mut MicrotaskQueue,
  ) -> *const Context;
  fn v8__Isolate__GetCurrent() -> *mut RealIsolate;
  fn v8__Context__Global(this: *const Context) -> *const Object;
  fn v8__Context__DetachGlobal(this: *const Context);
  fn v8__Context__GetExtrasBindingObject(this: *const Context)
  -> *const Object;
  fn v8__Context__SetTemporalHostSystemUTCEpochNanosecondsCallback(
    this: *const Context,
    callback: TemporalHostSystemUTCEpochNanosecondsCallback,
  );
  fn v8__Context__GetNumberOfEmbedderDataFields(this: *const Context) -> u32;
  fn v8__Context__GetAlignedPointerFromEmbedderData(
    this: *const Context,
    index: int,
    tag: u16,
  ) -> *mut c_void;
  fn v8__Context__SetAlignedPointerInEmbedderData(
    this: *const Context,
    index: int,
    value: *mut c_void,
    tag: u16,
  );
  fn v8__Context__GetEmbedderData(
    this: *const Context,
    index: int,
  ) -> *const Value;
  fn v8__Context__SetEmbedderData(
    this: *const Context,
    index: int,
    value: *const Value,
  );
  fn v8__Context__FromSnapshot(
    isolate: *mut RealIsolate,
    context_snapshot_index: usize,
    global_object: *const Value,
    microtask_queue: *mut MicrotaskQueue,
  ) -> *const Context;
  pub(super) fn v8__Context__GetSecurityToken(
    this: *const Context,
  ) -> *const Value;
  pub(super) fn v8__Context__SetSecurityToken(
    this: *const Context,
    value: *const Value,
  );
  pub(super) fn v8__Context__UseDefaultSecurityToken(this: *const Context);
  pub(super) fn v8__Context__AllowCodeGenerationFromStrings(
    this: *const Context,
    allow: bool,
  );
  pub(super) fn v8__Context_IsCodeGenerationFromStringsAllowed(
    this: *const Context,
  ) -> bool;
  fn v8__Context__GetMicrotaskQueue(
    this: *const Context,
  ) -> *const MicrotaskQueue;
  fn v8__Context__SetMicrotaskQueue(
    this: *const Context,
    microtask_queue: *const MicrotaskQueue,
  );
}

#[derive(Default)]
pub struct ContextOptions<'s> {
  /// An optional object template from which the global object for the newly created context will
  /// be created.
  pub global_template: Option<Local<'s, ObjectTemplate>>,
  /// An optional global object to be reused for the newly created context. This global object
  /// must have been created by a previous call to Context::New with the same global template.
  /// The state of the global object will be completely reset and only object identify will
  /// remain.
  pub global_object: Option<Local<'s, Value>>,
  /// An optional microtask queue used to manage the microtasks created in this context. If not
  /// set the per-isolate default microtask queue would be used.
  pub microtask_queue: Option<*mut MicrotaskQueue>,
}

impl Context {
  const ANNEX_SLOT: int = 1;
  const INTERNAL_SLOT_COUNT: int = 2;

  /// The tag used by the untagged embedder data accessors, matching V8's
  /// `kEmbedderDataTypeTagDefault`.
  ///
  /// [`set_aligned_pointer_in_embedder_data_with_tag`]:
  ///     Context::set_aligned_pointer_in_embedder_data_with_tag
  pub const EMBEDDER_DATA_TAG_DEFAULT: u16 = 0;

  /// The number of distinct embedder data tags V8 supports, matching V8's
  /// `V8_EMBEDDER_DATA_TAG_COUNT`. Valid tags are
  /// `0..EMBEDDER_DATA_TAG_COUNT`; V8 aborts on anything larger, so the
  /// tagged accessors panic instead. A `static_assert` in `binding.cc` keeps
  /// this in sync with V8.
  pub const EMBEDDER_DATA_TAG_COUNT: u16 = 15;

  /// Creates a new context.
  #[inline(always)]
  pub fn new<'s>(
    scope: &PinScope<'s, '_, ()>,
    options: ContextOptions,
  ) -> Local<'s, Context> {
    // A null queue selects the isolate's default queue.
    if let Some(queue) = options.microtask_queue.and_then(NonNull::new) {
      unsafe { queue.as_ref() }.assert_belongs_to(scope);
    }
    unsafe {
      scope.cast_local(|sd| {
        v8__Context__New(
          sd.get_isolate_ptr(),
          options
            .global_template
            .map_or_else(null, |t| &*t as *const _),
          options.global_object.map_or_else(null, |o| &*o as *const _),
          options.microtask_queue.unwrap_or_else(null_mut),
        )
      })
    }
    .unwrap()
  }

  #[inline(always)]
  pub fn get_extras_binding_object<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, Object> {
    unsafe { scope.cast_local(|_| v8__Context__GetExtrasBindingObject(self)) }
      .unwrap()
  }

  /// Returns the global proxy object.
  ///
  /// Global proxy object is a thin wrapper whose prototype points to actual
  /// context's global object with the properties like Object, etc. This is done
  /// that way for security reasons (for more details see
  /// https://wiki.mozilla.org/Gecko:SplitWindow).
  ///
  /// Please note that changes to global proxy object prototype most probably
  /// would break VM---v8 expects only global object as a prototype of global
  /// proxy object.
  #[inline(always)]
  pub fn global<'s>(&self, scope: &PinScope<'s, '_, ()>) -> Local<'s, Object> {
    unsafe { scope.cast_local(|_| v8__Context__Global(self)) }.unwrap()
  }

  /// Detaches this context's global proxy so the global object can be reused
  /// to create a new context through [`ContextOptions::global_object`]. The new
  /// context must use the same global template as this context. Calling this
  /// method more than once has no additional effect.
  ///
  /// Detaching resets the context's security token to its default unique state,
  /// revoking cross-context access previously granted through a shared token.
  ///
  /// This also disassociates the context from its microtask queue. Pending
  /// microtasks associated with this context will not run.
  ///
  /// A reference previously returned by [`Self::get_microtask_queue`] does not
  /// keep the queue alive. With the default `v8_cppgc_microtask_queue=true`,
  /// detaching removes this context's contribution to keeping the queue alive
  /// after its [`crate::MicrotaskQueueHandle`] has been dropped. The handle or
  /// another associated context must keep the queue alive while using any such
  /// reference.
  #[inline(always)]
  pub fn detach_global(&self) {
    unsafe { v8__Context__DetachGlobal(self) }
  }

  /// Returns the microtask queue associated with this context.
  ///
  /// Returns `None` if the context has no microtask queue, which is the case
  /// after its global object has been detached — detaching clears the native
  /// context's microtask queue pointer.
  ///
  /// The returned reference does not own or root the queue. If the context is
  /// later detached with the default `v8_cppgc_microtask_queue=true`, the
  /// context no longer keeps the queue alive after its
  /// [`crate::MicrotaskQueueHandle`] has been dropped. Do not use a reference
  /// obtained before detachment unless the handle or another associated
  /// context still keeps the queue alive.
  #[inline(always)]
  pub fn get_microtask_queue(&self) -> Option<&MicrotaskQueue> {
    let queue = unsafe { v8__Context__GetMicrotaskQueue(self) };
    NonNull::new(queue.cast_mut()).map(|queue| unsafe { &*queue.as_ptr() })
  }

  /// Associates this context with the given microtask queue.
  ///
  /// # Panics
  ///
  /// Panics if this context has no microtask queue, i.e. if its global object
  /// has been detached. V8 unconditionally dereferences the existing queue
  /// when setting a new one, so a detached context cannot be re-associated
  /// with a microtask queue.
  #[inline(always)]
  pub fn set_microtask_queue(&self, microtask_queue: &MicrotaskQueue) {
    assert!(
      self.get_microtask_queue().is_some(),
      "cannot set a microtask queue on a context whose global object has been detached"
    );
    if let Some(isolate) = crate::scope::scope_owner::current() {
      let isolate = unsafe { crate::Isolate::from_non_null(isolate) };
      microtask_queue.assert_belongs_to(&isolate);
    }
    unsafe {
      v8__Context__SetMicrotaskQueue(self, microtask_queue);
    }
  }

  #[inline]
  #[allow(clippy::mut_from_ref)]
  fn get_annex_mut(
    &self,
    create_if_not_present: bool,
  ) -> Option<&mut ContextAnnex> {
    let isolate = unsafe { v8__Isolate__GetCurrent() };
    let mut isolate = unsafe { crate::isolate::Isolate::from_raw_ptr(isolate) };

    let num_data_fields =
      unsafe { v8__Context__GetNumberOfEmbedderDataFields(self) } as int;
    if num_data_fields > Self::ANNEX_SLOT {
      let annex_ptr = unsafe {
        v8__Context__GetAlignedPointerFromEmbedderData(
          self,
          Self::ANNEX_SLOT,
          Self::EMBEDDER_DATA_TAG_DEFAULT,
        )
      } as *mut ContextAnnex;
      if !annex_ptr.is_null() {
        // SAFETY: This reference doesn't outlive the Context, so it can't outlive
        // the annex itself. Also, any mutations or accesses to the annex after
        // its creation require a mutable reference to the context's isolate, but
        // such a mutable reference is consumed by this reference during its
        // lifetime.
        return Some(unsafe { &mut *annex_ptr });
      }
    }

    if !create_if_not_present {
      return None;
    }

    let annex = Box::new(ContextAnnex {
      slots: Default::default(),
      // Gets replaced later in the method.
      self_weak: Weak::empty(&mut isolate),
    });
    let annex_ptr = Box::into_raw(annex);
    unsafe {
      v8__Context__SetAlignedPointerInEmbedderData(
        self,
        Self::ANNEX_SLOT,
        annex_ptr as *mut _,
        Self::EMBEDDER_DATA_TAG_DEFAULT,
      );
    };
    assert!(
      unsafe { v8__Context__GetNumberOfEmbedderDataFields(self) } as int
        > Self::ANNEX_SLOT
    );

    // Make sure to drop the annex after the context is dropped, by creating a
    // weak handle with a finalizer that drops the annex, and storing the weak
    // in the annex itself.
    let weak = {
      // SAFETY: `self` can only have been derived from a `Local` or `Global`,
      // and assuming the caller is only using safe code, the `Local` or
      // `Global` must still be alive, so `self_ref_handle` won't outlive it.
      // We also check above that `isolate` is the context's isolate.
      let self_ref_handle = unsafe { UnsafeRefHandle::new(self, &mut isolate) };

      Weak::with_guaranteed_finalizer(
        &mut isolate,
        self_ref_handle,
        Box::new(move || {
          // SAFETY: The lifetimes of references to the annex returned by this
          // method are always tied to the context, and because this is the
          // context's finalizer, we know there are no living references to
          // the annex. And since the finalizer is only called once, the annex
          // can't have been dropped before.
          let _ = unsafe { Box::from_raw(annex_ptr) };
        }),
      )
    };

    // SAFETY: This reference doesn't outlive the Context, so it can't outlive
    // the annex itself. Also, any mutations or accesses to the annex after
    // its creation require a mutable reference to the context's isolate, but
    // such a mutable reference is consumed by this reference during its
    // lifetime.
    let annex_mut = unsafe { &mut *annex_ptr };
    annex_mut.self_weak = weak;
    Some(annex_mut)
  }

  /// Get a reference to embedder data added with [`Self::set_slot()`].
  #[inline(always)]
  pub fn get_slot<T: 'static>(&self) -> Option<Rc<T>> {
    if let Some(annex) = self.get_annex_mut(false) {
      annex.slots.get(&TypeId::of::<T>()).map(|slot| {
        // SAFETY: `Self::set_slot` guarantees that only values of type T will be
        // stored with T's TypeId as their key.
        unsafe { slot.borrow::<Rc<T>>().clone() }
      })
    } else {
      None
    }
  }

  /// Use with [`Context::get_slot`] and [`Context::get_slot_mut`] to associate
  /// state with a Context.
  ///
  /// This method gives ownership of value to the Context. Exactly one object of
  /// each type can be associated with a Context. If called more than once with
  /// an object of the same type, the earlier version will be dropped and
  /// replaced.
  ///
  /// Returns true if value was set without replacing an existing value.
  ///
  /// The value will be dropped when the context is garbage collected.
  #[inline(always)]
  pub fn set_slot<T: 'static>(&self, value: Rc<T>) -> Option<Rc<T>> {
    self
      .get_annex_mut(true)
      .unwrap()
      .slots
      .insert(TypeId::of::<T>(), RawSlot::new(value))
      .map(|slot| {
        // SAFETY: `Self::set_slot` guarantees that only values of type T will be
        // stored with T's TypeId as their key.
        unsafe { slot.into_inner::<Rc<T>>() }
      })
  }

  /// Removes the embedder data added with [`Self::set_slot()`] and returns it
  /// if it exists.
  #[inline(always)]
  pub fn remove_slot<T: 'static>(&self) -> Option<Rc<T>> {
    if let Some(annex) = self.get_annex_mut(false) {
      annex.slots.remove(&TypeId::of::<T>()).map(|slot| {
        // SAFETY: `Self::set_slot` guarantees that only values of type T will be
        // stored with T's TypeId as their key.
        unsafe { slot.into_inner::<Rc<T>>() }
      })
    } else {
      None
    }
  }

  /// Removes all embedder data added with [`Self::set_slot()`], and
  /// deletes any internal state needed to keep track of such slots.
  ///
  /// This is needed to make a snapshot with
  /// [`SnapshotCreator`](crate::SnapshotCreator), since the internal embedder
  /// state uses [`Weak`] handles, which cannot be alive at the time of
  /// snapshotting.
  #[inline(always)]
  pub fn clear_all_slots(&self) {
    if let Some(annex_mut) = self.get_annex_mut(false) {
      let annex_ptr = annex_mut as *mut ContextAnnex;
      let _ = unsafe { Box::from_raw(annex_ptr) };
      unsafe {
        v8__Context__SetAlignedPointerInEmbedderData(
          self,
          Self::ANNEX_SLOT,
          null_mut(),
          Self::EMBEDDER_DATA_TAG_DEFAULT,
        );
      };
    }
  }

  /// Sets the embedder data with the given index, growing the data as needed.
  ///
  /// Note that index 0 currently has a special meaning for Chrome's debugger.
  #[inline(always)]
  pub fn set_embedder_data(&self, slot: i32, data: Local<'_, Value>) {
    // Initialize the annex when slot count > INTERNAL_SLOT_COUNT.
    self.get_annex_mut(true);

    unsafe {
      v8__Context__SetEmbedderData(
        self,
        slot + Self::INTERNAL_SLOT_COUNT,
        &*data,
      );
    }
  }

  /// Gets the embedder data with the given index, which must have been set by
  /// a previous call to SetEmbedderData with the same index.
  #[inline(always)]
  pub fn get_embedder_data<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
    slot: i32,
  ) -> Option<Local<'s, Value>> {
    unsafe {
      scope.cast_local(|_| {
        v8__Context__GetEmbedderData(self, slot + Self::INTERNAL_SLOT_COUNT)
      })
    }
  }

  /// Sets a 2-byte-aligned native pointer in the embedder data with the given
  /// index, growing the data as needed.
  ///
  /// This is equivalent to calling
  /// [`set_aligned_pointer_in_embedder_data_with_tag`] with the default tag.
  ///
  /// # Safety
  /// The pointer must be 2-byte aligned.
  ///
  /// [`set_aligned_pointer_in_embedder_data_with_tag`]:
  ///     Context::set_aligned_pointer_in_embedder_data_with_tag
  #[inline(always)]
  pub unsafe fn set_aligned_pointer_in_embedder_data(
    &self,
    slot: i32,
    data: *mut c_void,
  ) {
    unsafe {
      self.set_aligned_pointer_in_embedder_data_with_tag(
        slot,
        data,
        Self::EMBEDDER_DATA_TAG_DEFAULT,
      );
    }
  }

  #[inline(always)]
  fn assert_valid_embedder_data_tag(tag: u16) {
    assert!(
      tag < Self::EMBEDDER_DATA_TAG_COUNT,
      "embedder data tag {tag} is out of range; must be less than {}",
      Self::EMBEDDER_DATA_TAG_COUNT
    );
  }

  /// Sets a 2-byte-aligned native pointer in the embedder data with the given
  /// index, growing the data as needed. The `tag` distinguishes pointers of
  /// different types stored in embedder data; the same tag must be passed to
  /// [`get_aligned_pointer_from_embedder_data_with_tag`] when reading the
  /// value back.
  ///
  /// Reading a slot with a tag other than the one it was written with does not
  /// report an error, and what it does return depends on the build: with the
  /// V8 sandbox enabled the mismatch is detected and the read returns null,
  /// while with the sandbox disabled the tag is ignored entirely and the
  /// stored pointer is returned regardless. Neither is a substitute for the
  /// caller keeping the tags of a slot consistent.
  ///
  /// Use [`EMBEDDER_DATA_TAG_DEFAULT`] when no distinction between pointer
  /// types is needed.
  ///
  /// # Panics
  ///
  /// Panics if `tag` is not less than [`EMBEDDER_DATA_TAG_COUNT`]. V8 aborts
  /// the process for out-of-range tags, so this is checked here instead.
  ///
  /// # Safety
  /// The pointer must be 2-byte aligned.
  ///
  /// [`get_aligned_pointer_from_embedder_data_with_tag`]:
  ///     Context::get_aligned_pointer_from_embedder_data_with_tag
  /// [`EMBEDDER_DATA_TAG_DEFAULT`]: Context::EMBEDDER_DATA_TAG_DEFAULT
  /// [`EMBEDDER_DATA_TAG_COUNT`]: Context::EMBEDDER_DATA_TAG_COUNT
  #[inline(always)]
  pub unsafe fn set_aligned_pointer_in_embedder_data_with_tag(
    &self,
    slot: i32,
    data: *mut c_void,
    tag: u16,
  ) {
    Self::assert_valid_embedder_data_tag(tag);

    // Initialize the annex when slot count > INTERNAL_SLOT_COUNT.
    self.get_annex_mut(true);

    unsafe {
      v8__Context__SetAlignedPointerInEmbedderData(
        self,
        slot + Self::INTERNAL_SLOT_COUNT,
        data,
        tag,
      );
    }
  }

  /// Gets a 2-byte-aligned native pointer from the embedder data with the
  /// given index, which must have been set by a previous call to
  /// [`set_aligned_pointer_in_embedder_data`] with the same index.
  ///
  /// [`set_aligned_pointer_in_embedder_data`]:
  ///     Context::set_aligned_pointer_in_embedder_data
  #[inline(always)]
  pub fn get_aligned_pointer_from_embedder_data(
    &self,
    slot: i32,
  ) -> *mut c_void {
    self.get_aligned_pointer_from_embedder_data_with_tag(
      slot,
      Self::EMBEDDER_DATA_TAG_DEFAULT,
    )
  }

  /// Gets a 2-byte-aligned native pointer from the embedder data with the
  /// given index, which must have been set by a previous call to
  /// [`set_aligned_pointer_in_embedder_data_with_tag`] with the same index
  /// and the same `tag`.
  ///
  /// If the tag does not match the one the slot was written with, this returns
  /// null when the V8 sandbox is enabled and the stored pointer when it is
  /// not; see [`set_aligned_pointer_in_embedder_data_with_tag`]. Do not
  /// dereference the result unless the tags match.
  ///
  /// # Panics
  ///
  /// Panics if `tag` is not less than [`EMBEDDER_DATA_TAG_COUNT`].
  ///
  /// [`set_aligned_pointer_in_embedder_data_with_tag`]:
  ///     Context::set_aligned_pointer_in_embedder_data_with_tag
  /// [`EMBEDDER_DATA_TAG_COUNT`]: Context::EMBEDDER_DATA_TAG_COUNT
  #[inline(always)]
  pub fn get_aligned_pointer_from_embedder_data_with_tag(
    &self,
    slot: i32,
    tag: u16,
  ) -> *mut c_void {
    Self::assert_valid_embedder_data_tag(tag);

    unsafe {
      v8__Context__GetAlignedPointerFromEmbedderData(
        self,
        slot + Self::INTERNAL_SLOT_COUNT,
        tag,
      )
    }
  }

  /// Create a new context from a (non-default) context snapshot. There
  /// is no way to provide a global object template since we do not create
  /// a new global object from template, but we can reuse a global object.
  pub fn from_snapshot<'s>(
    scope: &PinScope<'s, '_, ()>,
    context_snapshot_index: usize,
    options: ContextOptions,
  ) -> Option<Local<'s, Context>> {
    // A null queue selects the isolate's default queue.
    if let Some(queue) = options.microtask_queue.and_then(NonNull::new) {
      unsafe { queue.as_ref() }.assert_belongs_to(scope);
    }
    unsafe {
      scope.cast_local(|sd| {
        v8__Context__FromSnapshot(
          sd.get_isolate_ptr(),
          context_snapshot_index,
          options.global_object.map_or_else(null, |o| &*o as *const _),
          options.microtask_queue.unwrap_or_else(null_mut),
        )
      })
    }
  }

  #[inline(always)]
  pub fn get_security_token<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Local<'s, Value> {
    unsafe { scope.cast_local(|_| v8__Context__GetSecurityToken(self)) }
      .unwrap()
  }

  #[inline(always)]
  pub fn set_security_token(&self, token: Local<Value>) {
    unsafe {
      v8__Context__SetSecurityToken(self, &*token);
    }
  }

  /// Sets the callback used to obtain high resolution timestamps in Temporal.
  ///
  /// The callback is invoked with this context and returns the current time as
  /// a number of nanoseconds since the Unix epoch.
  #[inline(always)]
  pub fn set_temporal_host_system_utc_epoch_nanoseconds_callback(
    &self,
    callback: TemporalHostSystemUTCEpochNanosecondsCallback,
  ) {
    unsafe {
      v8__Context__SetTemporalHostSystemUTCEpochNanosecondsCallback(
        self, callback,
      );
    }
  }

  #[inline(always)]
  pub fn use_default_security_token(&self) {
    unsafe {
      v8__Context__UseDefaultSecurityToken(self);
    }
  }

  pub fn set_allow_generation_from_strings(&self, allow: bool) {
    unsafe {
      v8__Context__AllowCodeGenerationFromStrings(self, allow);
    }
  }

  pub fn is_code_generation_from_strings_allowed(&self) -> bool {
    unsafe { v8__Context_IsCodeGenerationFromStringsAllowed(self) }
  }
}

struct ContextAnnex {
  slots: HashMap<TypeId, RawSlot, BuildTypeIdHasher>,
  // In order to run the finalizer that drops the ContextAnnex when the Context
  // is GC'd, the corresponding Weak must be kept alive until that time.
  self_weak: Weak<Context>,
}
