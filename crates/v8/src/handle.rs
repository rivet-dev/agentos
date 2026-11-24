use std::borrow::Borrow;
use std::cell::Cell;
use std::ffi::c_void;
use std::hash::Hash;
use std::hash::Hasher;
use std::marker::PhantomData;
use std::mem::forget;
use std::mem::transmute;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::Data;
use crate::Isolate;
use crate::IsolateHandle;
use crate::isolate::IsolateLiveness;
use crate::isolate::RealIsolate;
use crate::scope::GetIsolate;
use crate::scope::PinScope;
use crate::support::Opaque;

unsafe extern "C" {
  fn v8__Local__New(
    isolate: *mut RealIsolate,
    other: *const Data,
  ) -> *const Data;
  fn v8__Global__New(
    isolate: *mut RealIsolate,
    data: *const Data,
  ) -> *const Data;
  fn v8__Global__NewWeak(
    isolate: *mut RealIsolate,
    data: *const Data,
    parameter: *const c_void,
    callback: unsafe extern "C" fn(*const WeakCallbackInfo),
  ) -> *const Data;
  fn v8__Global__Reset(data: *const Data);
  fn v8__WeakCallbackInfo__GetIsolate(
    this: *const WeakCallbackInfo,
  ) -> *mut RealIsolate;
  fn v8__WeakCallbackInfo__GetParameter(
    this: *const WeakCallbackInfo,
  ) -> *mut c_void;
  fn v8__WeakCallbackInfo__SetSecondPassCallback(
    this: *const WeakCallbackInfo,
    callback: unsafe extern "C" fn(*const WeakCallbackInfo),
  );

  fn v8__TracedReference__CONSTRUCT(this: *mut TracedReference<Data>);
  fn v8__TracedReference__DESTRUCT(this: *mut TracedReference<Data>);
  fn v8__TracedReference__Reset(
    this: *mut TracedReference<Data>,
    isolate: *mut RealIsolate,
    data: *mut Data,
  );
  fn v8__TracedReference__Get(
    this: *const TracedReference<Data>,
    isolate: *mut RealIsolate,
  ) -> *const Data;

  fn v8__Eternal__CONSTRUCT(this: *mut Eternal<Data>);
  fn v8__Eternal__DESTRUCT(this: *mut Eternal<Data>);
  fn v8__Eternal__Clear(this: *mut Eternal<Data>);
  fn v8__Eternal__Get(
    this: *const Eternal<Data>,
    isolate: *mut RealIsolate,
  ) -> *const Data;
  fn v8__Eternal__Set(
    this: *mut Eternal<Data>,
    isolate: *mut RealIsolate,
    data: *mut Data,
  );
  fn v8__Eternal__IsEmpty(this: *const Eternal<Data>) -> bool;
}

/// An object reference managed by the v8 garbage collector.
///
/// All objects returned from v8 have to be tracked by the garbage
/// collector so that it knows that the objects are still alive.  Also,
/// because the garbage collector may move objects, it is unsafe to
/// point directly to an object.  Instead, all objects are stored in
/// handles which are known by the garbage collector and updated
/// whenever an object moves.  Handles should always be passed by value
/// (except in cases like out-parameters) and they should never be
/// allocated on the heap.
///
/// There are two types of handles: local and persistent handles.
///
/// Local handles are light-weight and transient and typically used in
/// local operations.  They are managed by HandleScopes. That means that a
/// HandleScope must exist on the stack when they are created and that they are
/// only valid inside of the `HandleScope` active during their creation.
/// For passing a local handle to an outer `HandleScope`, an
/// `EscapableHandleScope` and its `Escape()` method must be used.
///
/// Persistent handles can be used when storing objects across several
/// independent operations and have to be explicitly deallocated when they're no
/// longer used.
///
/// It is safe to extract the object stored in the handle by
/// dereferencing the handle (for instance, to extract the `*Object` from
/// a `Local<Object>`); the value will still be governed by a handle
/// behind the scenes and the same rules apply to these values as to
/// their handles.
///
/// Note: Local handles in Rusty V8 differ from the V8 C++ API in that they are
/// never empty. In situations where empty handles are needed, use
/// `Option<Local>`.
#[repr(C)]
#[derive(Debug)]
pub struct Local<'s, T>(NonNull<T>, PhantomData<&'s ()>);

mod sealed {
  pub trait Sealed {}
}

// this trait exists to allow you to specify the output lifetime for `Local::extend_lifetime_unchecked`.
// so you can do something like `unsafe { Local::extend_lifetime_unchecked::<Local<'o, T>>(local) }`.
// if it were just a lifetime parameter, it would be "late bound" and you could not explicitly specify the output lifetime.
pub trait ExtendLifetime<'s, T>: sealed::Sealed {
  type Input;
  unsafe fn extend_lifetime_unchecked_from(value: Self::Input) -> Self;
}

impl<T> sealed::Sealed for Local<'_, T> {}

impl<'s, T> ExtendLifetime<'s, T> for Local<'_, T> {
  type Input = Local<'s, T>;
  unsafe fn extend_lifetime_unchecked_from(value: Self::Input) -> Self {
    unsafe { Local::from_non_null(value.as_non_null()) }
  }
}

