//! This module contains the rust wrappers for V8's scope types.
//!
//! There are two main types of scopes, with the other types being derived from them:
//! - `HandleScope` - a scope to create and access `Local` handles
//! - `TryCatch` - a scope to catch exceptions thrown from javascript
//!
//! There are a few important properties that make v8 scopes challenging to model in rust.
//! - `HandleScope`s can (and almost certainly will be) nested, but handles are only
//!   bound to the innermost `HandleScope`
//!   - Importantly, this means that the Handle lifetimes are determined by the innermost `HandleScope`
//! - Both `HandleScope` and `TryCatch`  cannot be moved, because V8 holds direct pointers to them
//! - The C++ API relies heavily on inheritance, which is a bit awkward to model in rust
//!
//! # Example
//!
//! ```rust
//! use v8::{Context, ContextScope, HandleScope, Isolate, Object};
//! # let platform = v8::new_default_platform(0, false).make_shared();
//! # v8::V8::initialize_platform(platform);
//! # v8::V8::initialize();
//!
//! let isolate = &mut Isolate::new(Default::default());
//! let scope = HandleScope::new(isolate);
//! let scope = std::pin::pin!(scope);
//! let mut scope = scope.init();
//! let context = Context::new(&scope, Default::default());
//!
//! let context_scope = ContextScope::new(&mut scope, context);
//! let object = Object::new(&context_scope);
//! ```
//!
//! ## Explanation
//! The first thing you'll notice is that creating a `HandleScope` requires a few different steps. You'll see this pattern
//! across all scope types that are address-sensitive (all except for `ContextScope`):
//!
//! 1. Allocate the storage for the scope. At this point, the scope is not yet address-sensitive, and so it can be safely moved.
//! 2. Pin the storage to the stack. This is necessary because once we initialize the scope, it must not be moved.
//! 3. Initialize the scope. This is where the scope is actually initialized, and our `Pin` ensures that the scope cannot be moved.
//!
//! This is a bit verbose, so you can collapse it into two lines,
//! ```rust
//! # use v8::{HandleScope, Isolate};
//! # let platform = v8::new_default_platform(0, false).make_shared();
//! # v8::V8::initialize_platform(platform);
//! # v8::V8::initialize();
//! # let isolate = &mut Isolate::new(Default::default());
//! let scope = std::pin::pin!(HandleScope::new(isolate));
//! let mut scope = scope.init();
//! ```
//!
//! or use the provided macros:
//! ```rust
//! # use v8::Isolate;
//! # let platform = v8::new_default_platform(0, false).make_shared();
//! # v8::V8::initialize_platform(platform);
//! # v8::V8::initialize();
//! # let isolate = &mut Isolate::new(Default::default());
//! // note that this expands into statements, introducing a new variable `scope` into the current
//! // block. Using it as an expression (`let scope = v8::scope!(let scope, isolate);`) will not work
//! v8::scope!(let scope, isolate);
//! ```
//!
//! # Scopes as function args
//! In a function that takes a scope, you'll typically want to take a `PinScope`, like
//! ```rust
//! fn foo<'s, 'i>(scope: &mut v8::PinScope<'s, 'i>) {
//!   let local = v8::Number::new(scope, 42.0);
//! }
//! ```
//!
//! `PinScope` is just a shorthand for `PinnedRef<'s, HandleScope<'i>>`, which you can use if you really prefer.
//!
//! The lifetimes can sometimes be elided, but if you are taking or returning a `Local`, you'll need to specify at least the first one.
//! ```
//! fn foo<'s>(scope: &mut v8::PinScope<'s, '_>, arg: v8::Local<'s, v8::Number>) -> v8::Local<'s, v8::Number> {
//!   v8::Number::new(scope, arg.value() + 42.0)
//! }
//! ```
//!
//! # Deref/DerefMut
//!
//! Scopes implement `Deref` and `DerefMut` to allow for sort of "inheritance" of behavior. This is useful because
//! it allows most methods (as mentioned above) to just take a `PinScope`, and other scopes will deref to `PinScope`.
//!
//! That lets you seamlessly pass, for instance, a `ContextScope` to a function that takes a `PinScope`.
//! Note that pinned scopes do not implement `Deref` or `DerefMut`, themselves, rather `PinnedRef` does.
//!
//! The deref implementations are:
//!
//! PinnedRef<'_, HandleScope<'_, ()>> -> Isolate
//! PinnedRef<'_, HandleScope<'_>> -> PinnedRef<'_, HandleScope<'_, ()>>
//! PinnedRef<'_, ContextScope<'_, '_>> -> PinnedRef<'_, HandleScope<'_>>
//! PinnedRef<'_, CallbackScope<'_, '_>> -> PinnedRef<'_, HandleScope<'_, ()>>
//!
//!
//!
//! # Internals
//!
//! The initialization process uses the typestate pattern. The storage for the scope is a `ScopeStorage` struct, which is
//! then transititions to a `Pin<&mut ScopeStorage<T>>`, and then `init` transitions to a `PinnedRef<'s, T>`.
//!
//! The `ScopeStorage` struct tracks initialization state, and is responsible for calling the destructor when the storage is dropped
//! (iff the scope was initialized).
//!
//! The `PinnedRef` type, returned from `init`, is a transparent wrapper around a `Pin<&mut T>`. The reason it is a newtype is so
//! that it can have specialized Deref/DerefMut implementations for the different scope types. `Pin` has a blanket implementation
//! that doesn't have the behavior we want.
//!
//! ## Lifetimes
//!
//! The trickiest part of the scopes here are the lifetimes. In general, the lifetimes exist for a few reasons:
//! - ensure that a scope can't outlive the thing it's made from (e.g. an isolate, or another scope)
//! - ensure that a scope higher up the stack can't be used until the scope below it has dropped
//! - ensure that the `Handle`s bound to the scope do not outlive the scope
//!
//! These lifetimes do not need to be exact, and in some cases I'm sure they are shorter than they could be,
//! as long as everything lives long enough. In other words, the lifetimes just need to be a safe approximation.
//!
//!
//! ### HandleScope
//! `HandleScope` itself has only one lifetime, `'i` which is the lifetime of the thing that the scope was created from
//! (e.g. an isolate).
//!
//! The lifetime for handles bound to the scope is really the lifetime of the `HandleScope` itself. In our case,
//! since we've pinned it to the stack, that is the lifetime of the pinned reference. So in
//! `PinnedRef<'s, HandleScope<'i>>`, `'s` is the lifetime of the pinned reference, and therefore
//! the handles, and 'i is the lifetime of the isolate.
//!
//! ### ContextScope
//! ContextScope is really just a wrapper around another scope, with a `Context` added to it.
//! It wraps a scope, and so it is not actually address-sensitive, and can be moved around freely.
//!
//! ContextScope has two lifetimes, `'b` and `'s`. `'b` is the lifetime of the borrow of the scope
//! it's wrapping, and `'s` is the lifetime of the scope.
//!
//! Effectively you have `&'b PinnedRef<'s, T>`.
//!
//! The lifetime for handles bound to the scope is the lifetime of the scope that it was created from.
//! So in `ContextScope<'b, 's>`, `'b` is the lifetime of the borrow of the inner scope, and `'s` is the lifetime of the inner scope (and therefore the handles).
//!
//! ### Isolates
//! A handle's lifetime ties it to a scope, but not to the `Isolate` that scope belongs to, so the
//! type system alone would allow a handle of one `Isolate` to be passed to an API call on another
//! one, which corrupts both heaps. To rule that out, all scopes that are open on a thread at the
//! same time must belong to the same `Isolate`: opening a `HandleScope`, `EscapableHandleScope` or
//! `CallbackScope` while a scope of another `Isolate` is open on the same thread panics. Since a
//! handle cannot outlive its scope, no handle of another `Isolate` can then be alive. Handles that
//! outlive scopes (`Global`, `Weak`, `TracedReference` and `Eternal`) remember their `Isolate`, and
//! panic when created from or turned into a `Local` in a scope of another `Isolate`. Debug builds
//! additionally check, whenever a `Local` is dereferenced, that its value can be used in the
//! `Isolate` that owns the open scopes: it lives in that `Isolate`'s heap, it is a value that all
//! `Isolate`s of its `IsolateGroup` share (such as `undefined`), or it is a Smi.
//!
//! Code that has to drive another `Isolate` while scopes of the first are open can do so inside
//! the `unsafe` [`allow_scopes_of_another_isolate`]. A scope that is leaked (for example with
//! `std::mem::forget`) counts as open for the rest of the thread's life, so scopes of other
//! `Isolate`s can no longer be opened on that thread.
use crate::{
  Context, Data, DataError, Function, FunctionCallbackInfo, Isolate, Local,
  Message, Object, OwnedIsolate, PromiseRejectMessage, PropertyCallbackInfo,
  SealedLocal, Value,
  fast_api::FastApiCallbackOptions,
  function::FunctionCallbackInfoParts,
  isolate::{IsolateAnnex, RealIsolate},
  support::assert_layout_subset,
};
use std::{
  any::type_name,
  cell::Cell,
  marker::{PhantomData, PhantomPinned},
  mem::ManuallyDrop,
  ops::{Deref, DerefMut},
  pin::Pin,
  ptr::NonNull,
};
pub(crate) mod raw;

/// Tracks which `Isolate` owns the scopes that are open on the current thread
/// (see "Isolates" in the module documentation).
pub(crate) mod scope_owner {
  use std::cell::Cell;
  use std::ptr::NonNull;

  use crate::isolate::RealIsolate;

  thread_local! {
    // The `Isolate` that owns the open scopes, and how many scopes are open.
    static OWNER: Cell<(*const RealIsolate, usize)> =
      const { Cell::new((std::ptr::null(), 0)) };
  }

