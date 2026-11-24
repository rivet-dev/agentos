// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Function;
use crate::Isolate;
use crate::Local;
use crate::MicrotasksPolicy;
use crate::isolate::RealIsolate;
use crate::support::Opaque;
use crate::support::int;
use std::ops::Deref;
use std::ptr::NonNull;

#[repr(C)]
struct MicrotaskQueueHandleRaw(Opaque);

unsafe extern "C" {
  fn v8__MicrotaskQueueHandle__New(
    isolate: *mut RealIsolate,
    policy: MicrotasksPolicy,
  ) -> *mut MicrotaskQueueHandleRaw;
  fn v8__MicrotaskQueueHandle__DELETE(handle: *mut MicrotaskQueueHandleRaw);
  fn v8__MicrotaskQueueHandle__Get(
    handle: *const MicrotaskQueueHandleRaw,
  ) -> *mut MicrotaskQueue;
  fn v8__MicrotaskQueue__PerformCheckpoint(
    isolate: *mut RealIsolate,
    queue: *const MicrotaskQueue,
  );
  fn v8__MicrotaskQueue__BelongsTo(
    queue: *const MicrotaskQueue,
    isolate: *mut RealIsolate,
  ) -> bool;
  fn v8__MicrotaskQueue__IsRunningMicrotasks(
    queue: *const MicrotaskQueue,
  ) -> bool;
  fn v8__MicrotaskQueue__GetMicrotasksScopeDepth(
    queue: *const MicrotaskQueue,
  ) -> int;
  fn v8__MicrotaskQueue__EnqueueMicrotask(
    isolate: *mut RealIsolate,
    queue: *const MicrotaskQueue,
    microtask: *const Function,
  );
}

/// Represents the microtask queue, where microtasks are stored and processed.
/// https://html.spec.whatwg.org/multipage/webappapis.html#microtask-queue
/// https://html.spec.whatwg.org/multipage/webappapis.html#enqueuejob(queuename,-job,-arguments)
/// https://html.spec.whatwg.org/multipage/webappapis.html#perform-a-microtask-checkpoint
///
/// A MicrotaskQueue instance may be associated to multiple Contexts by passing
/// it to Context::New(), and they can be detached by Context::DetachGlobal().
///
/// Use the same instance of MicrotaskQueue for all Contexts that may access each
/// other synchronously. E.g. for Web embedding, use the same instance for all
/// origins that share the same URL scheme and eTLD+1.
#[repr(C)]
#[derive(Debug)]
pub struct MicrotaskQueue(Opaque);

impl MicrotaskQueue {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(
    isolate: &mut Isolate,
    policy: MicrotasksPolicy,
  ) -> MicrotaskQueueHandle {
    let handle =
      unsafe { v8__MicrotaskQueueHandle__New(isolate.as_real_ptr(), policy) };
    MicrotaskQueueHandle(NonNull::new(handle).unwrap())
  }

  pub fn enqueue_microtask(
    &self,
    isolate: &mut Isolate,
    microtask: Local<Function>,
  ) {
    self.assert_belongs_to(isolate);
    crate::handle::assert_locals_in_isolate(isolate);
    unsafe {
      v8__MicrotaskQueue__EnqueueMicrotask(
        isolate.as_real_ptr(),
        self,
        &*microtask,
      )
    }
  }

  /// Adds a callback to notify the embedder after microtasks were run. The
  /// callback is triggered by explicit RunMicrotasks call or automatic
  /// microtasks execution (see Isolate::SetMicrotasksPolicy).
  ///
  /// Callback will trigger even if microtasks were attempted to run,
  /// but the microtasks queue was empty and no single microtask was actually
  /// executed.
  ///
  /// Executing scripts inside the callback will not re-trigger microtasks and
  /// the callback.
  pub fn perform_checkpoint(&self, isolate: &mut Isolate) {
    self.assert_belongs_to(isolate);
    unsafe {
      v8__MicrotaskQueue__PerformCheckpoint(isolate.as_real_ptr(), self);
    }
  }

  /// Panics unless this queue belongs to `isolate`.
  ///
  /// This takes time proportional to the number of microtask queues of
  /// `isolate`.
  pub(crate) fn assert_belongs_to(&self, isolate: &Isolate) {
    assert!(
      unsafe { v8__MicrotaskQueue__BelongsTo(self, isolate.as_real_ptr()) },
      "attempt to use a MicrotaskQueue in an Isolate other than its own"
    );
  }

  /// Removes callback that was installed by AddMicrotasksCompletedCallback.
  pub fn is_running_microtasks(&self) -> bool {
    unsafe { v8__MicrotaskQueue__IsRunningMicrotasks(self) }
  }

  /// Returns the current depth of nested MicrotasksScope that has kRunMicrotasks.
  pub fn get_microtasks_scope_depth(&self) -> i32 {
    unsafe { v8__MicrotaskQueue__GetMicrotasksScopeDepth(self) }
  }
}

/// A rooted handle to a [`MicrotaskQueue`].
///
/// This handle must be dropped before its isolate is disposed because dropping
/// it releases a root through the isolate's heap.
///
/// With the default `v8_cppgc_microtask_queue` GN setting, each associated
/// context keeps the queue alive after this handle is dropped until it is
/// detached. If that setting is disabled through `EXTRA_GN_ARGS`, this handle
/// owns the queue and must outlive every associated context.
#[derive(Debug)]
pub struct MicrotaskQueueHandle(NonNull<MicrotaskQueueHandleRaw>);

impl Deref for MicrotaskQueueHandle {
  type Target = MicrotaskQueue;

  fn deref(&self) -> &Self::Target {
    unsafe { &*v8__MicrotaskQueueHandle__Get(self.0.as_ptr()) }
  }
}

impl AsRef<MicrotaskQueue> for MicrotaskQueueHandle {
  fn as_ref(&self) -> &MicrotaskQueue {
    self
  }
}

impl Drop for MicrotaskQueueHandle {
  fn drop(&mut self) {
    unsafe { v8__MicrotaskQueueHandle__DELETE(self.0.as_ptr()) }
  }
}