impl<'s, T> Local<'s, T> {
  /// Construct a new Local from an existing Handle.
  #[inline(always)]
  pub fn new<'i>(
    scope: &PinScope<'s, 'i, ()>,
    handle: impl Handle<Data = T>,
  ) -> Local<'s, T> {
    let HandleInfo { data, host } = handle.get_handle_info();
    host.assert_match_isolate(scope);
    unsafe {
      scope.cast_local(|sd| {
        v8__Local__New(sd.get_isolate_ptr(), data.cast().as_ptr()) as *const T
      })
    }
    .unwrap()
  }

  /// Create a local handle by downcasting from one of its super types.
  /// This function is unsafe because the cast is unchecked.
  #[inline(always)]
  pub unsafe fn cast_unchecked<A>(other: Local<'s, A>) -> Self
  where
    Local<'s, A>: TryFrom<Self>,
  {
    unsafe { transmute(other) }
  }
  /// Extend the lifetime of a `Local` handle to a longer lifetime.
  ///
  /// # Safety
  ///
  /// The caller is responsible for ensuring that the `Local` handle is valid
  /// for the longer lifetime. Incorrect usage can lead to the usage of invalid
  /// handles
  ///
  /// # Example
  ///
  /// ```ignore
  /// let isolate = unsafe { Isolate::from_raw_isolate_ptr(isolate_ptr) };
  /// callback_scope!(unsafe scope, &mut isolate);
  /// // the lifetime of the local handle will be tied to the lifetime of `&mut isolate`,
  /// // which, because we've created it from a raw pointer, is only as long as the current function.
  /// // the real lifetime at runtime is
  /// // actually the lifetime of the parent scope. if we can guarantee that the parent scope lives at least as long as
  /// // `'o`, it is valid to extend the lifetime of the local handle to `'o` by using `extend_lifetime_unchecked`.
  /// let context = Local::new(scope, context_global_handle);
  ///
  /// let local_longer_lifetime = unsafe { local.extend_lifetime_unchecked::<Local<'o, T>>() };
  /// ```
  #[inline(always)]
  pub unsafe fn extend_lifetime_unchecked<'o, O>(self) -> O
  where
    O: ExtendLifetime<'s, T, Input = Self>,
  {
    unsafe { O::extend_lifetime_unchecked_from(self) }
  }

  #[inline(always)]
  pub(crate) unsafe fn from_raw(ptr: *const T) -> Option<Self> {
    NonNull::new(ptr as *mut _).map(|nn| unsafe { Self::from_non_null(nn) })
  }

  #[inline(always)]
  pub(crate) unsafe fn from_raw_unchecked(ptr: *const T) -> Self {
    Self(
      unsafe { NonNull::new_unchecked(ptr as *mut _) },
      PhantomData,
    )
  }

  #[inline(always)]
  pub(crate) unsafe fn from_non_null(nn: NonNull<T>) -> Self {
    Self(nn, PhantomData)
  }

  #[inline(always)]
  pub(crate) fn as_non_null(self) -> NonNull<T> {
    #[cfg(debug_assertions)]
    crate::scope::scope_owner::check_handle(self.0.cast());
    self.0
  }

  #[inline(always)]
  pub(crate) fn slice_into_raw(slice: &[Self]) -> &[*const T] {
    #[cfg(debug_assertions)]
    for local in slice {
      crate::scope::scope_owner::check_handle(local.0.cast());
    }
    unsafe { &*(slice as *const [Self] as *const [*const T]) }
  }
}

impl<T> Copy for Local<'_, T> {}

impl<T> Clone for Local<'_, T> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<T> Deref for Local<'_, T> {
  type Target = T;
  fn deref(&self) -> &T {
    #[cfg(debug_assertions)]
    crate::scope::scope_owner::check_handle(self.0.cast());
    unsafe { self.0.as_ref() }
  }
}

impl<'s, T> Local<'s, T> {
  /// Attempts to cast the contained type to another,
  /// returning an error if the conversion fails.
  ///
  /// # Examples
  ///
  /// ```ignore
  /// let value: Local<'_, Value> = get_v8_value();
  ///
  /// if let Ok(func) = value.try_cast::<Function> {
  ///   //
  /// }
  /// ```
  #[inline(always)]
  pub fn try_cast<A>(
    self,
  ) -> Result<Local<'s, A>, <Self as TryInto<Local<'s, A>>>::Error>
  where
    Self: TryInto<Local<'s, A>>,
  {
    self.try_into()
  }

  /// Attempts to cast the contained type to another,
  /// panicking if the conversion fails.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let value: Local<'_, Value> = get_v8_value();
  ///
  /// let func = value.cast::<Function>();
  /// ```
  #[inline(always)]
  pub fn cast<A>(self) -> Local<'s, A>
  where
    Self: TryInto<Local<'s, A>, Error: std::fmt::Debug>,
  {
    self.try_into().unwrap()
  }
}

/// An object reference that is independent of any handle scope. Where
/// a Local handle only lives as long as the HandleScope in which it was
/// allocated, a global handle remains valid until it is dropped.
///
/// A global handle contains a reference to a storage cell within
/// the V8 engine which holds an object value and which is updated by
/// the garbage collector whenever the object is moved.
///
/// You can create a `v8::Local` out of `v8::Global` using
/// `v8::Local::new(scope, global_handle)`.
///
/// Dropping a `Global` belonging to a [`crate::SharedIsolate`] without holding
/// that isolate's [`crate::Locker`] defers resetting its V8 storage cell until
/// the next lock boundary or isolate teardown. Until then the handle remains a
/// GC root and may keep its JavaScript object graph alive.
///
/// # Thread safety
///
/// `Global<T>` is [`Send`] and [`Sync`], so the handle may be moved or shared
/// between threads. This does not make the V8 heap object itself concurrently
/// accessible. Cloning, hashing, creating a [`Local`], and comparisons that
/// may involve the same isolate touch V8: for a non-shared isolate they require
/// its home thread, and for a [`crate::SharedIsolate`] they require holding its
/// [`crate::Locker`] on the current thread. These operations panic when that
/// requirement is not met or when the host isolate has been disposed. Two
/// handles known to belong to different live isolates compare unequal without
/// accessing either isolate. Dropping a `Global` is allowed on any thread and
/// may defer resetting its V8 storage cell as described above.
///
/// `Global<T>` deliberately does not implement [`std::borrow::Borrow<T>`].
/// Such an impl could return a reference that outlives the isolate or the
/// `Locker` proving access to a shared isolate. Use [`Local::new`] under a
/// handle scope instead. As a consequence, a `HashMap<Global<T>, _>` cannot be
/// queried by `&T`; callers needing allocation-free borrowed lookup should use
/// an embedder-owned stable key rather than the V8 object reference.
///
/// Opening a `Global` into a plain reference is unsafe because the reference
/// could outlive its isolate or cross threads. Prefer [`Local::new`] under a
/// handle scope instead.
#[derive(Debug)]
pub struct Global<T> {
  data: NonNull<T>,
  isolate_liveness: NonNull<IsolateLiveness>,
}

impl<T> Global<T> {
  #[inline(always)]
  fn assert_access_allowed(&self) {
    unsafe {
      self.isolate_liveness.as_ref().assert_access_allowed();
    }
  }

  /// Construct a new Global from an existing Handle.
  #[inline(always)]
  pub fn new(isolate: &Isolate, handle: impl Handle<Data = T>) -> Self {
    let HandleInfo { data, host } = handle.get_handle_info();
    host.assert_match_isolate(isolate);
    unsafe { Self::new_raw(isolate as *const Isolate as *mut Isolate, data) }
  }

  /// Implementation helper function that contains the code that can be shared
  /// between `Global::new()` and `Global::clone()`.
  #[inline(always)]
  unsafe fn new_raw(isolate: *mut Isolate, data: NonNull<T>) -> Self {
    let data = data.cast().as_ptr();
    unsafe {
      let isolate_liveness = (*isolate).global_liveness();
      // Cheap checkpoint (a relaxed load when empty) so cells dropped by
      // threads that couldn't touch the isolate don't pin their JS
      // objects indefinitely on isolates that are never locked.
      isolate_liveness
        .as_ref()
        .maybe_drain_deferred_global_resets();
      let data = v8__Global__New((*isolate).as_real_ptr(), data) as *const T;
      let data = NonNull::new_unchecked(data as *mut _);
      Self {
        data,
        isolate_liveness,
      }
    }
  }