  /// Called when a scope of `isolate` is opened.
  ///
  /// # Panics
  ///
  /// Panics if a scope of another `Isolate` is open on this thread.
  #[inline]
  pub(super) fn enter(isolate: NonNull<RealIsolate>) {
    let (owner, depth) = OWNER.get();
    if depth != 0 && owner != isolate.as_ptr().cast_const() {
      panic!(
        "cannot open a scope on an Isolate while a scope of another Isolate \
         is open on the same thread"
      );
    }
    OWNER.set((isolate.as_ptr(), depth + 1));
  }

  /// Returns the `Isolate` that owns the scopes open on this thread, if any.
  #[inline]
  pub(crate) fn current() -> Option<NonNull<RealIsolate>> {
    let (owner, depth) = OWNER.get();
    if depth == 0 {
      None
    } else {
      NonNull::new(owner.cast_mut())
    }
  }

  /// Runs `f` with no scopes counted as open on this thread, and restores
  /// the count when `f` returns or unwinds.
  pub(super) fn suspend<R>(f: impl FnOnce() -> R) -> R {
    struct Restore((*const RealIsolate, usize));
    impl Drop for Restore {
      fn drop(&mut self) {
        debug_assert_eq!(OWNER.get().1, 0, "a scope was left open");
        OWNER.set(self.0);
      }
    }
    let _restore = Restore(OWNER.replace((std::ptr::null(), 0)));
    f()
  }

  /// Called when a scope opened with `enter()` is closed.
  #[inline]
  pub(super) fn exit() {
    let (owner, depth) = OWNER.get();
    debug_assert!(depth > 0);
    if depth == 1 {
      OWNER.set((std::ptr::null(), 0));
    } else {
      OWNER.set((owner, depth - 1));
    }
  }

  #[cfg(debug_assertions)]
  unsafe extern "C" {
    fn v8__Data__IsCompatibleWithIsolate(
      this: *const crate::Data,
      isolate: *const RealIsolate,
    ) -> bool;
  }

  /// Checks that the value of a dereferenced `Local` can be used in the
  /// `Isolate` that owns the open scopes.
  ///
  /// # Panics
  ///
  /// Panics if it can't.
  #[cfg(debug_assertions)]
  pub(crate) fn check_handle(data: NonNull<crate::Data>) {
    let (owner, depth) = OWNER.get();
    if depth == 0 {
      return;
    }
    // SAFETY: `data` points to a live handle and `owner` to a live isolate.
    if !unsafe { v8__Data__IsCompatibleWithIsolate(data.as_ptr(), owner) } {
      panic!(
        "a handle of one Isolate was used while a scope of another Isolate \
         is open"
      );
    }
  }
}

/// Runs `f` with the scopes that are open on this thread set aside, so that
/// `f` can open scopes of an `Isolate` other than the one they belong to (see
/// "Isolates" in the module documentation). They count as open again when `f`
/// returns.
///
/// # Safety
///
/// While `f` runs, handles of the `Isolate`s whose scopes were set aside must
/// not be used with any other `Isolate`, and handles that `f` creates must not
/// be used once it returns. `f` must close every scope it opens.
pub unsafe fn allow_scopes_of_another_isolate<R>(f: impl FnOnce() -> R) -> R {
  scope_owner::suspend(f)
}

pub type PinScope<'s, 'i, C = Context> = PinnedRef<'s, HandleScope<'i, C>>;
pub type PinCallbackScope<'s, 'i, C = Context> =
  PinnedRef<'s, CallbackScope<'i, C>>;

/// Storage for a scope.
///
/// Tracks the initialization state of the scope, and holds the scope itself.
#[repr(C)]
pub struct ScopeStorage<T: ScopeInit> {
  inited: bool,
  scope: ManuallyDrop<T>,
  _pinned: PhantomPinned,
}

impl<T: ScopeInit> ScopeStorage<T> {
  pub(crate) fn projected(self: Pin<&mut Self>) -> Pin<&mut T> {
    // SAFETY: we are just projecting to a field, so the scope remains pinned
    unsafe {
      let self_mut = self.get_unchecked_mut();
      Pin::new_unchecked(&mut self_mut.scope)
    }
  }

  pub fn new(scope: T) -> Self {
    Self {
      inited: false,
      scope: ManuallyDrop::new(scope),
      _pinned: PhantomPinned,
    }
  }

  pub fn init(mut self: Pin<&mut Self>) -> PinnedRef<'_, T> {
    if self.inited {
      // free old, going to reuse this storage
      unsafe {
        let self_mut = self.as_mut().get_unchecked_mut();
        self_mut.drop_inner();
        self_mut.inited = false;
      }
    }

    // hold onto a pointer so we can set this after initialization. we can't use a normal
    // mutable reference because the borrow checker will see overlapping borrows. this is
    // safe, however, because we lose our mutable reference to the storage in `init_stack`
    // as it gets projected to the inner type
    let inited_ptr =
      unsafe { &raw mut self.as_mut().get_unchecked_mut().inited };
    let ret = T::init_stack(self);
    unsafe { inited_ptr.write(true) };
    PinnedRef(ret)
  }

  /// SAFEFTY: `self.inited` must be true, and therefore must be pinned
  unsafe fn drop_inner(&mut self) {
    unsafe {
      T::deinit(&mut self.scope);
    }
    self.inited = false;
  }
}

impl<T: ScopeInit> Drop for ScopeStorage<T> {
  fn drop(&mut self) {
    if self.inited {
      unsafe {
        self.drop_inner();
      }
    }
  }
}

pub trait Scope: Sized + sealed::Sealed + ScopeInit {}

mod sealed {
  pub trait Sealed {}
}

pub trait ScopeInit: Sized {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self>;

  unsafe fn deinit(me: &mut Self);
}

impl<C> ScopeInit for HandleScope<'_, C> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    // SAFETY: no moving the scope from this point on
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    scope_owner::enter(storage_mut.scope.isolate);
    unsafe {
      let isolate = storage_mut.scope.isolate;
      raw::HandleScope::init(&mut storage_mut.scope.raw_handle_scope, isolate)
    };

    let projected = &mut storage_mut.scope;

    // SAFETY: scope is still pinned
    unsafe { Pin::new_unchecked(projected) }
  }

  unsafe fn deinit(me: &mut Self) {
    unsafe { raw::v8__HandleScope__DESTRUCT(&mut me.raw_handle_scope) };
    scope_owner::exit();
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
#[repr(C)]
#[derive(Debug)]
pub struct HandleScope<'s, C = Context> {
  raw_handle_scope: raw::HandleScope,
  isolate: NonNull<RealIsolate>,
  annex: NonNull<IsolateAnnex>,
  context: Cell<Option<NonNull<Context>>>,
  _phantom: PhantomData<&'s mut C>,
  _pinned: PhantomPinned,
}

impl<C> sealed::Sealed for HandleScope<'_, C> {}
impl<C> Scope for HandleScope<'_, C> {}

mod get_isolate {
  use crate::RealIsolate;
  pub trait GetIsolate {
    fn get_isolate_ptr(&self) -> *mut RealIsolate;
  }
}
pub(crate) use get_isolate::GetIsolate;

mod get_isolate_impls {
  use crate::{Promise, PromiseRejectMessage};

  use super::*;
  impl GetIsolate for Isolate {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.as_real_ptr()
    }
  }

  impl GetIsolate for OwnedIsolate {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.as_real_ptr()
    }
  }

  impl GetIsolate for FunctionCallbackInfo {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.get_isolate_ptr()
    }
  }

  impl GetIsolate for FunctionCallbackInfoParts<'_> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.isolate.as_real_ptr()
    }
  }

  impl<T> GetIsolate for PropertyCallbackInfo<T> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.get_isolate_ptr()
    }
  }

  impl GetIsolate for FastApiCallbackOptions<'_> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.isolate
    }
  }

  impl GetIsolate for Local<'_, Context> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      unsafe { raw::v8__Isolate__GetCurrent() }
    }
  }

  impl GetIsolate for Local<'_, Message> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      unsafe { raw::v8__Isolate__GetCurrent() }
    }
  }

  impl GetIsolate for Local<'_, Promise> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      unsafe { raw::v8__Isolate__GetCurrent() }
    }
  }

  impl GetIsolate for PromiseRejectMessage<'_> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      unsafe { raw::v8__Isolate__GetCurrent() }
    }
  }

  impl<C> GetIsolate for HandleScope<'_, C> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.isolate.as_ptr()
    }
  }

  impl<P: GetIsolate + ClearCachedContext> GetIsolate
    for ContextScope<'_, '_, P>
  {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.scope.get_isolate_ptr()
    }
  }

  impl<P: GetIsolate> GetIsolate for &mut P {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      P::get_isolate_ptr(self)
    }
  }

  impl<P: GetIsolate> GetIsolate for &P {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      P::get_isolate_ptr(self)
    }
  }

  impl<P: GetIsolate> GetIsolate for TryCatch<'_, '_, P> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.scope.get_isolate_ptr()
    }
  }

  impl<C> GetIsolate for CallbackScope<'_, C> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.isolate.as_ptr()
    }
  }

  impl<C> GetIsolate for EscapableHandleScope<'_, '_, C> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.isolate.as_ptr()
    }
  }

  impl<P: GetIsolate> GetIsolate for AllowJavascriptExecutionScope<'_, '_, P> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.scope.get_isolate_ptr()
    }
  }

  impl<P: GetIsolate> GetIsolate for DisallowJavascriptExecutionScope<'_, '_, P> {
    fn get_isolate_ptr(&self) -> *mut RealIsolate {
      self.scope.get_isolate_ptr()
    }
  }
}

pub trait NewHandleScope<'s> {
  type NewScope: Scope;

  fn make_new_scope(me: &'s mut Self) -> Self::NewScope;
}

