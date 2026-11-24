use crate::Isolate;
use crate::isolate::RealIsolate;
use crate::support::int;

use crate::support::Opaque;
use crate::support::Shared;
use crate::support::SharedPtrBase;
use crate::support::SharedRef;
use crate::support::UniquePtr;
use crate::support::UniqueRef;
use crate::support::long;

unsafe extern "C" {
  fn v8__Platform__NewDefaultPlatform(
    thread_pool_size: int,
    idle_task_support: bool,
  ) -> *mut Platform;
  fn v8__Platform__NewUnprotectedDefaultPlatform(
    thread_pool_size: int,
    idle_task_support: bool,
  ) -> *mut Platform;
  fn v8__Platform__NewSingleThreadedDefaultPlatform(
    idle_task_support: bool,
  ) -> *mut Platform;
  fn v8__Platform__NewCustomPlatform(
    thread_pool_size: int,
    idle_task_support: bool,
    unprotected: bool,
    context: *mut std::ffi::c_void,
  ) -> *mut Platform;
  fn v8__Platform__DELETE(this: *mut Platform);

  fn v8__Platform__PumpMessageLoop(
    platform: *mut Platform,
    isolate: *mut RealIsolate,
    wait_for_work: bool,
  ) -> bool;

  fn v8__Platform__RunIdleTasks(
    platform: *mut Platform,
    isolate: *mut RealIsolate,
    idle_time_in_seconds: f64,
  );

  fn v8__Platform__NotifyIsolateShutdown(
    platform: *mut Platform,
    isolate: *mut RealIsolate,
  );

  fn v8__Task__Run(task: *mut std::ffi::c_void);
  fn v8__Task__DELETE(task: *mut std::ffi::c_void);
  fn v8__IdleTask__Run(task: *mut std::ffi::c_void, deadline_in_seconds: f64);
  fn v8__IdleTask__DELETE(task: *mut std::ffi::c_void);

  fn std__shared_ptr__v8__Platform__CONVERT__std__unique_ptr(
    unique_ptr: UniquePtr<Platform>,
  ) -> SharedPtrBase<Platform>;
  fn std__shared_ptr__v8__Platform__get(
    ptr: *const SharedPtrBase<Platform>,
  ) -> *mut Platform;
  fn std__shared_ptr__v8__Platform__COPY(
    ptr: *const SharedPtrBase<Platform>,
  ) -> SharedPtrBase<Platform>;
  fn std__shared_ptr__v8__Platform__reset(ptr: *mut SharedPtrBase<Platform>);
  fn std__shared_ptr__v8__Platform__use_count(
    ptr: *const SharedPtrBase<Platform>,
  ) -> long;
}

#[repr(C)]
#[derive(Debug)]
pub struct Platform(Opaque);

/// A V8 foreground task. Ownership is transferred from C++ to Rust when
/// V8 posts a task via [`PlatformImpl`] trait methods.
///
/// The embedder is responsible for scheduling the task and calling
/// [`run()`](Task::run). For example, in an async runtime like tokio:
///
/// ```ignore
/// tokio::spawn(async move { task.run() });
/// ```
///
/// If dropped without calling `run()`, the task is destroyed without
/// executing.
pub struct Task(*mut std::ffi::c_void);

// SAFETY: V8 tasks are designed to be posted from background threads and
// run on the isolate's foreground thread. The unique_ptr transfer is safe
// across thread boundaries.
unsafe impl Send for Task {}

impl Task {
  /// Run the task. Consumes self to prevent double execution.
  pub fn run(self) {
    let ptr = self.0;
    // Prevent Drop from deleting — we'll delete after Run.
    std::mem::forget(self);
    unsafe {
      v8__Task__Run(ptr);
      v8__Task__DELETE(ptr);
    }
  }
}

impl Drop for Task {
  fn drop(&mut self) {
    unsafe { v8__Task__DELETE(self.0) };
  }
}

/// A V8 idle task. Similar to [`Task`] but accepts a deadline parameter
/// when run.
///
/// If dropped without calling `run()`, the task is destroyed without
/// executing.
pub struct IdleTask(*mut std::ffi::c_void);