  /// Consume this `Global` and return the underlying raw pointer.
  ///
  /// The returned raw pointer must be converted back into a `Global` by using
  /// [`Global::from_raw`], otherwise the V8 value referenced by this global
  /// handle will be pinned on the V8 heap permanently and never get garbage
  /// collected.
  #[inline(always)]
  pub fn into_raw(self) -> NonNull<T> {
    let data = self.data;
    forget(self);
    data
  }

  /// Converts a raw pointer created with [`Global::into_raw()`] back to its
  /// original `Global`.
  #[inline(always)]
  pub unsafe fn from_raw(isolate: &mut Isolate, data: NonNull<T>) -> Self {
    let isolate_liveness = isolate.global_liveness();
    Self {
      data,
      isolate_liveness,
    }
  }

  /// Returns a reference to the V8 heap object represented by this handle.
  /// The handle is not cloned or converted to a [`Local`].
  ///
  /// Prefer [`Local::new`] whenever possible. Unlike this function, a
  /// [`Local`]'s lifetime is tied to its handle scope.
  ///
  /// # Safety
  ///
  /// For the entire lifetime of the returned reference, `isolate` must remain
  /// alive and the current thread must remain permitted to access it. If the
  /// isolate is shared, its [`crate::Locker`] must remain held on the current
  /// thread. The reference must never be sent to or accessed from another
  /// thread.
  ///
  /// # Panics
  ///
  /// This function panics if the handle is not hosted by `isolate`, if the
  /// isolate has been disposed, or if the current thread is not permitted to
  /// access the isolate.
  #[inline(always)]
  pub unsafe fn open<'a>(&'a self, isolate: &mut Isolate) -> &'a T {
    self.assert_access_allowed();
    self.get_handle_host().assert_match_isolate(isolate);
    unsafe { &*self.data.as_ptr() }
  }

  #[inline(always)]
  fn get_handle_host(&self) -> HandleHost {
    let isolate = unsafe { self.isolate_liveness.as_ref().get_isolate_ptr() };
    NonNull::new(isolate)
      .map_or(HandleHost::DisposedIsolate, HandleHost::Isolate)
  }

  /// Panics unless the `Local`s of the scopes open on this thread belong to
  /// the `Isolate` that hosts this handle.
  pub(crate) fn assert_locals_in_same_isolate(&self) {
    HandleHost::Scope.assert_match_host(self.get_handle_host(), None);
  }
}

/// Panics unless the `Local`s of the scopes open on this thread belong to
/// `isolate`. APIs that take an `Isolate` and `Local`s as separate arguments
/// call this, because the scope rule doesn't tie the two together.
pub(crate) fn assert_locals_in_isolate(isolate: &Isolate) {
  HandleHost::Scope.assert_match_isolate(isolate);
}

/// Like [`assert_locals_in_isolate`], for an `Isolate` that may have been
/// disposed.
pub(crate) fn assert_locals_in_isolate_of(isolate_handle: &IsolateHandle) {
  HandleHost::Scope.assert_match_host(isolate_handle.into(), None);
}

// A `Global` only touches V8 through methods that either take a scope or
// `&Isolate` argument (obtainable only on the isolate's thread or under
// its Locker), or that are guarded through `IsolateLiveness`: `clone`,
// `eq` and `hash` assert the current thread may touch the isolate, and
// `drop` releases the cell immediately when it may, deferring to the
// liveness queue otherwise.
unsafe impl<T> Send for Global<T> {}
unsafe impl<T> Sync for Global<T> {}

impl<T> Clone for Global<T> {
  fn clone(&self) -> Self {
    self.assert_access_allowed();
    let HandleInfo { data, host } = self.get_handle_info();
    let mut isolate = unsafe { Isolate::from_non_null(host.get_isolate()) };
    unsafe { Self::new_raw(isolate.as_mut(), data) }
  }
}

impl<T> Drop for Global<T> {
  fn drop(&mut self) {
    unsafe {
      let liveness = self.isolate_liveness.as_ref();
      if liveness.get_isolate_ptr().is_null() {
        // This `Global` handle is associated with an `Isolate` that has already
        // been disposed.
      } else if !liveness.is_shared() && liveness.on_home_thread() {
        // Destroy the storage cell that contains the contents of this Global.
        v8__Global__Reset(self.data.cast().as_ptr());
        liveness.maybe_drain_deferred_global_resets();
      } else {
        // Another thread may own the isolate right now; release the cell
        // immediately if we may touch it, otherwise defer to the next
        // lock acquisition or isolate teardown.
        liveness.reset_or_defer_global(self.data.cast());
      }
    }
  }
}

/// An implementation of [`Handle`] that can be constructed unsafely from a
/// reference.
pub(crate) struct UnsafeRefHandle<'a, T> {
  reference: &'a T,
  isolate_handle: IsolateHandle,
}
impl<'a, T> UnsafeRefHandle<'a, T> {
  /// Constructs an `UnsafeRefHandle`.
  ///
  /// # Safety
  ///
  /// `reference` must be derived from a [`Local`] or [`Global`] handle, and its
  /// lifetime must not outlive that handle. Furthermore, `isolate` must be the
  /// isolate associated with the handle (for [`Local`], the current isolate;
  /// for [`Global`], the isolate you would pass to the unsafe
  /// [`Global::open()`] method).
  #[inline(always)]
  pub unsafe fn new(reference: &'a T, isolate: &mut Isolate) -> Self {
    UnsafeRefHandle {
      reference,
      isolate_handle: isolate.thread_safe_handle(),
    }
  }
}

pub trait Handle: Sized {
  type Data;

  #[doc(hidden)]
  fn get_handle_info(&self) -> HandleInfo<Self::Data>;

  #[doc(hidden)]
  fn assert_safe_to_access(&self) {}

