// Copyright 2019-2026 the Deno authors. All rights reserved. MIT license.

//! Support for sharing an isolate between threads, one thread at a time,
//! via the `v8::Locker` API.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr::NonNull;
use std::sync::Arc;

use crate::Isolate;
use crate::IsolateHandle;
use crate::isolate::RealIsolate;

unsafe extern "C" {
  fn v8__Locker__CONSTRUCT(buf: *mut RawLocker, isolate: *mut RealIsolate);
  fn v8__Locker__DESTRUCT(this: *mut RawLocker);
  pub(crate) fn v8__Locker__IsLocked(isolate: *const RealIsolate) -> bool;
  fn v8__Unlocker__CONSTRUCT(buf: *mut RawUnlocker, isolate: *mut RealIsolate);
  fn v8__Unlocker__DESTRUCT(this: *mut RawUnlocker);
  fn v8__Isolate__Enter(isolate: *mut RealIsolate);
  fn v8__Isolate__Exit(isolate: *mut RealIsolate);
  fn v8__Isolate__TryGetCurrent() -> *mut RealIsolate;
}

/// Whether the current thread holds the `v8::Locker` for `isolate`.
///
/// This asks V8 rather than shadowing the lock state in a thread-local.
/// A shadow would be a little cheaper on the `Global` clone/drop/eq/hash
/// path, but it has to stay correct in two places it is hard to
/// guarantee: it must be updated across [`Locker::unlock`] windows, and
/// any thread-local holding it is destructible, so a `Locker` or shared
/// `Global` living in a thread-local that was initialized *earlier* would
/// touch it from its destructor after it had already been torn down —
/// `LocalKey::with` panics there, and a panic in a TLS destructor aborts.
/// A false positive here would let `Global::drop` reset a cell without
/// the lock, so correctness wins over the FFI call.
pub(crate) fn thread_holds_lock(isolate: *mut RealIsolate) -> bool {
  unsafe { v8__Locker__IsLocked(isolate) }
}

/// Raw storage for a `v8::Locker`. Its size is checked by a static_assert
/// in binding.cc. It is not address-sensitive (it holds two flags and an
/// isolate pointer), but it lives in a `Box` anyway so its address stays
/// stable for the C++ destructor no matter how the Rust guard moves.
#[repr(C)]
pub(crate) struct RawLocker([usize; 2]);

/// Raw storage for a `v8::Unlocker`. Size checked by a static_assert in
/// binding.cc. Boxed for the same reason as [`RawLocker`].
#[repr(C)]
pub(crate) struct RawUnlocker([usize; 1]);