impl<'s, 'p: 's, C> NewHandleScope<'s> for PinnedRef<'_, HandleScope<'p, C>> {
  type NewScope = HandleScope<'s, C>;

  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    HandleScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: me.0.isolate,
      annex: me.0.annex,
      context: Cell::new(me.0.context.get()),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'s> NewHandleScope<'s> for Isolate {
  type NewScope = HandleScope<'s, ()>;

  #[inline(always)]
  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    HandleScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: unsafe { NonNull::new_unchecked(me.as_real_ptr()) },
      annex: me.get_annex_ptr(),
      context: Cell::new(None),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'s> NewHandleScope<'s> for OwnedIsolate {
  type NewScope = HandleScope<'s, ()>;

  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    let isolate: &Isolate = me;
    HandleScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: unsafe { NonNull::new_unchecked(me.get_isolate_ptr()) },
      annex: isolate.get_annex_ptr(),
      context: Cell::new(None),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'s, 'p: 's, 'i, C> NewHandleScope<'s>
  for PinnedRef<'p, CallbackScope<'i, C>>
{
  type NewScope = HandleScope<'i, C>;

  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    HandleScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: me.0.isolate,
      annex: me.0.annex,
      context: Cell::new(me.0.context.get()),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'a, 'i> NewHandleScope<'a> for ContextScope<'_, '_, HandleScope<'i>> {
  type NewScope = HandleScope<'i>;
  fn make_new_scope(me: &'a mut Self) -> Self::NewScope {
    HandleScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: unsafe { NonNull::new_unchecked(me.scope.get_isolate_ptr()) },
      annex: me.scope.0.annex,
      context: Cell::new(Some(me.raw_handle_scope.entered_context)),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

pub(crate) struct ScopeData {
  isolate: NonNull<RealIsolate>,
  context: Cell<Option<NonNull<Context>>>,
}

impl ScopeData {
  #[inline(always)]
  pub(crate) fn get_isolate_ptr(&self) -> *mut RealIsolate {
    self.isolate.as_ptr()
  }

  pub(crate) fn get_current_context(&self) -> *mut Context {
    if let Some(context) = self.context.get() {
      context.as_ptr()
    } else {
      let isolate = self.get_isolate_ptr();
      let context =
        unsafe { raw::v8__Isolate__GetCurrentContext(isolate) }.cast_mut();
      self
        .context
        .set(Some(unsafe { NonNull::new_unchecked(context) }));
      context
    }
  }
}

impl<'s> HandleScope<'s> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new<P: NewHandleScope<'s>>(
    scope: &'s mut P,
  ) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(scope))
  }
}

impl<'s> PinnedRef<'s, HandleScope<'_>> {
  /// Returns the context of the currently running JavaScript, or the context
  /// on the top of the stack if no JavaScript is running
  pub fn get_current_context(&self) -> Local<'s, Context> {
    if let Some(context) = self.0.context.get() {
      unsafe { Local::from_non_null(context) }
    } else {
      let isolate = self.0.isolate;
      let context =
        unsafe { raw::v8__Isolate__GetCurrentContext(isolate.as_ptr()) }
          .cast_mut();
      unsafe {
        self.0.context.set(Some(NonNull::new_unchecked(context)));
        Local::from_raw_unchecked(context)
      }
    }
  }

  /// Returns either the last context entered through V8's C++ API, or the
  /// context of the currently running microtask while processing microtasks.
  /// If a context is entered while executing a microtask, that context is
  /// returned.
  pub fn get_entered_or_microtask_context(&self) -> Local<'_, Context> {
    let context_ptr = unsafe {
      raw::v8__Isolate__GetEnteredOrMicrotaskContext(self.0.isolate.as_ptr())
    };
    unsafe { Local::from_raw_unchecked(context_ptr) }
  }

  /// Return data that was previously attached to the isolate snapshot via
  /// SnapshotCreator, and removes the reference to it. If called again with
  /// same `index` argument, this function returns `DataError::NoData`.
  ///
  /// The value that was stored in the snapshot must either match or be
  /// convertible to type parameter `T`, otherwise `DataError::BadType` is
  /// returned.
  pub fn get_isolate_data_from_snapshot_once<T>(
    &self,
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
    &self,
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
    &self,
    init_hook: Option<Local<Function>>,
    before_hook: Option<Local<Function>>,
    after_hook: Option<Local<Function>>,
    resolve_hook: Option<Local<Function>>,
  ) {
    unsafe {
      let context = self.get_current_context();
      raw::v8__Context__SetPromiseHooks(
        context.as_non_null().as_ptr(),
        init_hook.map_or_else(std::ptr::null, |v| &*v),
        before_hook.map_or_else(std::ptr::null, |v| &*v),
        after_hook.map_or_else(std::ptr::null, |v| &*v),
        resolve_hook.map_or_else(std::ptr::null, |v| &*v),
      );
    }
  }

  #[inline(always)]
  pub fn set_continuation_preserved_embedder_data(&self, data: Local<Value>) {
    unsafe {
      let isolate = self.0.isolate;
      raw::v8__Context__SetContinuationPreservedEmbedderData(
        isolate.as_ptr(),
        &*data,
      );
    }
  }

  #[inline(always)]
  pub fn get_continuation_preserved_embedder_data(&self) -> Local<'s, Value> {
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

  /// Returns the host defined options set for currently running script or
  /// module, if available.
  #[inline(always)]
  pub fn get_current_host_defined_options(&self) -> Option<Local<'s, Data>> {
    let isolate_ptr = self.0.isolate.as_ptr();
    unsafe {
      Local::from_raw(raw::v8__Isolate__GetCurrentHostDefinedOptions(
        isolate_ptr,
      ))
    }
  }
}

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

impl<'p> PinnedRef<'p, HandleScope<'_, ()>> {
  #[inline(always)]
  pub(crate) unsafe fn cast_local<T>(
    &self,
    _f: impl FnOnce(&mut ScopeData) -> *const T,
  ) -> Option<Local<'p, T>> {
    let mut data: ScopeData = ScopeData {
      context: Cell::new(self.0.context.get()),
      isolate: self.0.isolate,
    };
    let ptr = _f(&mut data);
    unsafe { Local::from_raw(ptr) }
  }

  /// Schedules an exception to be thrown when returning to JavaScript. When
  /// an exception has been scheduled it is illegal to invoke any
  /// JavaScript operation; the caller must return immediately and only
  /// after the exception has been handled does it become legal to invoke
  /// JavaScript operations.
  ///
  /// This function always returns the `undefined` value.
  pub fn throw_exception(
    &self,
    exception: Local<'p, Value>,
  ) -> Local<'p, Value> {
    unsafe {
      self.cast_local(|sd| {
        raw::v8__Isolate__ThrowException(sd.get_isolate_ptr(), &*exception)
      })
    }
    .unwrap()
  }

  /// Open a handle passed from V8 in the current scope.
  ///
  /// # Safety
  ///
  /// The handle must be rooted in this scope.
  #[inline(always)]
  pub unsafe fn unseal<'a, T>(&self, v: SealedLocal<T>) -> Local<'a, T> {
    unsafe { Local::from_non_null(v.0) }
  }
}

impl<C> HandleScope<'_, C> {}

impl<C> GetIsolate for Pin<&mut HandleScope<'_, C>> {
  fn get_isolate_ptr(&self) -> *mut RealIsolate {
    self.isolate.as_ptr()
  }
}

/// Stack-allocated class which sets the execution context for all operations
/// executed within a local scope. After entering a context, all code compiled
/// and run is compiled and run in this context.
#[repr(C)]
pub struct ContextScope<'borrow, 'scope, P: ClearCachedContext> {
  raw_handle_scope: raw::ContextScope,
  scope: &'borrow mut PinnedRef<'scope, P>,
}

impl<P: ClearCachedContext> ScopeInit for ContextScope<'_, '_, P> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    storage.projected()
  }
  unsafe fn deinit(_me: &mut Self) {}
}

impl<'p, P: ClearCachedContext> Deref for ContextScope<'_, 'p, P> {
  type Target = PinnedRef<'p, P>;
  fn deref(&self) -> &Self::Target {
    self.scope
  }
}

impl<P: ClearCachedContext> DerefMut for ContextScope<'_, '_, P> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.scope
  }
}

impl<P: ClearCachedContext> sealed::Sealed for ContextScope<'_, '_, P> {}
impl<P: ClearCachedContext> Scope for ContextScope<'_, '_, P> {}

mod new_context_scope {

  use super::{GetIsolate, Scope};
  use crate::{Context, Local};

  pub trait NewContextScope<'s, 'c>: GetIsolate {
    type NewScope: Scope;

    fn make_new_scope(
      me: &'s mut Self,
      context: Local<'c, Context>,
    ) -> Self::NewScope;
  }
}
use new_context_scope::NewContextScope;

mod clear_cached_context {
  pub trait ClearCachedContext {
    fn clear_cached_context(&self);
  }
}
use clear_cached_context::ClearCachedContext;

impl<C> ClearCachedContext for HandleScope<'_, C> {
  fn clear_cached_context(&self) {
    self.context.set(None);
  }
}

impl<C> ClearCachedContext for CallbackScope<'_, C> {
  fn clear_cached_context(&self) {}
}

impl<P: ClearCachedContext> ClearCachedContext for PinnedRef<'_, P> {
  fn clear_cached_context(&self) {
    self.0.clear_cached_context();
  }
}

impl<C> ClearCachedContext for EscapableHandleScope<'_, '_, C> {
  fn clear_cached_context(&self) {
    self.context.set(None);
  }
}

impl<P> Drop for ContextScope<'_, '_, P>
where
  P: ClearCachedContext,
{
  fn drop(&mut self) {
    self.scope.0.clear_cached_context();
  }
}