  /// Reads the inner value contained in this handle, _without_ verifying that
  /// the this handle is hosted by the currently active `Isolate`.
  ///
  /// # Safety
  ///
  /// Using a V8 heap object with another `Isolate` than the `Isolate` that
  /// hosts it is not permitted under any circumstance. Doing so leads to
  /// undefined behavior, likely a crash.
  ///
  /// For the entire lifetime of the returned reference, its handle and host
  /// isolate must remain alive and the current thread must remain permitted to
  /// access that isolate. If this is a [`Global`] belonging to a shared isolate,
  /// its [`crate::Locker`] must remain held on the current thread. The reference
  /// must never be sent to or accessed from another thread.
  ///
  /// # Panics
  ///
  /// This function panics if the `Isolate` that hosts the handle has been
  /// disposed or, for a [`Global`], if the current thread is not permitted to
  /// access it.
  unsafe fn get_unchecked(&self) -> &Self::Data {
    self.assert_safe_to_access();
    let HandleInfo { data, host } = self.get_handle_info();
    if let HandleHost::DisposedIsolate = host {
      panic!("attempt to access Handle hosted by disposed Isolate");
    }
    unsafe { &*data.as_ptr() }
  }
}

impl<T> Handle for Local<'_, T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(self.as_non_null(), HandleHost::Scope)
  }
}

impl<'a, 's: 'a, T> Handle for &'a Local<'s, T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(self.as_non_null(), HandleHost::Scope)
  }
}

impl<T> Handle for Global<T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(self.data, self.get_handle_host())
  }
  fn assert_safe_to_access(&self) {
    self.assert_access_allowed();
  }
}

impl<T> Handle for &Global<T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(self.data, self.get_handle_host())
  }
  fn assert_safe_to_access(&self) {
    self.assert_access_allowed();
  }
}

impl<T> Handle for UnsafeRefHandle<'_, T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(
      NonNull::from(self.reference),
      (&self.isolate_handle).into(),
    )
  }
}

impl<T> Handle for &UnsafeRefHandle<'_, T> {
  type Data = T;
  fn get_handle_info(&self) -> HandleInfo<T> {
    HandleInfo::new(
      NonNull::from(self.reference),
      (&self.isolate_handle).into(),
    )
  }
}

impl<T> Borrow<T> for Local<'_, T> {
  fn borrow(&self) -> &T {
    self
  }
}

// `Borrow<T> for Global<T>` is deliberately absent. `fn borrow(&self) -> &T`
// has nowhere to take proof that the caller may touch the isolate, and
// nowhere to tie the returned reference's lifetime to that proof: any check
// it made would expire while the `&T` it handed out stayed alive. Heap-object
// wrappers are `!Sync`, which prevents moving that reference to another
// thread, but does not prevent it from outliving a Locker or the isolate. Use
// `Local::new(scope, &global)` instead — a `Local` is bound to a scope, which
// is bound to the isolate.

impl<T> Eq for Local<'_, T> where T: Eq {}
impl<T> Eq for Global<T> where T: Eq {}

impl<T: Hash> Hash for Local<'_, T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    (**self).hash(state);
  }
}

impl<T: Hash> Hash for Global<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    // Hashing may call into V8 (e.g. `Object::GetIdentityHash`, which can
    // mutate the object), so it needs the same gate as any other access.
    if unsafe { self.isolate_liveness.as_ref().get_isolate_ptr().is_null() } {
      panic!("can't hash Global after its host Isolate has been disposed");
    }
    self.assert_access_allowed();
    unsafe { self.data.as_ref().hash(state) }
  }
}

impl<T, Rhs: Handle> PartialEq<Rhs> for Local<'_, T>
where
  T: PartialEq<Rhs::Data>,
{
  fn eq(&self, other: &Rhs) -> bool {
    let i1 = self.get_handle_info();
    let i2 = other.get_handle_info();
    if i1.host.are_different_live_isolates(i2.host) {
      return false;
    }
    self.assert_safe_to_access();
    other.assert_safe_to_access();
    i1.host.match_host(i2.host, None)
      && unsafe { i1.data.as_ref() == i2.data.as_ref() }
  }
}

impl<T, Rhs: Handle> PartialEq<Rhs> for Global<T>
where
  T: PartialEq<Rhs::Data>,
{
  fn eq(&self, other: &Rhs) -> bool {
    let i1 = self.get_handle_info();
    let i2 = other.get_handle_info();
    // Distinct live isolates cannot contain the same V8 object. This check
    // does not touch either isolate, so preserve the historical `false`
    // result even if one Global is currently off its home/Locker thread.
    if i1.host.are_different_live_isolates(i2.host) {
      return false;
    }
    self.assert_safe_to_access();
    other.assert_safe_to_access();
    if !i1.host.match_host(i2.host, None) {
      return false;
    }
    // Comparison calls into V8 (e.g. `Value::SameValue`); both operands
    // were gated by `assert_safe_to_access` above.
    unsafe { i1.data.as_ref() == i2.data.as_ref() }
  }
}

#[derive(Copy, Debug, Clone)]
pub struct HandleInfo<T> {
  data: NonNull<T>,
  host: HandleHost,
}

impl<T> HandleInfo<T> {
  fn new(data: NonNull<T>, host: HandleHost) -> Self {
    Self { data, host }
  }
}

#[derive(Copy, Debug, Clone)]
enum HandleHost {
  // Note: the `HandleHost::Scope` variant does not indicate that the handle
  // it applies to is not associated with an `Isolate`. It only means that
  // the handle is a `Local` handle that was unable to provide a pointer to
  // the `Isolate` that hosts it (the handle) and the currently entered
  // scope.
  Scope,
  Isolate(NonNull<RealIsolate>),
  DisposedIsolate,
}

impl From<&'_ Isolate> for HandleHost {
  fn from(isolate: &'_ Isolate) -> Self {
    Self::Isolate(unsafe { NonNull::new_unchecked(isolate.as_real_ptr()) })
  }
}

impl From<&'_ IsolateHandle> for HandleHost {
  fn from(isolate_handle: &IsolateHandle) -> Self {
    NonNull::new(unsafe { isolate_handle.get_isolate_ptr() })
      .map_or(Self::DisposedIsolate, Self::Isolate)
  }
}

impl HandleHost {
  fn are_different_live_isolates(self, other: Self) -> bool {
    matches!(
      (self, other),
      (Self::Isolate(left), Self::Isolate(right)) if left != right
    )
  }