/// An isolate that can be used from multiple threads, one at a time.
///
/// Created with [`crate::OwnedIsolate::try_into_shared`]. All access goes
/// through [`SharedIsolate::lock`], which acquires the isolate's
/// `v8::Locker`, enters the isolate on the current thread, and yields a
/// [`Locker`] guard that dereferences to [`Isolate`].
///
/// Limitations:
/// - [`crate::Weak`] handles are not supported on shared isolates, and an
///   isolate with live weaks or pending finalizers is rejected by conversion.
/// - Snapshot-creator isolates and isolates with a cppgc heap attached
///   are rejected by conversion.
/// - Opening a [`crate::Global`] into a plain reference is unsafe, and that
///   reference must remain on the current thread and not outlive the lock.
///   Prefer converting it to a [`crate::Local`] under a handle scope. Other
///   access, such as cloning, hashing, or comparing, also requires holding the
///   lock on the current thread.
///
/// [`crate::Global`]s are `Send` and may be dropped on any thread at any time:
/// if the
/// dropping thread holds the lock its V8 cell is reset immediately. Otherwise
/// the reset is deferred until the next lock acquisition, [`Locker::unlock`],
/// [`Locker`] drop, or isolate teardown. Until then it remains a GC root and
/// may retain the JavaScript object graph it references.
///
/// # Blocking under the lock
///
/// The lock is held for as long as its [`Locker`] guard lives, and
/// [`SharedIsolate::lock`] has no timeout and no `try_lock`. Anything that
/// blocks while holding it — a Rust callback doing I/O, a long computation,
/// JS that parks — blocks *every* other thread that wants this isolate, and
/// a thread that blocks forever wedges the isolate permanently.
///
/// Use [`Locker::unlock`] to release the lock around such work so other
/// threads can make progress in the meantime.
///
/// # Cost
///
/// [`SharedIsolate::lock`] is not uniformly cheap: its cost scales with how
/// often the *entering thread changes*. V8's `ThreadManager` archives an
/// isolate's per-thread state when a different thread takes the lock and
/// restores it on the way back in, so a sequence of locks from one thread is
/// far cheaper than the same sequence alternating between two.
///
/// This matters for work-stealing executors, which are free to run each of
/// an isolate's turns on a different worker and so maximise the migration.
/// A measured case: an embedder serving a trivial request per lock lost
/// about 9% of its throughput moving from one worker thread to twelve, with
/// the *same* number of locks in both — the loss was migration alone. If an
/// embedder can keep consecutive locks of one isolate on one thread, or run
/// fewer workers, it is worth doing.
///
/// # Initialize V8 before creating the threads that will lock
///
/// Every thread that calls [`SharedIsolate::lock`] must have been created
/// *after* [`V8::initialize`](crate::V8::initialize). On hardware with
/// memory protection keys, V8 guards its pointer tables with a key whose
/// access lives in the per-thread `PKRU` register, and a thread inherits
/// that access only at creation time. A worker created before V8 was
/// initialized will lock successfully and then take `SIGSEGV` with
/// `si_code == SEGV_PKUERR` the first time it runs JavaScript.
///
/// The failure is easy to misread: it presents as a fault on a mapped,
/// readable page, and no debug build, heap verifier or handle check
/// reports anything, because nothing is corrupt. See
/// [`V8::initialize`](crate::V8::initialize) for the full description.
#[derive(Debug)]
pub struct SharedIsolate {
  inner: Arc<SharedIsolateInner>,
}

#[derive(Debug)]
struct SharedIsolateInner {
  cxx_isolate: NonNull<RealIsolate>,
  isolate_handle: IsolateHandle,
}

// SAFETY: V8 access is serialized by `v8::Locker`; the only operation exposed
// without it is cloning the separately synchronized `IsolateHandle`.
unsafe impl Send for SharedIsolateInner {}
unsafe impl Sync for SharedIsolateInner {}

impl SharedIsolate {
  pub(crate) fn new(
    cxx_isolate: NonNull<RealIsolate>,
    isolate_handle: IsolateHandle,
  ) -> Self {
    Self {
      inner: Arc::new(SharedIsolateInner {
        cxx_isolate,
        isolate_handle,
      }),
    }
  }

  pub(crate) fn as_real_ptr(&self) -> *mut RealIsolate {
    self.inner.cxx_isolate.as_ptr()
  }

  /// Acquire the isolate's lock and enter it on the current thread,
  /// blocking until any other thread holding the lock releases it.
  ///
  /// # Panics
  ///
  /// Panics if the current thread already holds the lock. `v8::Locker` is
  /// recursive, but two live guards would hand out aliasing `&mut Isolate`
  /// references, so recursive locking is forbidden here. Also panics if a
  /// different isolate is currently entered: permitting nested [`Locker`]
  /// guards would make it possible to drop them out of order, which V8 cannot
  /// recover from.
  pub fn lock(&self) -> Locker<'_> {
    Locker::new(self)
  }

  /// A handle usable from any thread without holding the lock, for
  /// [`IsolateHandle::terminate_execution`] and
  /// [`IsolateHandle::request_interrupt`].
  ///
  /// Obtaining one through a [`Locker`] would require the very lock the
  /// runaway thread is holding, so take it from here instead.
  pub fn thread_safe_handle(&self) -> IsolateHandle {
    self.inner.isolate_handle.clone()
  }
}