impl<'scope, 'obj: 'scope, 'ct, P: Scope + GetIsolate>
  NewContextScope<'scope, 'ct> for ContextScope<'_, 'obj, P>
where
  P: ClearCachedContext,
{
  type NewScope = ContextScope<'scope, 'obj, P>;

  fn make_new_scope(
    me: &'scope mut Self,
    context: Local<'ct, Context>,
  ) -> Self::NewScope {
    ContextScope {
      raw_handle_scope: raw::ContextScope::new(context),
      scope: me.scope,
    }
  }
}

impl<'scope, 'obj: 'scope, 'ct, 'i, C> NewContextScope<'scope, 'ct>
  for PinnedRef<'obj, HandleScope<'i, C>>
where
  'ct: 'scope,
{
  type NewScope = ContextScope<'scope, 'obj, HandleScope<'i>>;

  fn make_new_scope(
    me: &'scope mut Self,
    context: Local<'ct, Context>,
  ) -> Self::NewScope {
    me.0.context.set(None);
    ContextScope {
      raw_handle_scope: raw::ContextScope::new(context),
      scope: unsafe {
        // SAFETY: we are adding the context, so we can mark that it now has a context.
        // the types are the same aside from the type parameter, which is only used in a ZST
        cast_pinned_ref_mut::<HandleScope<'i, C>, HandleScope<'i, Context>>(me)
      },
    }
  }
}

impl<'scope, 'obj: 'scope, 'i, 'ct, C> NewContextScope<'scope, 'ct>
  for PinnedRef<'obj, CallbackScope<'i, C>>
{
  type NewScope = ContextScope<'scope, 'obj, HandleScope<'i>>;

  fn make_new_scope(
    me: &'scope mut Self,
    context: Local<'ct, Context>,
  ) -> Self::NewScope {
    ContextScope {
      raw_handle_scope: raw::ContextScope::new(context),
      scope: unsafe {
        // we are adding the context, so we can mark that it now has a context.
        // SAFETY: CallbackScope is a superset of HandleScope, so giving a "view" of
        // the CallbackScope as a HandleScope is valid, and we won't ever move out of the transmuted
        // value
        cast_pinned_ref_mut::<CallbackScope<'i, C>, HandleScope<'i, Context>>(
          me,
        )
      },
    }
  }
}

// these lifetimes are crazy. basically we have
// - 'borrow: the borrow of the scope
// - 'scope: the lifetime of the scope. this must be longer than 'borrow. this is the lifetime of the handles created from the scope.
// - 'i: the lifetime of the inner the `EscapableHandleScope` is made from
// - 'esc: the lifetime of the slot that the `EscapableHandleScope` will eventually escape to. this must be longer than 'i
// - 'ct: the lifetime of the context (this is _not_ the same as 'scope, it can be longer or shorter)
impl<'borrow, 'scope: 'borrow, 'i, 'esc: 'i, 'ct, C>
  NewContextScope<'borrow, 'ct>
  for PinnedRef<'scope, EscapableHandleScope<'i, 'esc, C>>
{
  type NewScope = ContextScope<'borrow, 'scope, EscapableHandleScope<'i, 'esc>>;

  fn make_new_scope(
    me: &'borrow mut Self,
    context: Local<'ct, Context>,
  ) -> Self::NewScope {
    ContextScope {
      raw_handle_scope: raw::ContextScope::new(context),
      scope: unsafe {
        // SAFETY: layouts are the same aside from the type parameter, which is only used in a ZST
        std::mem::transmute::<
          &'borrow mut PinnedRef<'scope, EscapableHandleScope<'i, 'esc, C>>,
          &'borrow mut PinnedRef<
            'scope,
            EscapableHandleScope<'i, 'esc, Context>,
          >,
        >(me)
      },
    }
  }
}

impl<P: ClearCachedContext> ClearCachedContext for ContextScope<'_, '_, P> {
  fn clear_cached_context(&self) {
    self.scope.0.clear_cached_context();
  }
}

impl<
  'scope,
  'obj: 'scope,
  'ct,
  P: NewContextScope<'scope, 'ct> + ClearCachedContext,
> ContextScope<'scope, 'obj, P>
{
  #[allow(clippy::new_ret_no_self)]
  pub fn new(
    param: &'scope mut P,
    context: Local<'ct, Context>,
  ) -> P::NewScope {
    if param.get_isolate_ptr() != unsafe { raw::v8__Isolate__GetCurrent() } {
      panic!(
        "{} and Context do not belong to the same Isolate",
        type_name::<P>()
      )
    }
    param.clear_cached_context();
    P::make_new_scope(param, context)
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
#[repr(C)]
#[derive(Debug)]
pub struct CallbackScope<'s, C = Context> {
  raw_handle_scope: raw::HandleScope,
  isolate: NonNull<RealIsolate>,
  annex: NonNull<IsolateAnnex>,
  context: Cell<Option<NonNull<Context>>>,
  _phantom: PhantomData<&'s C>,
  _pinned: PhantomPinned,
  needs_scope: bool,
}

assert_layout_subset!(HandleScope<'static, ()>, CallbackScope<'static, ()> { raw_handle_scope, isolate, annex, context, _phantom, _pinned });

impl<'s> CallbackScope<'s> {
  #[allow(clippy::new_ret_no_self)]
  pub unsafe fn new<P: NewCallbackScope<'s>>(
    param: P,
  ) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(param))
  }
}

impl<C> AsRef<Isolate> for CallbackScope<'_, C> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.isolate) }
  }
}

impl<C> ScopeInit for CallbackScope<'_, C> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    let isolate = storage_mut.scope.isolate;
    scope_owner::enter(isolate);
    if storage_mut.scope.needs_scope {
      unsafe {
        raw::HandleScope::init(
          &mut storage_mut.scope.raw_handle_scope,
          isolate,
        );
      }
    }

    let projected = &mut storage_mut.scope;
    unsafe { Pin::new_unchecked(projected) }
  }

  unsafe fn deinit(me: &mut Self) {
    if me.needs_scope {
      unsafe { raw::v8__HandleScope__DESTRUCT(&mut me.raw_handle_scope) };
    }
    scope_owner::exit();
  }
}

impl<C> sealed::Sealed for CallbackScope<'_, C> {}
impl<C> Scope for CallbackScope<'_, C> {}

pub trait NewCallbackScope<'s>: Sized + GetIsolate {
  type NewScope: Scope;
  const NEEDS_SCOPE: bool = false;

  #[inline]
  fn get_context(&self) -> Option<Local<'s, Context>> {
    None
  }

  fn make_new_scope(me: Self) -> Self::NewScope;
}

fn make_new_callback_scope<'a, C>(
  isolate: impl GetIsolate,
  context: Option<NonNull<Context>>,
) -> CallbackScope<'a, C> {
  let isolate_ptr =
    unsafe { NonNull::new_unchecked(isolate.get_isolate_ptr()) };
  let isolate_ref = unsafe { Isolate::from_raw_ref(&isolate_ptr) };
  CallbackScope {
    raw_handle_scope: unsafe { raw::HandleScope::uninit() },
    isolate: isolate_ptr,
    annex: isolate_ref.get_annex_ptr(),
    context: Cell::new(context),
    _phantom: PhantomData,
    _pinned: PhantomPinned,
    needs_scope: false,
  }
}

impl<'s> NewCallbackScope<'s> for &'s mut Isolate {
  type NewScope = CallbackScope<'s, ()>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(&*me, None)
  }
}

impl<'s> NewCallbackScope<'s> for &'s mut OwnedIsolate {
  type NewScope = CallbackScope<'s, ()>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(&*me, None)
  }
}

impl<'s> NewCallbackScope<'s> for &'s FunctionCallbackInfo {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s> NewCallbackScope<'s> for &'s FunctionCallbackInfoParts<'s> {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s, T> NewCallbackScope<'s> for &'s PropertyCallbackInfo<T> {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s> NewCallbackScope<'s> for &'s FastApiCallbackOptions<'s> {
  type NewScope = CallbackScope<'s>;
  const NEEDS_SCOPE: bool = true;

  fn make_new_scope(me: Self) -> Self::NewScope {
    let isolate_ptr =
      unsafe { NonNull::new_unchecked((*me).get_isolate_ptr()) };
    let isolate_ref = unsafe { Isolate::from_raw_ref(&isolate_ptr) };
    CallbackScope {
      raw_handle_scope: unsafe { raw::HandleScope::uninit() },
      isolate: isolate_ptr,
      annex: isolate_ref.get_annex_ptr(),
      context: Cell::new(me.get_context().map(|c| c.as_non_null())),
      _phantom: PhantomData,
      _pinned: PhantomPinned,
      needs_scope: Self::NEEDS_SCOPE,
    }
  }
}

impl<'s> NewCallbackScope<'s> for Local<'s, Context> {
  type NewScope = CallbackScope<'s>;

  #[inline]
  fn get_context(&self) -> Option<Local<'s, Context>> {
    Some(*self)
  }

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, Some(me.as_non_null()))
  }
}

impl<'s> NewCallbackScope<'s> for Local<'s, Message> {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s, T: Into<Local<'s, Object>> + GetIsolate> NewCallbackScope<'s> for T {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s> NewCallbackScope<'s> for &'s PromiseRejectMessage<'s> {
  type NewScope = CallbackScope<'s>;

  fn make_new_scope(me: Self) -> Self::NewScope {
    make_new_callback_scope(me, None)
  }
}

impl<'s> AsRef<Pin<&'s mut HandleScope<'s, ()>>> for CallbackScope<'s, ()> {
  fn as_ref(&self) -> &Pin<&'s mut HandleScope<'s, ()>> {
    unsafe { std::mem::transmute(self) }
  }
}