// SAFETY: Same as Task — safe to transfer across threads.
unsafe impl Send for IdleTask {}

impl IdleTask {
  /// Run the idle task with the given deadline. Consumes self.
  ///
  /// `deadline_in_seconds` is the absolute time (in seconds since some
  /// epoch) by which the idle task should complete.
  pub fn run(self, deadline_in_seconds: f64) {
    let ptr = self.0;
    std::mem::forget(self);
    unsafe {
      v8__IdleTask__Run(ptr, deadline_in_seconds);
      v8__IdleTask__DELETE(ptr);
    }
  }
}

impl Drop for IdleTask {
  fn drop(&mut self) {
    unsafe { v8__IdleTask__DELETE(self.0) };
  }
}

/// Trait for customizing platform behavior, following the same pattern as
/// [`V8InspectorClientImpl`](crate::inspector::V8InspectorClientImpl).
///
/// Implement this trait to receive V8 foreground tasks and schedule them
/// on your event loop. The C++ `CustomPlatform` wraps each isolate's
/// `TaskRunner` so that every `PostTask` / `PostDelayedTask` / etc. call
/// transfers task ownership to Rust through the corresponding trait method.
///
/// **The embedder is responsible for calling [`Task::run()`] on the
/// isolate's thread.** For example, using tokio:
///
/// ```ignore
/// fn post_task(&self, isolate_ptr: *mut c_void, task: Task) {
///     tokio::spawn(async move { task.run() });
/// }
///
/// fn post_delayed_task(&self, isolate_ptr: *mut c_void, task: Task, delay: f64) {
///     tokio::spawn(async move {
///         tokio::time::sleep(Duration::from_secs_f64(delay)).await;
///         task.run();
///     });
/// }
/// ```
///
/// All methods have default implementations that run the task immediately
/// (synchronously). Override to integrate with your event loop.
///
/// Implementations must be `Send + Sync` as callbacks may fire from any
/// thread.
#[allow(unused_variables)]
pub trait PlatformImpl: Send + Sync {
  /// Called when `TaskRunner::PostTask` is invoked for the given isolate.
  ///
  /// The [`Task`] must be run on the isolate's foreground thread by calling
  /// [`Task::run()`].
  ///
  /// May be called from ANY thread (V8 background threads, etc.).
  fn post_task(&self, isolate_ptr: *mut std::ffi::c_void, task: Task) {
    task.run();
  }

  /// Called when `TaskRunner::PostNonNestableTask` is invoked.
  ///
  /// Same semantics as [`post_task`](Self::post_task), but the task must
  /// not be run within a nested `PumpMessageLoop`.
  fn post_non_nestable_task(
    &self,
    isolate_ptr: *mut std::ffi::c_void,
    task: Task,
  ) {
    task.run();
  }

  /// Called when `TaskRunner::PostDelayedTask` is invoked.
  ///
  /// The task should be run after `delay_in_seconds` has elapsed.
  /// For example, using `tokio::time::sleep` or a timer wheel.
  ///
  /// May be called from ANY thread.
  fn post_delayed_task(
    &self,
    isolate_ptr: *mut std::ffi::c_void,
    task: Task,
    delay_in_seconds: f64,
  ) {
    task.run();
  }

  /// Called when `TaskRunner::PostNonNestableDelayedTask` is invoked.
  ///
  /// Same semantics as [`post_delayed_task`](Self::post_delayed_task).
  fn post_non_nestable_delayed_task(
    &self,
    isolate_ptr: *mut std::ffi::c_void,
    task: Task,
    delay_in_seconds: f64,
  ) {
    task.run();
  }

  /// Called when `TaskRunner::PostIdleTask` is invoked.
  ///
  /// The [`IdleTask`] should be run when the embedder has idle time,
  /// passing the deadline via [`IdleTask::run(deadline)`](IdleTask::run).
  fn post_idle_task(&self, isolate_ptr: *mut std::ffi::c_void, task: IdleTask) {
    task.run(0.0);
  }
}