impl Drop for SharedIsolateInner {
  fn drop(&mut self) {
    // Every Locker owns an Arc to this allocation. Forgetting a Locker therefore
    // leaks the allocation and its still-entered V8 isolate instead of allowing
    // this destructor to dispose an isolate that V8 still considers in use.
    // With no Lockers left, other threads may still be dropping Globals:
    // drain the deferred queue and close it under the lock, then tear down the
    // same way `OwnedIsolate::drop` does.
    unsafe {
      let mut isolate = Isolate::from_non_null(self.cxx_isolate);
      let ptr = self.cxx_isolate.as_ptr();
      let mut raw = Box::new(RawLocker([0; 2]));
      v8__Locker__CONSTRUCT(&mut *raw, ptr);
      isolate
        .global_liveness()
        .as_ref()
        .close_deferred_global_resets();
      v8__Locker__DESTRUCT(&mut *raw);
      let (annex_ptr, _create_param_allocations) =
        isolate.prepare_annex_for_dispose();
      Isolate::run_remaining_guaranteed_finalizers(annex_ptr);
      crate::Platform::notify_isolate_shutdown(
        &crate::V8::get_current_platform(),
        &isolate,
      );
      isolate.dispose();
      Isolate::finish_annex_dispose(annex_ptr);
    }
  }
}

/// A guard that holds the `v8::Locker` for a [`SharedIsolate`] and keeps
/// the isolate entered on the current thread. Dereferences to [`Isolate`];
/// construct scopes with e.g. `HandleScope::new(&mut *locker)`.
pub struct Locker<'s> {
  raw: Box<RawLocker>,
  cxx_isolate: NonNull<RealIsolate>,
  // Dropped manually only after the C++ Locker is successfully destroyed. If
  // cleanup panics first, retaining this Arc leaks the isolate rather than
  // allowing its owner to dispose an isolate whose V8 lock is still held.
  inner: ManuallyDrop<Arc<SharedIsolateInner>>,
  _shared: PhantomData<&'s SharedIsolate>,
}

impl<'s> Locker<'s> {
  fn new(shared: &'s SharedIsolate) -> Self {
    let ptr = shared.as_real_ptr();
    unsafe {
      assert!(
        !thread_holds_lock(ptr),
        "attempted to lock an isolate that is already locked by this thread"
      );
      assert!(
        v8__Isolate__TryGetCurrent().is_null(),
        "attempted to lock a shared isolate while another isolate is entered"
      );
      let mut raw = Box::new(RawLocker([0; 2]));
      v8__Locker__CONSTRUCT(&mut *raw, ptr);
      v8__Isolate__Enter(ptr);
      let locker = Self {
        raw,
        cxx_isolate: shared.inner.cxx_isolate,
        inner: ManuallyDrop::new(Arc::clone(&shared.inner)),
        _shared: PhantomData,
      };
      // Release Globals that were dropped by threads not holding the lock.
      locker
        .global_liveness()
        .as_ref()
        .maybe_drain_deferred_global_resets();
      locker
    }
  }