  /// Compares two `HandleHost` values, returning `true` if they refer to the
  /// same `Isolate`, or `false` if they refer to different isolates.
  ///
  /// A `Scope` host refers to the `Isolate` that owns the scopes open on this
  /// thread (see "Isolates" in the `scope` module documentation). When no
  /// scope is open, the caller can say which `Isolate` the handle's scope
  /// belongs to via the second argument (`scope_isolate_opt`).
  ///
  /// # Panics
  ///
  /// This function panics if one of the `HandleHost` values refers to an
  /// `Isolate` that has been disposed.
  ///
  /// # Safety / Bugs
  ///
  /// If no scope is open and the caller does not provide a value for the
  /// `scope_isolate_opt` argument, the `Isolate` of a `Scope` host is unknown,
  /// and this function returns `true`. A `Local` can only exist outside of a
  /// scope through `unsafe` code.
  fn match_host(
    self,
    other: Self,
    scope_isolate_opt: Option<&Isolate>,
  ) -> bool {
    let scope_isolate_opt_nn =
      crate::scope::scope_owner::current().or_else(|| {
        scope_isolate_opt.map(|isolate| unsafe {
          NonNull::new_unchecked(isolate.as_real_ptr())
        })
      });
    match (self, other, scope_isolate_opt_nn) {
      (Self::Scope, Self::Scope, _) => true,
      (Self::Isolate(ile1), Self::Isolate(ile2), _) => ile1 == ile2,
      (Self::Scope, Self::Isolate(ile1), Some(ile2)) => ile1 == ile2,
      (Self::Isolate(ile1), Self::Scope, Some(ile2)) => ile1 == ile2,
      (Self::Scope, Self::Isolate(_), None) => true,
      (Self::Isolate(_), Self::Scope, None) => true,
      // Handles hosted in an Isolate that has been disposed aren't good for
      // anything, even if a pair of handles used to to be hosted in the same
      // now-disposed solate.
      (Self::DisposedIsolate, ..) | (_, Self::DisposedIsolate, _) => {
        panic!("attempt to access Handle hosted by disposed Isolate")
      }
    }
  }

  fn assert_match_host(self, other: Self, scope_opt: Option<&Isolate>) {
    assert!(
      self.match_host(other, scope_opt),
      "attempt to use Handle in an Isolate that is not its host"
    );
  }

  #[allow(dead_code)]
  fn match_isolate(self, isolate: &Isolate) -> bool {
    self.match_host(isolate.into(), Some(isolate))
  }

  fn assert_match_isolate(self, isolate: &Isolate) {
    self.assert_match_host(isolate.into(), Some(isolate));
  }

  fn get_isolate(self) -> NonNull<RealIsolate> {
    match self {
      Self::Scope => panic!("host Isolate for Handle not available"),
      Self::Isolate(ile) => ile,
      Self::DisposedIsolate => panic!("attempt to access disposed Isolate"),
    }
  }

  #[allow(dead_code)]
  fn get_isolate_handle(self) -> IsolateHandle {
    let isolate = unsafe { Isolate::from_non_null(self.get_isolate()) };
    isolate.thread_safe_handle()
  }
}

/// An object reference that does not prevent garbage collection for the object,
/// and which allows installing finalization callbacks which will be called
/// after the object has been GC'd.
///
/// Note that finalization callbacks are tied to the lifetime of a `Weak<T>`,
/// and will not be called after the `Weak<T>` is dropped.
///
/// # `Clone`
///
/// Since finalization callbacks are specific to a `Weak<T>` instance, cloning
/// will create a new object reference without a finalizer, as if created by
/// [`Self::new`]. You can use [`Self::clone_with_finalizer`] to attach a
/// finalization callback to the clone.
#[derive(Debug)]
pub struct Weak<T> {
  data: Option<Box<WeakData<T>>>,
  isolate_handle: IsolateHandle,
}

impl<T> Weak<T> {
  pub fn new(isolate: &mut Isolate, handle: impl Handle<Data = T>) -> Self {
    let HandleInfo { data, host } = handle.get_handle_info();
    host.assert_match_isolate(isolate);
    Self::new_raw(isolate, data, None)
  }

  /// Create a weak handle with a finalization callback installed.
  ///
  /// There is no guarantee as to *when* or even *if* the finalization callback
  /// will be invoked. The invocation is performed solely on a best effort
  /// basis. GC-based finalization should *not* be relied upon for any critical
  /// form of resource management! Consider using
  /// [`Self::with_guaranteed_finalizer`] instead.
  ///
  /// The callback does not have access to the inner value, because it has
  /// already been collected by the time it runs.
  pub fn with_finalizer(
    isolate: &mut Isolate,
    handle: impl Handle<Data = T>,
    finalizer: Box<dyn FnOnce(&mut Isolate)>,
  ) -> Self {
    let HandleInfo { data, host } = handle.get_handle_info();
    host.assert_match_isolate(isolate);
    Self::assert_supported(isolate);
    let finalizer_id = isolate
      .get_finalizer_map_mut()
      .add(FinalizerCallback::Regular(finalizer));
    Self::new_raw(isolate, data, Some(finalizer_id))
  }

  /// Create a weak handle with a finalization callback installed, which is
  /// guaranteed to run at some point.
  ///
  /// Unlike [`Self::with_finalizer`], whose finalization callbacks are not
  /// guaranteed to run, this method is guaranteed to be called before the
  /// isolate is destroyed. It can therefore be used for critical resource
  /// management. Note that other than that, there is still no guarantee as to
  /// *when* the callback will be called.
  ///
  /// Unlike regular finalizers, guaranteed finalizers aren't passed a mutable
  /// [`Isolate`] reference, since they might be called when the isolate is
  /// being destroyed, at which point it might be no longer valid to use.
  /// Accessing the isolate (with unsafe code) from the finalizer callback is
  /// therefore unsound, unless you prove the isolate is not being destroyed.
  pub fn with_guaranteed_finalizer(
    isolate: &mut Isolate,
    handle: impl Handle<Data = T>,
    finalizer: Box<dyn FnOnce()>,
  ) -> Self {
    let HandleInfo { data, host } = handle.get_handle_info();
    host.assert_match_isolate(isolate);
    Self::assert_supported(isolate);
    let finalizer_id = isolate
      .get_finalizer_map_mut()
      .add(FinalizerCallback::Guaranteed(finalizer));
    Self::new_raw(isolate, data, Some(finalizer_id))
  }

  fn new_raw(
    isolate: *mut Isolate,
    data: NonNull<T>,
    finalizer_id: Option<FinalizerId>,
  ) -> Self {
    Self::assert_supported(isolate);
    unsafe { *(*isolate).live_weak_count_mut() += 1 };
    let weak_data = Box::new(WeakData {
      pointer: Default::default(),
      finalizer_id,
      weak_dropped: Cell::new(false),
    });
    let data = data.cast().as_ptr();
    let data = unsafe {
      v8__Global__NewWeak(
        (*isolate).as_real_ptr(),
        data,
        weak_data.deref() as *const _ as *const c_void,
        Self::first_pass_callback,
      )
    };
    weak_data
      .pointer
      .set(Some(unsafe { NonNull::new_unchecked(data as *mut _) }));
    Self {
      data: Some(weak_data),
      isolate_handle: unsafe { (*isolate).thread_safe_handle() },
    }
  }