/// An external exception handler.
#[repr(C)]
pub struct TryCatch<'scope, 'obj, P> {
  raw_try_catch: raw::TryCatch,
  scope: &'scope mut PinnedRef<'obj, P>,
  _pinned: PhantomPinned,
}

impl<'scope, P: NewTryCatch<'scope>> TryCatch<'scope, '_, P> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'scope mut P) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(param))
  }
}

impl<P: GetIsolate> ScopeInit for TryCatch<'_, '_, P> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    let isolate = unsafe {
      NonNull::new_unchecked(storage_mut.scope.scope.get_isolate_ptr())
    };
    unsafe {
      raw::TryCatch::init(&mut storage_mut.scope.raw_try_catch, isolate);
    }
    let projected = &mut storage_mut.scope;
    unsafe { Pin::new_unchecked(projected) }
  }

  unsafe fn deinit(me: &mut Self) {
    unsafe { raw::v8__TryCatch__DESTRUCT(&mut me.raw_try_catch) };
  }
}

impl<'scope, 'obj: 'scope, 'iso: 'obj, P: GetIsolate>
  PinnedRef<'_, TryCatch<'scope, 'obj, P>>
where
  PinnedRef<'obj, P>: AsRef<PinnedRef<'obj, HandleScope<'iso>>>,
{
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
    &self.0.raw_try_catch
  }

  #[inline(always)]
  unsafe fn get_raw_mut(&mut self) -> &mut raw::TryCatch {
    unsafe { &mut self.0.as_mut().get_unchecked_mut().raw_try_catch }
  }

  pub fn exception(&self) -> Option<Local<'obj, Value>> {
    unsafe {
      self
        .0
        .scope
        .as_ref()
        .cast_local(|_data| raw::v8__TryCatch__Exception(self.get_raw()))
    }
  }

  pub fn message(&self) -> Option<Local<'obj, Message>> {
    unsafe {
      self
        .0
        .scope
        .as_ref()
        .cast_local(|_data| raw::v8__TryCatch__Message(self.get_raw()))
    }
  }

  pub fn rethrow(&mut self) -> Option<Local<'obj, Value>> {
    let raw_mut = unsafe { self.get_raw_mut() as *mut raw::TryCatch };
    unsafe {
      self
        .0
        .scope
        .as_ref()
        .cast_local(|_data| raw::v8__TryCatch__ReThrow(raw_mut))
    }
  }

  pub fn stack_trace(&self) -> Option<Local<'obj, Value>> {
    unsafe {
      self.0.scope.as_ref().cast_local(|_data| {
        raw::v8__TryCatch__StackTrace(
          self.get_raw(),
          _data.get_current_context(),
        )
      })
    }
  }
}

impl<P> sealed::Sealed for TryCatch<'_, '_, P> {}
impl<P: Scope + GetIsolate> Scope for TryCatch<'_, '_, P> {}

pub trait NewTryCatch<'scope>: GetIsolate {
  type NewScope: Scope;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope;
}

impl<'scope, 'obj: 'scope, 'i, C> NewTryCatch<'scope>
  for PinnedRef<'obj, HandleScope<'i, C>>
{
  type NewScope = TryCatch<'scope, 'obj, HandleScope<'i, C>>;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope {
    TryCatch {
      scope: me,
      raw_try_catch: unsafe { raw::TryCatch::uninit() },
      _pinned: PhantomPinned,
    }
  }
}

// the lifetimes here are:
// - 'borrow: the lifetime of the borrow of the scope
// - 'scope: the lifetime of the escapable handle scope
// - 'obj: the lifetime of the handles created from the escapable handle scope
// - 'esc: the lifetime of the slot that the escapable handle scope will eventually escape to. this must be longer than 'obj
impl<'borrow, 'scope: 'borrow, 'obj: 'borrow, 'esc: 'obj, C>
  NewTryCatch<'borrow>
  for PinnedRef<'scope, EscapableHandleScope<'obj, 'esc, C>>
{
  type NewScope =
    TryCatch<'borrow, 'scope, EscapableHandleScope<'obj, 'esc, C>>;

  fn make_new_scope(me: &'borrow mut Self) -> Self::NewScope {
    TryCatch {
      scope: me,
      raw_try_catch: unsafe { raw::TryCatch::uninit() },
      _pinned: PhantomPinned,
    }
  }
}

impl<'scope, 'obj: 'scope, T: GetIsolate + Scope + ClearCachedContext>
  NewTryCatch<'scope> for ContextScope<'_, 'obj, T>
{
  type NewScope = TryCatch<'scope, 'obj, T>;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope {
    TryCatch {
      scope: me,
      raw_try_catch: unsafe { raw::TryCatch::uninit() },
      _pinned: PhantomPinned,
    }
  }
}

impl<'scope, 'obj: 'scope, 'i, C> NewTryCatch<'scope>
  for PinnedRef<'obj, CallbackScope<'i, C>>
{
  type NewScope = TryCatch<'scope, 'i, HandleScope<'i, C>>;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope {
    TryCatch {
      scope: unsafe {
        std::mem::transmute::<
          &mut PinnedRef<'_, CallbackScope<'_, C>>,
          &mut PinnedRef<'_, HandleScope<'_, C>>,
        >(me)
      },
      raw_try_catch: unsafe { raw::TryCatch::uninit() },
      _pinned: PhantomPinned,
    }
  }
}

impl<'scope, 'obj: 'scope, 'obj_outer: 'obj, 'iso, C> NewTryCatch<'scope>
  for PinnedRef<'obj, TryCatch<'_, 'obj_outer, HandleScope<'iso, C>>>
{
  type NewScope = TryCatch<'scope, 'obj_outer, HandleScope<'iso, C>>;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope {
    TryCatch {
      scope: unsafe { me.as_mut_ref().0.get_unchecked_mut().scope },
      raw_try_catch: unsafe { raw::TryCatch::uninit() },
      _pinned: PhantomPinned,
    }
  }
}

/// A HandleScope which first allocates a handle in the current scope
/// which will be later filled with the escape value.
#[repr(C)]
pub struct EscapableHandleScope<'s, 'esc: 's, C = Context> {
  raw_handle_scope: raw::HandleScope,
  isolate: NonNull<RealIsolate>,
  annex: NonNull<IsolateAnnex>,
  context: Cell<Option<NonNull<Context>>>,
  _phantom:
    PhantomData<(&'s mut raw::HandleScope, &'esc mut raw::EscapeSlot, &'s C)>,
  _pinned: PhantomPinned,
  raw_escape_slot: Option<raw::EscapeSlot>,
}

assert_layout_subset!(HandleScope<'static, ()>, EscapableHandleScope<'static, 'static, ()> {
  raw_handle_scope,
  isolate,
  annex,
  context,
  _phantom,
  _pinned,
});

impl<'s, 'esc: 's, C> ScopeInit for EscapableHandleScope<'s, 'esc, C> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    scope_owner::enter(storage_mut.scope.isolate);
    unsafe {
      let isolate = storage_mut.scope.isolate;
      raw::HandleScope::init(&mut storage_mut.scope.raw_handle_scope, isolate);
    }
    let projected = &mut storage_mut.scope;

    unsafe { Pin::new_unchecked(projected) }
  }

  unsafe fn deinit(me: &mut Self) {
    unsafe { raw::v8__HandleScope__DESTRUCT(&raw mut me.raw_handle_scope) };
    scope_owner::exit();
  }
}

impl<'s, 'esc: 's> EscapableHandleScope<'s, 'esc> {
  #[allow(clippy::new_ret_no_self)]
  pub fn new<P: NewEscapableHandleScope<'s>>(
    scope: &'s mut P,
  ) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(scope))
  }
}

impl<'s, 'esc: 's, C> PinnedRef<'_, EscapableHandleScope<'s, 'esc, C>> {
  /// Pushes the value into the previous scope and returns a handle to it.
  /// Cannot be called twice.
  pub fn escape<'a, T>(&mut self, value: Local<'a, T>) -> Local<'esc, T>
  where
    for<'l> Local<'l, T>: Into<Local<'l, crate::Data>>,
  {
    let escape_slot = unsafe { self.0.as_mut().get_unchecked_mut() }
      .raw_escape_slot
      .take()
      .expect("EscapableHandleScope::escape() called twice");
    escape_slot.escape(value)
  }
}

impl<'p, 's, 'esc: 's, C> Deref
  for PinnedRef<'p, EscapableHandleScope<'s, 'esc, C>>
{
  type Target = PinnedRef<'p, HandleScope<'s, C>>;
  fn deref(&self) -> &Self::Target {
    unsafe { std::mem::transmute(self) }
  }
}

impl<'s, 'esc: 's, C> DerefMut
  for PinnedRef<'_, EscapableHandleScope<'s, 'esc, C>>
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { std::mem::transmute(self) }
  }
}

pub trait NewEscapableHandleScope<'s> {
  type NewScope: Scope;
  fn make_new_scope(me: &'s mut Self) -> Self::NewScope;
}

impl<'s> NewEscapableHandleScope<'s> for Isolate {
  type NewScope = EscapableHandleScope<'s, 's, ()>;

  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    let isolate = unsafe { NonNull::new_unchecked(me.as_real_ptr()) };
    let annex = me.get_annex_ptr();
    let raw_escape_slot = raw::EscapeSlot::new(isolate);
    let raw_handle_scope = unsafe { raw::HandleScope::uninit() };

