// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

use crate::Function;
use crate::Isolate;
use crate::Local;
use crate::MicrotasksPolicy;
use crate::UniqueRef;
use crate::support::Opaque;
use crate::support::int;

unsafe extern "C" {
  fn v8__MicrotaskQueue__New(
    isolate: *mut Isolate,
    policy: MicrotasksPolicy,
  ) -> *mut MicrotaskQueue;
  fn v8__MicrotaskQueue__DESTRUCT(queue: *mut MicrotaskQueue);
  fn v8__MicrotaskQueue__PerformCheckpoint(
    isolate: *mut Isolate,
    queue: *const MicrotaskQueue,
  );
  fn v8__MicrotaskQueue__IsRunningMicrotasks(
    queue: *const MicrotaskQueue,
  ) -> bool;
  fn v8__MicrotaskQueue__GetMicrotasksScopeDepth(
    queue: *const MicrotaskQueue,
  ) -> int;
  fn v8__MicrotaskQueue__EnqueueMicrotask(
    isolate: *mut Isolate,
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
/// The embedder must keep the MicrotaskQueue instance alive until all associated
/// Contexts are gone or detached.
///
/// Use the same instance of MicrotaskQueue for all Contexts that may access each
/// other synchronously. E.g. for Web embedding, use the same instance for all
/// origins that share the same URL scheme and eTLD+1.
#[repr(C)]
#[derive(Debug)]
pub struct MicrotaskQueue(Opaque);

impl MicrotaskQueue {
  pub fn new(
    isolate: &mut Isolate,
    policy: MicrotasksPolicy,
  ) -> UniqueRef<Self> {
    unsafe { UniqueRef::from_raw(v8__MicrotaskQueue__New(isolate, policy)) }
  }

  pub fn enqueue_microtask(
    &self,
    isolate: &mut Isolate,
    microtask: Local<Function>,
  ) {
    unsafe { v8__MicrotaskQueue__EnqueueMicrotask(isolate, self, &*microtask) }
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
    unsafe {
      v8__MicrotaskQueue__PerformCheckpoint(isolate, self);
    }
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

impl Drop for MicrotaskQueue {
  fn drop(&mut self) {
    unsafe { v8__MicrotaskQueue__DESTRUCT(self) }
  }
}