  fn assert_supported(isolate: *mut Isolate) {
    // Weak callbacks fire during GC on whichever thread holds a shared
    // isolate's lock, racing the `WeakData` owned by this (non-Send)
    // handle on its home thread.
    assert!(
      !unsafe { (*isolate).global_liveness().as_ref() }.is_shared(),
      "v8::Weak is not supported on shared isolates"
    );
  }

  /// Creates a new empty handle, identical to one for an object that has
  /// already been GC'd.
  pub fn empty(isolate: &mut Isolate) -> Self {
    Weak {
      data: None,
      isolate_handle: isolate.thread_safe_handle(),
    }
  }

  /// Clones this handle and installs a finalizer callback on the clone, as if
  /// by calling [`Self::with_finalizer`].
  ///
  /// Note that if this handle is empty (its value has already been GC'd), the
  /// finalization callback will never run.
  pub fn clone_with_finalizer(
    &self,
    finalizer: Box<dyn FnOnce(&mut Isolate)>,
  ) -> Self {
    self.clone_raw(Some(FinalizerCallback::Regular(finalizer)))
  }

  /// Clones this handle and installs a guaranteed finalizer callback on the
  /// clone, as if by calling [`Self::with_guaranteed_finalizer`].
  ///
  /// Note that if this handle is empty (its value has already been GC'd), the
  /// finalization callback will never run.
  pub fn clone_with_guaranteed_finalizer(
    &self,
    finalizer: Box<dyn FnOnce()>,
  ) -> Self {
    self.clone_raw(Some(FinalizerCallback::Guaranteed(finalizer)))
  }

  fn clone_raw(&self, finalizer: Option<FinalizerCallback>) -> Self {
    if let Some(data) = self.get_pointer() {
      // SAFETY: We're in the isolate's thread, because Weak<T> isn't Send or
      // Sync.
      let isolate_ptr = unsafe { self.isolate_handle.get_isolate_ptr() };
      if isolate_ptr.is_null() {
        unreachable!("Isolate was dropped but weak handle wasn't reset.");
      }
      let mut isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
      Self::assert_supported(&mut isolate);
      let finalizer_id = finalizer
        .map(|finalizer| isolate.get_finalizer_map_mut().add(finalizer));
      Self::new_raw(&mut isolate, data, finalizer_id)
    } else {
      Weak {
        data: None,
        isolate_handle: self.isolate_handle.clone(),
      }
    }
  }

  /// Converts an optional raw pointer created with [`Weak::into_raw()`] back to
  /// its original `Weak`.
  ///
  /// This method is called with `Some`, the pointer is invalidated and it
  /// cannot be used with this method again. Additionally, it is unsound to call
  /// this method with an isolate other than that in which the original `Weak`
  /// was created.
  ///
  /// # Panics
  ///
  /// Panics if called with `Some` for a shared isolate.
  pub unsafe fn from_raw(
    isolate: &mut Isolate,
    data: Option<NonNull<WeakData<T>>>,
  ) -> Self {
    if data.is_some() {
      Self::assert_supported(isolate);
    }
    Weak {
      data: data.map(|raw| unsafe { Box::from_raw(raw.cast().as_ptr()) }),
      isolate_handle: isolate.thread_safe_handle(),
    }
  }

  /// Consume this `Weak` handle and return the underlying raw pointer, or
  /// `None` if the value has been GC'd.
  ///
  /// The return value can be converted back into a `Weak` by using
  /// [`Weak::from_raw`]. Note that `Weak` allocates some memory, and if this
  /// method returns `Some`, the pointer must be converted back into a `Weak`
  /// for it to be freed.
  ///
  /// Note that this method might return `Some` even after the V8 value has been
  /// GC'd.
  pub fn into_raw(mut self) -> Option<NonNull<WeakData<T>>> {
    if let Some(data) = self.data.take() {
      let has_finalizer = if let Some(finalizer_id) = data.finalizer_id {
        // SAFETY: We're in the isolate's thread because Weak isn't Send or Sync
        let isolate_ptr = unsafe { self.isolate_handle.get_isolate_ptr() };
        if isolate_ptr.is_null() {
          // Disposed isolates have no finalizers.
          false
        } else {
          let isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
          isolate.get_finalizer_map().map.contains_key(&finalizer_id)
        }
      } else {
        false
      };

      if data.pointer.get().is_none() && !has_finalizer {
        // If the pointer is None and we're not waiting for the second pass,
        // drop the box and release its count. A `Some(raw)` return keeps the
        // count until `from_raw` re-adopts the box and the resulting `Weak`
        // is dropped; leaking the raw pointer conservatively keeps sharing
        // disabled.
        // SAFETY: we're in the isolate's thread because `Weak` isn't Send or
        // Sync.
        let isolate_ptr = unsafe { self.isolate_handle.get_isolate_ptr() };
        if !isolate_ptr.is_null() {
          let mut isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
          isolate.release_live_weak();
        }
        None
      } else {
        assert!(!data.weak_dropped.get());
        Some(unsafe { NonNull::new_unchecked(Box::into_raw(data)) })
      }
    } else {
      None
    }
  }

  fn get_pointer(&self) -> Option<NonNull<T>> {
    if let Some(data) = &self.data {
      // It seems like when the isolate is dropped, even the first pass callback
      // might not be called.
      if unsafe { self.isolate_handle.get_isolate_ptr() }.is_null() {
        None
      } else {
        data.pointer.get()
      }
    } else {
      None
    }
  }

  pub fn is_empty(&self) -> bool {
    self.get_pointer().is_none()
  }

  pub fn to_global(&self, isolate: &mut Isolate) -> Option<Global<T>> {
    if let Some(data) = self.get_pointer() {
      let handle_host: HandleHost = (&self.isolate_handle).into();
      handle_host.assert_match_isolate(isolate);
      Some(unsafe { Global::new_raw(isolate, data) })
    } else {
      None
    }
  }