    EscapableHandleScope {
      isolate,
      annex,
      context: Cell::new(None),
      raw_escape_slot: Some(raw_escape_slot),
      raw_handle_scope,
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'s, 'obj: 's, C> NewEscapableHandleScope<'s>
  for PinnedRef<'obj, HandleScope<'_, C>>
{
  type NewScope = EscapableHandleScope<'s, 'obj, C>;
  fn make_new_scope(me: &'s mut Self) -> Self::NewScope {
    // Note: the `raw_escape_slot` field must be initialized _before_ the
    // `raw_handle_scope` field, otherwise the escaped local handle ends up
    // inside the `EscapableHandleScope` that's being constructed here,
    // rather than escaping from it.
    let isolate = me.0.isolate;
    let annex = me.0.annex;
    let raw_escape_slot = raw::EscapeSlot::new(isolate);
    let raw_handle_scope = unsafe { raw::HandleScope::uninit() };

    EscapableHandleScope {
      isolate,
      annex,
      context: Cell::new(me.0.context.get()),
      raw_escape_slot: Some(raw_escape_slot),
      raw_handle_scope,
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'borrow, 'obj: 'borrow, C> NewEscapableHandleScope<'borrow>
  for ContextScope<'_, 'obj, HandleScope<'_, C>>
{
  type NewScope = EscapableHandleScope<'borrow, 'obj, C>;
  fn make_new_scope(me: &'borrow mut Self) -> Self::NewScope {
    NewEscapableHandleScope::make_new_scope(me.scope)
  }
}

impl<'borrow, 's: 'borrow, 'esc: 'borrow, C> NewEscapableHandleScope<'borrow>
  for PinnedRef<'_, EscapableHandleScope<'s, 'esc, C>>
{
  type NewScope = EscapableHandleScope<'borrow, 's, C>;
  fn make_new_scope(me: &'borrow mut Self) -> Self::NewScope {
    // Note: the `raw_escape_slot` field must be initialized _before_ the
    // `raw_handle_scope` field, otherwise the escaped local handle ends up
    // inside the `EscapableHandleScope` that's being constructed here,
    // rather than escaping from it.
    let isolate = me.0.isolate;
    let annex = me.0.annex;
    let raw_escape_slot = raw::EscapeSlot::new(isolate);
    let raw_handle_scope = unsafe { raw::HandleScope::uninit() };
    EscapableHandleScope {
      isolate,
      annex,
      context: Cell::new(me.0.context.get()),
      raw_escape_slot: Some(raw_escape_slot),
      raw_handle_scope,
      _phantom: PhantomData,
      _pinned: PhantomPinned,
    }
  }
}

impl<'s, 'esc: 's, C> sealed::Sealed for EscapableHandleScope<'s, 'esc, C> {}
impl<'s, 'esc: 's, C> Scope for EscapableHandleScope<'s, 'esc, C> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum OnFailure {
  CrashOnFailure,
  ThrowOnFailure,
  DumpOnFailure,
}

#[repr(C)]
pub struct DisallowJavascriptExecutionScope<'scope, 'obj, P> {
  raw: raw::DisallowJavascriptExecutionScope,
  scope: &'scope mut PinnedRef<'obj, P>,
  on_failure: OnFailure,
  _pinned: PhantomPinned,
}

impl<P: GetIsolate> ScopeInit for DisallowJavascriptExecutionScope<'_, '_, P> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    // SAFETY: we aren't going to move the raw scope out
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    let isolate = storage_mut.scope.scope.get_isolate_ptr();
    let on_failure = storage_mut.scope.on_failure;
    // SAFETY: calling the raw function, isolate is valid, and the raw scope won't be moved
    unsafe {
      raw::DisallowJavascriptExecutionScope::init(
        &mut storage_mut.scope.raw,
        NonNull::new_unchecked(isolate),
        on_failure,
      );
      Pin::new_unchecked(&mut storage_mut.scope)
    }
  }

  unsafe fn deinit(me: &mut Self) {
    unsafe { raw::v8__DisallowJavascriptExecutionScope__DESTRUCT(&mut me.raw) };
  }
}

impl<P: GetIsolate> sealed::Sealed
  for DisallowJavascriptExecutionScope<'_, '_, P>
{
}
impl<P: Scope + GetIsolate> Scope
  for DisallowJavascriptExecutionScope<'_, '_, P>
{
}

impl<'scope, P: NewDisallowJavascriptExecutionScope<'scope>>
  DisallowJavascriptExecutionScope<'scope, '_, P>
{
  #[allow(clippy::new_ret_no_self)]
  pub fn new(
    param: &'scope mut P,
    on_failure: OnFailure,
  ) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(param, on_failure))
  }
}

pub trait NewDisallowJavascriptExecutionScope<'scope> {
  type NewScope: Scope;
  fn make_new_scope(
    me: &'scope mut Self,
    on_failure: OnFailure,
  ) -> Self::NewScope;
}

impl<'scope, 'obj, P> NewDisallowJavascriptExecutionScope<'scope>
  for ContextScope<'_, 'obj, P>
where
  P: ClearCachedContext,
  PinnedRef<'obj, P>: NewDisallowJavascriptExecutionScope<'scope>,
{
  type NewScope = <PinnedRef<'obj, P> as NewDisallowJavascriptExecutionScope<
    'scope,
  >>::NewScope;
  fn make_new_scope(
    me: &'scope mut Self,
    on_failure: OnFailure,
  ) -> Self::NewScope {
    PinnedRef::<'obj, P>::make_new_scope(me.scope, on_failure)
  }
}

impl<'scope, 'obj: 'scope, P: Scope + GetIsolate>
  NewDisallowJavascriptExecutionScope<'scope> for PinnedRef<'obj, P>
{
  type NewScope = DisallowJavascriptExecutionScope<'scope, 'obj, P>;

  fn make_new_scope(
    me: &'scope mut Self,
    on_failure: OnFailure,
  ) -> Self::NewScope {
    DisallowJavascriptExecutionScope {
      raw: unsafe { raw::DisallowJavascriptExecutionScope::uninit() },
      scope: me,
      on_failure,
      _pinned: PhantomPinned,
    }
  }
}

#[repr(C)]
pub struct AllowJavascriptExecutionScope<'scope, 'obj, P> {
  raw: raw::AllowJavascriptExecutionScope,
  scope: &'scope mut PinnedRef<'obj, P>,
  _pinned: PhantomPinned,
}

impl<P: GetIsolate> ScopeInit for AllowJavascriptExecutionScope<'_, '_, P> {
  fn init_stack(storage: Pin<&mut ScopeStorage<Self>>) -> Pin<&mut Self> {
    let storage_mut = unsafe { storage.get_unchecked_mut() };
    let isolate = unsafe {
      NonNull::new_unchecked(storage_mut.scope.scope.get_isolate_ptr())
    };
    unsafe {
      raw::AllowJavascriptExecutionScope::init(
        &mut storage_mut.scope.raw,
        isolate,
      );
    }
    let projected = &mut storage_mut.scope;
    unsafe { Pin::new_unchecked(projected) }
  }

  unsafe fn deinit(me: &mut Self) {
    unsafe { raw::v8__AllowJavascriptExecutionScope__DESTRUCT(&mut me.raw) };
  }
}

impl<P: GetIsolate> sealed::Sealed
  for AllowJavascriptExecutionScope<'_, '_, P>
{
}
impl<P: Scope + GetIsolate> Scope for AllowJavascriptExecutionScope<'_, '_, P> {}

impl<'scope, P: NewAllowJavascriptExecutionScope<'scope>>
  AllowJavascriptExecutionScope<'scope, '_, P>
{
  #[allow(clippy::new_ret_no_self)]
  pub fn new(param: &'scope mut P) -> ScopeStorage<P::NewScope> {
    ScopeStorage::new(P::make_new_scope(param))
  }
}

pub trait NewAllowJavascriptExecutionScope<'scope> {
  type NewScope: Scope;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope;
}

impl<'scope, 'obj: 'scope, P: Scope + GetIsolate>
  NewAllowJavascriptExecutionScope<'scope> for PinnedRef<'obj, P>
{
  type NewScope = AllowJavascriptExecutionScope<'scope, 'obj, P>;
  fn make_new_scope(me: &'scope mut Self) -> Self::NewScope {
    AllowJavascriptExecutionScope {
      raw: unsafe { raw::AllowJavascriptExecutionScope::uninit() },
      scope: me,
      _pinned: PhantomPinned,
    }
  }
}

// Note: the macros below _do not_ use std::pin::pin! because
// it leads to worse compiler errors when the scope doesn't live long enough.
// Instead, we do the same thing as std::pin::pin! but without the additional temporary scope.