// FFI callbacks called from C++ CustomPlatform/CustomTaskRunner.
// `context` is a raw pointer to a `Box<dyn PlatformImpl>`.
// Task pointers are owned — Rust is responsible for running and deleting them.

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__PostTask(
  context: *mut std::ffi::c_void,
  isolate: *mut std::ffi::c_void,
  task: *mut std::ffi::c_void,
) {
  let imp = unsafe { &*(context as *const Box<dyn PlatformImpl>) };
  imp.post_task(isolate, Task(task));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__PostNonNestableTask(
  context: *mut std::ffi::c_void,
  isolate: *mut std::ffi::c_void,
  task: *mut std::ffi::c_void,
) {
  let imp = unsafe { &*(context as *const Box<dyn PlatformImpl>) };
  imp.post_non_nestable_task(isolate, Task(task));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__PostDelayedTask(
  context: *mut std::ffi::c_void,
  isolate: *mut std::ffi::c_void,
  task: *mut std::ffi::c_void,
  delay_in_seconds: f64,
) {
  let imp = unsafe { &*(context as *const Box<dyn PlatformImpl>) };
  imp.post_delayed_task(isolate, Task(task), delay_in_seconds);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__PostNonNestableDelayedTask(
  context: *mut std::ffi::c_void,
  isolate: *mut std::ffi::c_void,
  task: *mut std::ffi::c_void,
  delay_in_seconds: f64,
) {
  let imp = unsafe { &*(context as *const Box<dyn PlatformImpl>) };
  imp.post_non_nestable_delayed_task(isolate, Task(task), delay_in_seconds);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__PostIdleTask(
  context: *mut std::ffi::c_void,
  isolate: *mut std::ffi::c_void,
  task: *mut std::ffi::c_void,
) {
  let imp = unsafe { &*(context as *const Box<dyn PlatformImpl>) };
  imp.post_idle_task(isolate, IdleTask(task));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8__Platform__CustomPlatform__BASE__DROP(
  context: *mut std::ffi::c_void,
) {
  unsafe {
    let _ = Box::from_raw(context as *mut Box<dyn PlatformImpl>);
  }
}

/// Returns a new instance of the default v8::Platform implementation.
///
/// |thread_pool_size| is the number of worker threads to allocate for
/// background jobs. If a value of zero is passed, a suitable default
/// based on the current number of processors online will be chosen.
/// If |idle_task_support| is enabled then the platform will accept idle
/// tasks (IdleTasksEnabled will return true) and will rely on the embedder
/// calling v8::platform::RunIdleTasks to process the idle tasks.
///
/// The default platform for v8 may include restrictions and caveats on thread
/// creation and initialization. This platform should only be used in cases
/// where v8 can be reliably initialized on the application's main thread, or
/// the parent thread to all threads in the system that will use v8.
///
/// One example of a restriction is the use of Memory Protection Keys (pkeys) on
/// modern Linux systems using modern Intel/AMD processors. This particular
/// technology requires that all threads using v8 are created as descendent
/// threads of the thread that called `v8::Initialize`.
#[inline(always)]
pub fn new_default_platform(
  thread_pool_size: u32,
  idle_task_support: bool,
) -> UniqueRef<Platform> {
  Platform::new(thread_pool_size, idle_task_support)
}

/// Creates a platform that is identical to the default platform, but does not
/// enforce thread-isolated allocations. This may reduce security in some cases,
/// so this method should be used with caution in cases where the threading
/// guarantees of `new_default_platform` cannot be upheld (generally for tests).
#[inline(always)]
pub fn new_unprotected_default_platform(
  thread_pool_size: u32,
  idle_task_support: bool,
) -> UniqueRef<Platform> {
  Platform::new_unprotected(thread_pool_size, idle_task_support)
}

/// The same as new_default_platform() but disables the worker thread pool.
/// It must be used with the --single-threaded V8 flag.
///
/// If |idle_task_support| is enabled then the platform will accept idle
/// tasks (IdleTasksEnabled will return true) and will rely on the embedder
/// calling v8::platform::RunIdleTasks to process the idle tasks.
#[inline(always)]
pub fn new_single_threaded_default_platform(
  idle_task_support: bool,
) -> UniqueRef<Platform> {
  Platform::new_single_threaded(idle_task_support)
}

/// Creates a custom platform backed by `DefaultPlatform` that transfers
/// foreground task ownership to the provided [`PlatformImpl`] trait object.
///
/// Unlike the default platform, foreground tasks are NOT queued internally.
/// Instead, each `PostTask` / `PostDelayedTask` / etc. call transfers the
/// [`Task`] to Rust via the trait. The embedder is responsible for
/// scheduling and calling [`Task::run()`] on the isolate's thread.
///
/// Background tasks (thread pool) are still handled by `DefaultPlatform`.
///
/// When `unprotected` is true, thread-isolated allocations are disabled
/// (same as `new_unprotected_default_platform`). This is required when
/// isolates may be created on threads other than the one that called
/// `V8::initialize`.
#[inline(always)]
pub fn new_custom_platform(
  thread_pool_size: u32,
  idle_task_support: bool,
  unprotected: bool,
  platform_impl: impl PlatformImpl + 'static,
) -> UniqueRef<Platform> {
  Platform::new_custom(
    thread_pool_size,
    idle_task_support,
    unprotected,
    platform_impl,
  )
}

impl Platform {
  /// Returns a new instance of the default v8::Platform implementation.
  ///
  /// |thread_pool_size| is the number of worker threads to allocate for
  /// background jobs. If a value of zero is passed, a suitable default
  /// based on the current number of processors online will be chosen.
  /// If |idle_task_support| is enabled then the platform will accept idle
  /// tasks (IdleTasksEnabled will return true) and will rely on the embedder
  /// calling v8::platform::RunIdleTasks to process the idle tasks.
  ///
  /// The default platform for v8 may include restrictions and caveats on thread
  /// creation and initialization. This platform should only be used in cases
  /// where v8 can be reliably initialized on the application's main thread, or
  /// the parent thread to all threads in the system that will use v8.
  ///
  /// One example of a restriction is the use of Memory Protection Keys (pkeys)
  /// on modern Linux systems using modern Intel/AMD processors. This particular
  /// technology requires that all threads using v8 are created as descendent
  /// threads of the thread that called `v8::Initialize`.
  #[inline(always)]
  pub fn new(
    thread_pool_size: u32,
    idle_task_support: bool,
  ) -> UniqueRef<Self> {
    unsafe {
      UniqueRef::from_raw(v8__Platform__NewDefaultPlatform(
        thread_pool_size.min(16) as i32,
        idle_task_support,
      ))
    }
  }

  /// Creates a platform that is identical to the default platform, but does not
  /// enforce thread-isolated allocations. This may reduce security in some
  /// cases, so this method should be used with caution in cases where the
  /// threading guarantees of `new_default_platform` cannot be upheld (generally
  /// for tests).
  #[inline(always)]
  pub fn new_unprotected(
    thread_pool_size: u32,
    idle_task_support: bool,
  ) -> UniqueRef<Self> {
    unsafe {
      UniqueRef::from_raw(v8__Platform__NewUnprotectedDefaultPlatform(
        thread_pool_size.min(16) as i32,
        idle_task_support,
      ))
    }
  }

  /// The same as new() but disables the worker thread pool.
  /// It must be used with the --single-threaded V8 flag.
  ///
  /// If |idle_task_support| is enabled then the platform will accept idle
  /// tasks (IdleTasksEnabled will return true) and will rely on the embedder
  /// calling v8::platform::RunIdleTasks to process the idle tasks.
  #[inline(always)]
  pub fn new_single_threaded(idle_task_support: bool) -> UniqueRef<Self> {
    unsafe {
      UniqueRef::from_raw(v8__Platform__NewSingleThreadedDefaultPlatform(
        idle_task_support,
      ))
    }
  }

  /// Creates a custom platform that transfers foreground task ownership to
  /// the provided [`PlatformImpl`] trait object.
  ///
  /// See [`new_custom_platform`] for details.
  ///
  /// The trait object is owned by the platform and will be dropped when the
  /// platform is destroyed.
  #[inline(always)]
  pub fn new_custom(
    thread_pool_size: u32,
    idle_task_support: bool,
    unprotected: bool,
    platform_impl: impl PlatformImpl + 'static,
  ) -> UniqueRef<Self> {
    // Double-box: inner Box<dyn> is a fat pointer, outer Box gives us a
    // thin pointer we can pass through C++ void*.
    let boxed: Box<dyn PlatformImpl> = Box::new(platform_impl);
    let context = Box::into_raw(Box::new(boxed)) as *mut std::ffi::c_void;
    // thread_pool_size clamping (0 → hardware_concurrency, max 16) is
    // handled on the C++ side in v8__Platform__NewCustomPlatform.
    unsafe {
      UniqueRef::from_raw(v8__Platform__NewCustomPlatform(
        thread_pool_size as i32,
        idle_task_support,
        unprotected,
        context,
      ))
    }
  }
}

impl Platform {
  /// Pumps the message loop for the given isolate.
  ///
  /// The caller has to make sure that this is called from the right thread.
  /// Returns true if a task was executed, and false otherwise. If the call to
  /// PumpMessageLoop is nested within another call to PumpMessageLoop, only
  /// nestable tasks may run. Otherwise, any task may run. Unless requested through
  /// the |wait_for_work| parameter, this call does not block if no task is pending.
  #[inline(always)]
  pub fn pump_message_loop(
    platform: &SharedRef<Self>,
    isolate: &Isolate,
    wait_for_work: bool,
  ) -> bool {
    unsafe {
      v8__Platform__PumpMessageLoop(
        &**platform as *const Self as *mut _,
        isolate.as_real_ptr(),
        wait_for_work,
      )
    }
  }

  /// Runs pending idle tasks for at most |idle_time_in_seconds| seconds.
  ///
  /// The caller has to make sure that this is called from the right thread.
  /// This call does not block if no task is pending.
  #[inline(always)]
  pub fn run_idle_tasks(
    platform: &SharedRef<Self>,
    isolate: &Isolate,
    idle_time_in_seconds: f64,
  ) {
    unsafe {
      v8__Platform__RunIdleTasks(
        &**platform as *const Self as *mut _,
        isolate.as_real_ptr(),
        idle_time_in_seconds,
      );
    }
  }

  /// Notifies the given platform about the Isolate getting deleted soon. Has to
  /// be called for all Isolates which are deleted - unless we're shutting down
  /// the platform.
  ///
  /// The |platform| has to be created using |NewDefaultPlatform|.
  #[inline(always)]
  pub(crate) unsafe fn notify_isolate_shutdown(
    platform: &SharedRef<Self>,
    isolate: &Isolate,
  ) {
    unsafe {
      v8__Platform__NotifyIsolateShutdown(
        &**platform as *const Self as *mut _,
        isolate.as_real_ptr(),
      );
    }
  }
}

impl Shared for Platform {
  fn from_unique_ptr(unique_ptr: UniquePtr<Self>) -> SharedPtrBase<Self> {
    unsafe {
      std__shared_ptr__v8__Platform__CONVERT__std__unique_ptr(unique_ptr)
    }
  }
  fn get(ptr: &SharedPtrBase<Self>) -> *const Self {
    unsafe { std__shared_ptr__v8__Platform__get(ptr) }
  }
  fn clone(ptr: &SharedPtrBase<Self>) -> SharedPtrBase<Self> {
    unsafe { std__shared_ptr__v8__Platform__COPY(ptr) }
  }
  fn reset(ptr: &mut SharedPtrBase<Self>) {
    unsafe { std__shared_ptr__v8__Platform__reset(ptr) }
  }
  fn use_count(ptr: &SharedPtrBase<Self>) -> long {
    unsafe { std__shared_ptr__v8__Platform__use_count(ptr) }
  }
}

impl Drop for Platform {
  fn drop(&mut self) {
    unsafe { v8__Platform__DELETE(self) };
  }
}