  pub fn to_local<'s>(
    &self,
    scope: &PinScope<'s, '_, ()>,
  ) -> Option<Local<'s, T>> {
    if let Some(data) = self.get_pointer() {
      let handle_host: HandleHost = (&self.isolate_handle).into();
      handle_host.assert_match_isolate(scope);
      let local = unsafe {
        scope.cast_local(|sd| {
          v8__Local__New(sd.get_isolate_ptr(), data.cast().as_ptr()) as *const T
        })
      };
      Some(local.unwrap())
    } else {
      None
    }
  }

  // Finalization callbacks.

  unsafe extern "C" fn first_pass_callback(wci: *const WeakCallbackInfo) {
    // SAFETY: If this callback is called, then the weak handle hasn't been
    // reset, which means the `Weak` instance which owns the pinned box that the
    // parameter points to hasn't been dropped.
    let weak_data = unsafe {
      let ptr = v8__WeakCallbackInfo__GetParameter(wci);
      &*(ptr as *mut WeakData<T>)
    };

    let data = weak_data.pointer.take().unwrap();
    unsafe {
      v8__Global__Reset(data.cast().as_ptr());
    }

    // Only set the second pass callback if there could be a finalizer.
    if weak_data.finalizer_id.is_some() {
      unsafe {
        v8__WeakCallbackInfo__SetSecondPassCallback(
          wci,
          Self::second_pass_callback,
        );
      };
    }
  }

  unsafe extern "C" fn second_pass_callback(wci: *const WeakCallbackInfo) {
    // SAFETY: This callback is guaranteed by V8 to be called in the isolate's
    // thread before the isolate is disposed.
    let isolate = unsafe { v8__WeakCallbackInfo__GetIsolate(wci) };

    // SAFETY: This callback might be called well after the first pass callback,
    // which means the corresponding Weak might have been dropped. In Weak's
    // Drop impl we make sure that if the second pass callback hasn't yet run, the
    // Box<WeakData<T>> is leaked, so it will still be alive by the time this
    // callback is called.
    let weak_data = unsafe {
      let ptr = v8__WeakCallbackInfo__GetParameter(wci);
      &*(ptr as *mut WeakData<T>)
    };

    let mut isolate = unsafe { Isolate::from_raw_ptr(isolate) };
    let finalizer: Option<FinalizerCallback> = {
      let finalizer_id = weak_data.finalizer_id.unwrap();
      isolate.get_finalizer_map_mut().map.remove(&finalizer_id)
    };

    if weak_data.weak_dropped.get() {
      // SAFETY: If weak_dropped is true, the corresponding Weak has been dropped,
      // so it's safe to take ownership of the Box<WeakData<T>> and drop it.
      let _ = unsafe {
        Box::from_raw(weak_data as *const WeakData<T> as *mut WeakData<T>)
      };
    }

    match finalizer {
      Some(FinalizerCallback::Regular(finalizer)) => finalizer(&mut isolate),
      Some(FinalizerCallback::Guaranteed(finalizer)) => finalizer(),
      None => {}
    }
  }
}

impl<T> Clone for Weak<T> {
  fn clone(&self) -> Self {
    self.clone_raw(None)
  }
}

impl<T> Drop for Weak<T> {
  fn drop(&mut self) {
    // `data` is `Some` iff this handle was created through `new_raw` and
    // thus counted. SAFETY: we're in the isolate's thread because `Weak`
    // isn't Send or Sync.
    if self.data.is_some() {
      let isolate_ptr = unsafe { self.isolate_handle.get_isolate_ptr() };
      if !isolate_ptr.is_null() {
        let mut isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
        isolate.release_live_weak();
      }
    }

    // Returns whether the finalizer existed.
    let remove_finalizer = |finalizer_id: Option<FinalizerId>| -> bool {
      if let Some(finalizer_id) = finalizer_id {
        // SAFETY: We're in the isolate's thread because `Weak` isn't Send or Sync.
        let isolate_ptr = unsafe { self.isolate_handle.get_isolate_ptr() };
        if !isolate_ptr.is_null() {
          let mut isolate = unsafe { Isolate::from_raw_ptr(isolate_ptr) };
          let finalizer =
            isolate.get_finalizer_map_mut().map.remove(&finalizer_id);
          return finalizer.is_some();
        }
      }
      false
    };

    if let Some(data) = self.get_pointer() {
      // If the pointer is not None, the first pass callback hasn't been
      // called yet, and resetting will prevent it from being called.
      unsafe { v8__Global__Reset(data.cast().as_ptr()) };
      remove_finalizer(self.data.as_ref().unwrap().finalizer_id);
    } else if let Some(weak_data) = self.data.take() {
      // The second pass callback removes the finalizer, so if there is one,
      // the second pass hasn't yet run, and WeakData will have to be alive.
      // In that case we leak the WeakData but remove the finalizer.
      if remove_finalizer(weak_data.finalizer_id) {
        weak_data.weak_dropped.set(true);
        Box::leak(weak_data);
      }
    }
  }
}

impl<T> Eq for Weak<T> where T: Eq {}

impl<T, Rhs: Handle> PartialEq<Rhs> for Weak<T>
where
  T: PartialEq<Rhs::Data>,
{
  fn eq(&self, other: &Rhs) -> bool {
    let HandleInfo {
      data: other_data,
      host: other_host,
    } = other.get_handle_info();
    let self_host: HandleHost = (&self.isolate_handle).into();
    if !self_host.match_host(other_host, None) {
      false
    } else if let Some(self_data) = self.get_pointer() {
      unsafe { self_data.as_ref() == other_data.as_ref() }
    } else {
      false
    }
  }
}

impl<T, T2> PartialEq<Weak<T2>> for Weak<T>
where
  T: PartialEq<T2>,
{
  fn eq(&self, other: &Weak<T2>) -> bool {
    let self_host: HandleHost = (&self.isolate_handle).into();
    let other_host: HandleHost = (&other.isolate_handle).into();
    if !self_host.match_host(other_host, None) {
      return false;
    }
    match (self.get_pointer(), other.get_pointer()) {
      (Some(self_data), Some(other_data)) => unsafe {
        self_data.as_ref() == other_data.as_ref()
      },
      (None, None) => true,
      _ => false,
    }
  }
}

/// The inner mechanism behind [`Weak`] and finalizations.
///
/// This struct is heap-allocated and will not move until it's dropped, so it
/// can be accessed by the finalization callbacks by creating a shared reference
/// from a pointer. The fields are wrapped in [`Cell`] so they are modifiable by
/// both the [`Weak`] and the finalization callbacks.
pub struct WeakData<T> {
  pointer: Cell<Option<NonNull<T>>>,
  finalizer_id: Option<FinalizerId>,
  weak_dropped: Cell<bool>,
}

impl<T> std::fmt::Debug for WeakData<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("WeakData")
      .field("pointer", &self.pointer)
      .finish_non_exhaustive()
  }
}

#[repr(C)]
struct WeakCallbackInfo(Opaque);

type FinalizerId = usize;

pub(crate) enum FinalizerCallback {
  Regular(Box<dyn FnOnce(&mut Isolate)>),
  Guaranteed(Box<dyn FnOnce()>),
}

#[derive(Default)]
pub(crate) struct FinalizerMap {
  map: std::collections::HashMap<FinalizerId, FinalizerCallback>,
  next_id: FinalizerId,
}