#[allow(unused_macros, clippy::macro_metavars_in_unsafe)]
#[macro_export]
macro_rules! callback_scope {
  (unsafe $scope: ident, $param: expr $(,)?) => {
    #[allow(clippy::macro_metavars_in_unsafe)]
    let mut $scope = {
      // force the caller to put a separate unsafe block around the param expr
      let param = $param;
      unsafe { $crate::CallbackScope::new(param) }
    };
    // SAFETY: we are shadowing the original binding, so it can't be accessed
    // ever again
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (unsafe let $scope: ident, $param: expr $(,)?) => {
    $crate::callback_scope!(unsafe $scope, $param);
  }
}

#[allow(unused_imports)]
pub(crate) use callback_scope;

/// Creates a pinned `HandleScope` and binds `&mut PinScope` to `$scope`.
///
/// ```rust
/// # let platform = v8::new_default_platform(0, false).make_shared();
/// # v8::V8::initialize_platform(platform);
/// # v8::V8::initialize();
/// # let isolate = &mut v8::Isolate::new(Default::default());
/// v8::scope!(let scope, isolate);
/// ```
#[allow(unused_macros)]
#[macro_export]
macro_rules! scope {
  ($scope: ident, $param: expr $(,)?) => {
    let mut $scope = $crate::HandleScope::new($param);
    // SAFETY: we are shadowing the original binding, so it can't be accessed
    // ever again
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (let $scope: ident, $param: expr $(,)?) => {
    $crate::scope!($scope, $param);
  };
}

#[allow(unused_imports)]
pub(crate) use scope;

#[allow(unused_macros)]
#[macro_export]
macro_rules! scope_with_context {
  ($scope: ident, $param: expr, $context: expr $(,)?) => {
    let mut $scope = $crate::HandleScope::new($param);
    // SAFETY: we are shadowing the original binding, so it can't be accessed
    // ever again
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
    let context = v8::Local::new($scope, $context);
    let $scope = &mut $crate::ContextScope::new($scope, context);
  };
  (let $scope: ident, $param: expr, $context: expr $(,)?) => {
    $crate::scope_with_context!($scope, $param, $context);
  };
}

#[allow(unused_imports)]
pub(crate) use scope_with_context;

#[allow(unused_macros)]
#[macro_export]
macro_rules! tc_scope {
  ($scope: ident, $param: expr $(,)?) => {
    let mut $scope = $crate::TryCatch::new($param);
    // SAFETY: we are shadowing the original binding, so it can't be accessed
    // ever again
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (let $scope: ident, $param: expr $(,)?) => {
    $crate::tc_scope!($scope, $param);
  };
}

#[macro_export]
macro_rules! disallow_javascript_execution_scope {
  ($scope: ident, $param: expr, $on_failure: expr $(,)?) => {
    let mut $scope =
      $crate::DisallowJavascriptExecutionScope::new($param, $on_failure);
    // SAFETY: we are shadowing the original binding, so it can't be accessed
    // ever again
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (let $scope: ident, $param: expr, $on_failure: expr $(,)?) => {
    $crate::disallow_javascript_execution_scope!($scope, $param, $on_failure);
  };
}

#[allow(unused_imports)]
pub(crate) use disallow_javascript_execution_scope;

#[macro_export]
macro_rules! allow_javascript_execution_scope {
  ($scope: ident, $param: expr $(,)?) => {
    let mut $scope = $crate::AllowJavascriptExecutionScope::new($param);
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (let $scope: ident, $param: expr $(,)?) => {
    $crate::allow_javascript_execution_scope!($scope, $param);
  };
}

#[allow(unused_imports)]
pub(crate) use allow_javascript_execution_scope;

#[macro_export]
macro_rules! escapable_handle_scope {
  ($scope: ident, $param: expr $(,)?) => {
    let mut $scope = $crate::EscapableHandleScope::new($param);
    let mut $scope = {
      let scope_pinned = unsafe { std::pin::Pin::new_unchecked(&mut $scope) };
      scope_pinned.init()
    };
    let $scope = &mut $scope;
  };
  (let $scope: ident, $param: expr $(,)?) => {
    $crate::escapable_handle_scope!($scope, $param);
  };
}

#[allow(unused_imports)]
pub(crate) use escapable_handle_scope;

#[repr(transparent)]
pub struct PinnedRef<'p, T>(Pin<&'p mut T>);

impl<'p, T> From<Pin<&'p mut T>> for PinnedRef<'p, T> {
  fn from(value: Pin<&'p mut T>) -> Self {
    PinnedRef(value)
  }
}

impl<T> PinnedRef<'_, T> {
  pub fn as_mut_ref(&mut self) -> PinnedRef<'_, T> {
    PinnedRef(self.0.as_mut())
  }
}

unsafe fn cast_pinned_ref<'b, 'o, I, O>(
  pinned: &PinnedRef<'_, I>,
) -> &'b PinnedRef<'o, O> {
  unsafe { std::mem::transmute(pinned) }
}

unsafe fn cast_pinned_ref_mut<'b, 'o, I, O>(
  pinned: &mut PinnedRef<'_, I>,
) -> &'b mut PinnedRef<'o, O> {
  unsafe { std::mem::transmute(pinned) }
}

impl<'p, 'i> Deref for PinnedRef<'p, HandleScope<'i>> {
  type Target = PinnedRef<'p, HandleScope<'i, ()>>;
  fn deref(&self) -> &Self::Target {
    unsafe { cast_pinned_ref::<HandleScope<'i>, HandleScope<'i, ()>>(self) }
  }
}

impl DerefMut for PinnedRef<'_, HandleScope<'_>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { cast_pinned_ref_mut::<HandleScope<'_>, HandleScope<'_, ()>>(self) }
  }
}

impl Deref for PinnedRef<'_, HandleScope<'_, ()>> {
  type Target = Isolate;
  #[inline(always)]
  fn deref(&self) -> &Self::Target {
    unsafe { Isolate::from_raw_ref(&self.0.isolate) }
  }
}

impl DerefMut for PinnedRef<'_, HandleScope<'_, ()>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe {
      Isolate::from_raw_ref_mut(
        &mut self.0.as_mut().get_unchecked_mut().isolate,
      )
    }
  }
}

impl<'i> Deref for PinnedRef<'_, CallbackScope<'i>> {
  // You may notice the output lifetime is a little bit weird here.
  // Basically, we're saying that any Handles created from this `CallbackScope`
  // will live as long as the thing that we made the `CallbackScope` from.
  // In practice, this means that the caller of `CallbackScope::new` needs to
  // be careful to ensure that the input lifetime is a safe approximation.
  type Target = PinnedRef<'i, HandleScope<'i>>;
  fn deref(&self) -> &Self::Target {
    unsafe { cast_pinned_ref::<CallbackScope<'i>, HandleScope<'i>>(self) }
  }
}

impl DerefMut for PinnedRef<'_, CallbackScope<'_>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { cast_pinned_ref_mut::<CallbackScope<'_>, HandleScope<'_>>(self) }
  }
}

impl<'i> Deref for PinnedRef<'_, CallbackScope<'i, ()>> {
  type Target = PinnedRef<'i, HandleScope<'i, ()>>;
  fn deref(&self) -> &Self::Target {
    unsafe {
      cast_pinned_ref::<CallbackScope<'i, ()>, HandleScope<'i, ()>>(self)
    }
  }
}

impl DerefMut for PinnedRef<'_, CallbackScope<'_, ()>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe {
      cast_pinned_ref_mut::<CallbackScope<'_, ()>, HandleScope<'_, ()>>(self)
    }
  }
}

impl<'obj, 'iso, C> Deref
  for PinnedRef<'_, TryCatch<'_, 'obj, HandleScope<'iso, C>>>
{
  type Target = PinnedRef<'obj, HandleScope<'iso, C>>;
  fn deref(&self) -> &Self::Target {
    self.0.scope
  }
}

impl<C> DerefMut for PinnedRef<'_, TryCatch<'_, '_, HandleScope<'_, C>>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    // SAFETY: we're just projecting the pinned reference, it still can't be moved
    unsafe { self.as_mut_ref().0.get_unchecked_mut().scope }
  }
}

impl<'borrow, 'scope: 'borrow, 'obj: 'borrow, 'esc: 'obj, C> Deref
  for PinnedRef<
    '_,
    TryCatch<'borrow, 'scope, EscapableHandleScope<'obj, 'esc, C>>,
  >
{
  type Target = PinnedRef<'scope, EscapableHandleScope<'obj, 'esc, C>>;

  fn deref(&self) -> &Self::Target {
    self.0.scope
  }
}

impl<'borrow, 'scope: 'borrow, 'obj: 'borrow, 'esc: 'obj, C> DerefMut
  for PinnedRef<
    '_,
    TryCatch<'borrow, 'scope, EscapableHandleScope<'obj, 'esc, C>>,
  >
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    // SAFETY: we're just projecting the pinned reference, it still can't be moved
    unsafe { self.0.as_mut().get_unchecked_mut().scope }
  }
}

impl<'obj, P> Deref
  for PinnedRef<'_, DisallowJavascriptExecutionScope<'_, 'obj, P>>
{
  type Target = PinnedRef<'obj, P>;
  fn deref(&self) -> &Self::Target {
    self.0.scope
  }
}

impl<P> DerefMut
  for PinnedRef<'_, DisallowJavascriptExecutionScope<'_, '_, P>>
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    // SAFETY: we're just projecting the pinned reference, it still can't be moved
    unsafe { self.0.as_mut().get_unchecked_mut().scope }
  }
}

impl<'obj, P> Deref
  for PinnedRef<'_, AllowJavascriptExecutionScope<'_, 'obj, P>>
{
  type Target = PinnedRef<'obj, P>;
  fn deref(&self) -> &Self::Target {
    self.0.scope
  }
}
impl<P> DerefMut for PinnedRef<'_, AllowJavascriptExecutionScope<'_, '_, P>> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    // SAFETY: we're just projecting the pinned reference, it still can't be moved
    unsafe { self.0.as_mut().get_unchecked_mut().scope }
  }
}

impl<P> GetIsolate for PinnedRef<'_, P>
where
  P: GetIsolate,
{
  fn get_isolate_ptr(&self) -> *mut RealIsolate {
    self.0.get_isolate_ptr()
  }
}

impl<C> AsRef<Isolate> for PinnedRef<'_, HandleScope<'_, C>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.isolate) }
  }
}

impl<C> AsMut<Isolate> for PinnedRef<'_, HandleScope<'_, C>> {
  fn as_mut(&mut self) -> &mut Isolate {
    unsafe {
      Isolate::from_raw_ref_mut(
        &mut self.0.as_mut().get_unchecked_mut().isolate,
      )
    }
  }
}