  /// Release the isolate's lock for the duration of `f` so other threads
  /// can lock and use the isolate, then reacquire it before returning.
  ///
  /// Wrap anything that blocks or runs long without needing the isolate —
  /// I/O in a callback, waiting on a channel, a heavy pure-Rust
  /// computation — so it doesn't hold every other thread off the isolate.
  ///
  /// The isolate must not be touched inside `f`. The `&mut self` borrow
  /// enforces that for anything reached through this guard: scopes and
  /// `&Isolate` references derived from it are already dead by the time
  /// `unlock` can be called, and handle operations that would need the
  /// lock (e.g. cloning a [`crate::Global`]) panic while it is released.
  ///
  /// Handles created before the call stay valid — `v8::Unlocker` archives
  /// this thread's isolate state and restores it on the way back in — but
  /// another thread may run JS and collect garbage in the window, so
  /// nothing observed beforehand can be assumed unchanged afterwards.
  ///
  /// # Panics
  ///
  /// Panics if another isolate has been entered on top of this one, since
  /// unlocking would then release the wrong isolate's hold on this thread, or
  /// if `f` returns while an isolate it entered is still current.
  pub fn unlock<R>(&mut self, f: impl FnOnce() -> R) -> R {
    let ptr = self.cxx_isolate.as_ptr();
    unsafe {
      assert!(
        std::ptr::eq(ptr, v8__Isolate__TryGetCurrent()),
        "Locker::unlock called while another isolate was entered on top of \
         this one"
      );
      // Release what other threads queued while we held the lock; they
      // can't do it themselves, and we're about to stop being able to.
      self
        .global_liveness()
        .as_ref()
        .maybe_drain_deferred_global_resets();
    }
    unsafe { v8__Isolate__Exit(ptr) };
    let mut raw = Box::new(RawUnlocker([0; 1]));
    // V8 requires the isolate to be exited before constructing an Unlocker.
    // The guard destroys the Unlocker (reacquiring the lock) and then
    // re-enters the isolate, including when `f` unwinds.
    unsafe { v8__Unlocker__CONSTRUCT(&mut *raw, ptr) };
    let _relock = RelockGuard {
      raw,
      cxx_isolate: self.cxx_isolate,
    };
    let result = f();
    // `result` was created after `_relock`, so if this assertion unwinds it is
    // dropped first. That gives a returned OwnedIsolate or Locker a chance to
    // exit cleanly before `_relock` restores this isolate.
    assert!(
      unsafe { v8__Isolate__TryGetCurrent().is_null() },
      "Locker::unlock closure returned while an isolate was still entered"
    );
    result
  }
}

/// Reacquires the lock and re-enters the isolate when the unlock window
/// ends, on the normal path and while unwinding alike.
struct RelockGuard {
  raw: Box<RawUnlocker>,
  cxx_isolate: NonNull<RealIsolate>,
}

impl Drop for RelockGuard {
  fn drop(&mut self) {
    unsafe {
      v8__Unlocker__DESTRUCT(&mut *self.raw);
      v8__Isolate__Enter(self.cxx_isolate.as_ptr());
      // Globals may have been dropped while the lock was released. This is a
      // lock acquisition boundary just like `SharedIsolate::lock()`.
      Isolate::from_non_null(self.cxx_isolate)
        .global_liveness()
        .as_ref()
        .maybe_drain_deferred_global_resets();
    }
  }
}

impl Drop for Locker<'_> {
  fn drop(&mut self) {
    unsafe {
      // Final drain while we still hold the lock, so cells dropped by
      // other threads during this lock don't sit in the queue (keeping
      // their JS objects alive) until the next acquisition.
      self
        .global_liveness()
        .as_ref()
        .maybe_drain_deferred_global_resets();
      assert!(
        std::ptr::eq(self.cxx_isolate.as_ptr(), v8__Isolate__TryGetCurrent()),
        "Locker dropped while its isolate was not the entered one; lockers \
         must be dropped in reverse order of creation"
      );
      v8__Isolate__Exit(self.cxx_isolate.as_ptr());
      v8__Locker__DESTRUCT(&mut *self.raw);
      ManuallyDrop::drop(&mut self.inner);
    }
  }
}

impl Deref for Locker<'_> {
  type Target = Isolate;
  fn deref(&self) -> &Isolate {
    unsafe { Isolate::from_raw_ref(&self.cxx_isolate) }
  }
}

impl DerefMut for Locker<'_> {
  fn deref_mut(&mut self) -> &mut Isolate {
    unsafe { Isolate::from_raw_ref_mut(&mut self.cxx_isolate) }
  }
}