impl FinalizerMap {
  pub(crate) fn is_empty(&self) -> bool {
    self.map.is_empty()
  }

  fn add(&mut self, finalizer: FinalizerCallback) -> FinalizerId {
    let id = self.next_id;
    // TODO: Overflow.
    self.next_id += 1;
    self.map.insert(id, finalizer);
    id
  }

  pub(crate) fn drain(
    &mut self,
  ) -> impl Iterator<Item = FinalizerCallback> + '_ {
    self.map.drain().map(|(_, finalizer)| finalizer)
  }
}

/// Panics unless the `TracedReference` or `Eternal` whose isolate is `set_in`
/// is empty or was set in `isolate`.
fn assert_set_in_isolate(set_in: &AtomicUsize, isolate: &Isolate) {
  let set_in = set_in.load(Ordering::Relaxed);
  assert!(
    set_in == 0 || set_in == isolate.global_liveness().as_ptr() as usize,
    "attempt to get a handle in an Isolate other than the one it was set in"
  );
}

/// A traced handle without destructor that clears the handle. The embedder needs
/// to ensure that the handle is not accessed once the V8 object has been
/// reclaimed. For more details see BasicTracedReference.
#[repr(C)]
pub struct TracedReference<T> {
  data: [u8; crate::binding::v8__TracedReference_SIZE],
  // The address of the `IsolateLiveness` of the isolate the reference was set
  // in, or 0 if it is empty. Unlike isolate addresses, these are never
  // reused.
  isolate: AtomicUsize,
  _phantom: PhantomData<T>,
}

impl<T> TracedReference<T> {
  /// An empty TracedReference without storage cell.
  pub fn empty() -> Self {
    let mut this = std::mem::MaybeUninit::<Self>::uninit();
    unsafe {
      v8__TracedReference__CONSTRUCT(this.as_mut_ptr() as _);
      std::ptr::addr_of_mut!((*this.as_mut_ptr()).isolate)
        .write(AtomicUsize::new(0));
      this.assume_init()
    }
  }

  /// Construct a TracedReference from a Local.
  ///
  /// A new storage cell is created pointing to the same object.
  pub fn new<'s>(scope: &PinScope<'s, '_, ()>, data: Local<'s, T>) -> Self {
    let mut this = Self::empty();
    this.reset(scope, Some(data));
    this
  }

  /// # Panics
  ///
  /// Panics if the reference was set in another `Isolate`.
  pub fn get<'s>(&self, scope: &PinScope<'s, '_, ()>) -> Option<Local<'s, T>> {
    assert_set_in_isolate(&self.isolate, scope);
    unsafe {
      scope.cast_local(|sd| {
        v8__TracedReference__Get(
          self as *const Self as *const TracedReference<Data>,
          sd.get_isolate_ptr(),
        ) as *const T
      })
    }
  }

  /// Always resets the reference. Creates a new reference from `other` if it is
  /// non-empty.
  ///
  /// # Panics
  ///
  /// Panics if the reference was set in another `Isolate`.
  pub fn reset<'s>(
    &mut self,
    scope: &PinScope<'s, '_, ()>,
    data: Option<Local<'s, T>>,
  ) {
    assert_set_in_isolate(&self.isolate, scope);
    let isolate = match data {
      Some(_) => scope.global_liveness().as_ptr() as usize,
      None => 0,
    };
    self.isolate.store(isolate, Ordering::Relaxed);
    unsafe {
      v8__TracedReference__Reset(
        self as *mut Self as *mut TracedReference<Data>,
        scope.get_isolate_ptr(),
        data
          .map_or(std::ptr::null_mut(), |h| h.as_non_null().as_ptr())
          .cast(),
      );
    }
  }
}

impl<T> Drop for TracedReference<T> {
  fn drop(&mut self) {
    unsafe {
      v8__TracedReference__DESTRUCT(
        self as *mut Self as *mut TracedReference<Data>,
      );
    }
  }
}

/// Eternal handles are set-once handles that live for the lifetime of the isolate.
#[repr(C)]
pub struct Eternal<T> {
  data: [u8; crate::binding::v8__Eternal_SIZE],
  // The address of the `IsolateLiveness` of the isolate the handle was set
  // in, or 0 if it is empty. An eternal handle is an index into its isolate's
  // table of eternal handles, which means nothing in another isolate.
  isolate: AtomicUsize,
  _phantom: PhantomData<T>,
}

impl<T> Eternal<T> {
  pub fn empty() -> Self {
    let mut this = std::mem::MaybeUninit::<Self>::uninit();
    unsafe {
      v8__Eternal__CONSTRUCT(this.as_mut_ptr() as _);
      std::ptr::addr_of_mut!((*this.as_mut_ptr()).isolate)
        .write(AtomicUsize::new(0));
      this.assume_init()
    }
  }

  pub fn clear(&self) {
    self.isolate.store(0, Ordering::Relaxed);
    unsafe {
      v8__Eternal__Clear(self as *const Self as *mut Eternal<Data>);
    }
  }

  pub fn set<'s>(&self, scope: &PinScope<'s, '_, ()>, data: Local<'s, T>) {
    self
      .isolate
      .store(scope.global_liveness().as_ptr() as usize, Ordering::Relaxed);
    unsafe {
      v8__Eternal__Set(
        self as *const Self as *mut Eternal<Data>,
        scope.get_isolate_ptr(),
        data.as_non_null().as_ptr().cast(),
      );
    }
  }

  /// # Panics
  ///
  /// Panics if the handle was set in another `Isolate`.
  pub fn get<'s>(&self, scope: &PinScope<'s, '_, ()>) -> Option<Local<'s, T>> {
    assert_set_in_isolate(&self.isolate, scope);
    unsafe {
      scope.cast_local(|sd| {
        v8__Eternal__Get(
          self as *const Self as *const Eternal<Data>,
          sd.get_isolate_ptr(),
        ) as *const T
      })
    }
  }

  pub fn is_empty(&self) -> bool {
    unsafe { v8__Eternal__IsEmpty(self as *const Self as *const Eternal<Data>) }
  }
}

impl<T> Drop for Eternal<T> {
  fn drop(&mut self) {
    unsafe {
      v8__Eternal__DESTRUCT(self as *mut Self as *mut Eternal<Data>);
    }
  }
}

/// A Local<T> passed from V8 without an inherent scope.
/// The value must be "unsealed" with Scope::unseal to bind
/// it to a lifetime.
#[derive(Debug)]
#[repr(transparent)]
pub struct SealedLocal<T>(pub(crate) NonNull<T>);