// Cached slot accessors that use the annex pointer stored in the scope,
// avoiding an FFI call to v8__Isolate__GetData on every access.
// These shadow the Isolate::get_slot/set_slot/etc. methods in the Deref chain.
impl<C> PinnedRef<'_, HandleScope<'_, C>> {
  /// Get a reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot<T: 'static>(&self) -> Option<&T> {
    let annex = unsafe { self.0.annex.as_ref() };
    annex.get_slot::<T>()
  }

  /// Get a mutable reference to embedder data added with `set_slot()`.
  #[inline(always)]
  pub fn get_slot_mut<T: 'static>(&mut self) -> Option<&mut T> {
    let annex = unsafe { &mut *self.0.annex.as_ptr() };
    annex.get_slot_mut::<T>()
  }

  /// Use with `get_slot` and `get_slot_mut` to associate state with an Isolate.
  ///
  /// This method gives ownership of value to the Isolate. Exactly one object of
  /// each type can be associated with an Isolate. If called more than once with
  /// an object of the same type, the earlier version will be dropped and
  /// replaced.
  ///
  /// Returns true if value was set without replacing an existing value.
  ///
  /// The value will be dropped when the isolate is dropped.
  #[inline(always)]
  pub fn set_slot<T: 'static>(&mut self, value: T) -> bool {
    let annex = unsafe { &mut *self.0.annex.as_ptr() };
    annex.set_slot(value)
  }

  /// Removes the embedder data added with `set_slot()` and returns it if it
  /// exists.
  #[inline(always)]
  pub fn remove_slot<T: 'static>(&mut self) -> Option<T> {
    let annex = unsafe { &mut *self.0.annex.as_ptr() };
    annex.remove_slot::<T>()
  }
}

impl<C> AsRef<Isolate> for PinnedRef<'_, CallbackScope<'_, C>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.isolate) }
  }
}

impl<C> AsMut<Isolate> for PinnedRef<'_, CallbackScope<'_, C>> {
  fn as_mut(&mut self) -> &mut Isolate {
    unsafe {
      Isolate::from_raw_ref_mut(
        &mut self.0.as_mut().get_unchecked_mut().isolate,
      )
    }
  }
}

impl<C> AsRef<Isolate> for PinnedRef<'_, TryCatch<'_, '_, HandleScope<'_, C>>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.scope.0.isolate) }
  }
}

impl<C> AsMut<Isolate> for PinnedRef<'_, TryCatch<'_, '_, HandleScope<'_, C>>> {
  fn as_mut(&mut self) -> &mut Isolate {
    unsafe {
      Isolate::from_raw_ref_mut(
        &mut self
          .0
          .as_mut()
          .get_unchecked_mut()
          .scope
          .0
          .as_mut()
          .get_unchecked_mut()
          .isolate,
      )
    }
  }
}

impl<C> AsRef<Isolate>
  for PinnedRef<
    '_,
    DisallowJavascriptExecutionScope<'_, '_, HandleScope<'_, C>>,
  >
{
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.scope.0.isolate) }
  }
}

impl<C> AsRef<Isolate>
  for PinnedRef<'_, AllowJavascriptExecutionScope<'_, '_, HandleScope<'_, C>>>
{
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.scope.0.isolate) }
  }
}

impl AsRef<Isolate> for PinnedRef<'_, EscapableHandleScope<'_, '_>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.0.isolate) }
  }
}

impl<C> AsRef<Isolate> for ContextScope<'_, '_, HandleScope<'_, C>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.scope.0.isolate) }
  }
}
impl<C> AsRef<Isolate> for ContextScope<'_, '_, CallbackScope<'_, C>> {
  fn as_ref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.scope.0.isolate) }
  }
}

impl<'pin, 's, 'esc: 's, C> AsRef<PinnedRef<'pin, HandleScope<'s, C>>>
  for PinnedRef<'pin, EscapableHandleScope<'s, 'esc, C>>
{
  fn as_ref(&self) -> &PinnedRef<'pin, HandleScope<'s, C>> {
    unsafe {
      cast_pinned_ref::<EscapableHandleScope<'s, 'esc, C>, HandleScope<'s, C>>(
        self,
      )
    }
  }
}

impl<'obj, 'inner, C> AsRef<PinnedRef<'obj, HandleScope<'inner, C>>>
  for PinnedRef<'obj, HandleScope<'inner, C>>
{
  fn as_ref(&self) -> &PinnedRef<'obj, HandleScope<'inner, C>> {
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::ContextOptions;
  use std::any::type_name;
  use std::pin::pin;

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
    let l1_hs = pin!(HandleScope::new(isolate));
    let l1_hs = &mut l1_hs.init();
    AssertTypeOf(l1_hs).is::<PinnedRef<HandleScope<()>>>();
    let context = Context::new(l1_hs, ContextOptions::default());
    {
      let l2_cxs = &mut ContextScope::new(l1_hs, context);
      AssertTypeOf(l2_cxs).is::<ContextScope<HandleScope>>();
      {
        let d = l2_cxs.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
      }
      {
        tc_scope!(let l3_tc, &mut **l2_cxs);
        AssertTypeOf(l3_tc).is::<PinnedRef<TryCatch<HandleScope>>>();
        let d = l3_tc.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
      }
      {
        disallow_javascript_execution_scope!(let l3_djses, l2_cxs, OnFailure::CrashOnFailure);
        AssertTypeOf(l3_djses)
          .is::<PinnedRef<DisallowJavascriptExecutionScope<HandleScope>>>();
        let d = l3_djses.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
        {
          allow_javascript_execution_scope!(let l4_ajses, l3_djses);
          AssertTypeOf(l4_ajses).is::<PinnedRef<
            AllowJavascriptExecutionScope<
              DisallowJavascriptExecutionScope<HandleScope>,
            >,
          >>();
          let d = l4_ajses.deref_mut();
          AssertTypeOf(d)
            .is::<PinnedRef<DisallowJavascriptExecutionScope<HandleScope>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
      }
      {
        escapable_handle_scope!(let l3_ehs, l2_cxs);
        AssertTypeOf(l3_ehs).is::<PinnedRef<EscapableHandleScope>>();
        {
          let l4_cxs = &mut ContextScope::new(l3_ehs, context);
          AssertTypeOf(l4_cxs).is::<ContextScope<EscapableHandleScope>>();
          let d = l4_cxs.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<EscapableHandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
        {
          tc_scope!(let l4_tc, l3_ehs);
          AssertTypeOf(l4_tc).is::<PinnedRef<TryCatch<EscapableHandleScope>>>();
          let d = l4_tc.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<EscapableHandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
        {
          disallow_javascript_execution_scope!(let l4_djses, l3_ehs, OnFailure::CrashOnFailure);
          AssertTypeOf(l4_djses)
            .is::<PinnedRef<DisallowJavascriptExecutionScope<EscapableHandleScope>>>();
          let d = l4_djses.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<EscapableHandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
          {
            allow_javascript_execution_scope!(let l5_ajses, l4_djses);
            AssertTypeOf(l5_ajses).is::<PinnedRef<
              AllowJavascriptExecutionScope<
                DisallowJavascriptExecutionScope<EscapableHandleScope>,
              >,
            >>();
            let d = l5_ajses.deref_mut();
            AssertTypeOf(d).is::<PinnedRef<DisallowJavascriptExecutionScope<EscapableHandleScope>>>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<PinnedRef<EscapableHandleScope>>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
            let d = d.deref_mut();
            AssertTypeOf(d).is::<Isolate>();
          }
        }
      }
    }
    {
      tc_scope!(let l2_tc, l1_hs);
      AssertTypeOf(l2_tc).is::<PinnedRef<TryCatch<HandleScope<()>>>>();
      let d = l2_tc.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
      {
        disallow_javascript_execution_scope!(let l3_djses, l2_tc, OnFailure::CrashOnFailure);
        AssertTypeOf(l3_djses).is::<PinnedRef<
          DisallowJavascriptExecutionScope<TryCatch<HandleScope<()>>>,
        >>();
        let d = l3_djses.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<TryCatch<HandleScope<()>>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
        let d = d.deref_mut();
        AssertTypeOf(d).is::<Isolate>();
        {
          allow_javascript_execution_scope!(let l4_ajses, l3_djses);
          AssertTypeOf(l4_ajses).is::<PinnedRef<
            AllowJavascriptExecutionScope<
              DisallowJavascriptExecutionScope<TryCatch<HandleScope<()>>>,
            >,
          >>();
          let d = l4_ajses.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<
            DisallowJavascriptExecutionScope<TryCatch<HandleScope<()>>>,
          >>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<TryCatch<HandleScope<()>>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
          let d = d.deref_mut();
          AssertTypeOf(d).is::<Isolate>();
        }
      }
    }
    {
      escapable_handle_scope!(let l2_ehs, l1_hs);
      AssertTypeOf(l2_ehs).is::<PinnedRef<EscapableHandleScope<()>>>();
      tc_scope!(let l3_tc, l2_ehs);
      AssertTypeOf(l3_tc).is::<PinnedRef<TryCatch<EscapableHandleScope<()>>>>();
      let d = l3_tc.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<EscapableHandleScope<()>>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
    {
      // `CallbackScope` is meant to be used inside V8 API callback functions
      // only. It assumes that a `HandleScope` already exists on the stack, and
      // that a context has been entered. Push a `ContextScope` onto the stack
      // to also meet the second expectation.
      let _ = ContextScope::new(l1_hs, context);
      callback_scope!(unsafe l2_cbs, context);
      AssertTypeOf(l2_cbs).is::<PinnedRef<CallbackScope>>();
      let d = l2_cbs.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<HandleScope>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
    {
      let isolate: &mut Isolate = l1_hs.as_mut();
      callback_scope!(unsafe l2_cbs, isolate);
      AssertTypeOf(l2_cbs).is::<PinnedRef<CallbackScope<()>>>();
      let d = l2_cbs.deref_mut();
      AssertTypeOf(d).is::<PinnedRef<HandleScope<()>>>();
      let d = d.deref_mut();
      AssertTypeOf(d).is::<Isolate>();
    }
  }
}
