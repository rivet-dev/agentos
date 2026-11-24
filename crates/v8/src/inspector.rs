// Copyright 2019-2021 the Deno authors. All rights reserved. MIT license.

//! Bindings to the V8 Inspector API. Documentation for the V8 inspector API is
//! very sparse, so here are a few references for the next sorry soul who has to
//! dig into it.
//!
//! https://medium.com/@hyperandroid/v8-inspector-from-an-embedder-standpoint-7f9c0472e2b7
//! https://v8.dev/docs/inspector
//! https://chromedevtools.github.io/debugger-protocol-viewer/tot/
//! https://cs.chromium.org/chromium/src/v8/include/v8-inspector.h
//! https://github.com/nodejs/node/blob/v13.7.0/src/inspector_agent.cc
//! https://github.com/nodejs/node/blob/v13.7.0/src/inspector_agent.h
//! https://github.com/nodejs/node/tree/v13.7.0/src/inspector
//! https://github.com/denoland/deno/blob/v0.38.0/cli/inspector.rs

use crate::CallbackScope;
use crate::Context;
use crate::Isolate;
use crate::IsolateHandle;
use crate::Local;
use crate::PinScope;
use crate::StackTrace;
use crate::Value;
use crate::crdtp::CppVecU8;
use crate::isolate::RealIsolate;
use crate::support::CxxVTable;
use crate::support::Opaque;
use crate::support::UniquePtr;
use crate::support::UniqueRef;
use crate::support::int;
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::fmt::{self, Debug, Formatter};
use std::pin::pin;

unsafe extern "C" {
  fn v8_inspector__V8Inspector__Channel__BASE__CONSTRUCT(
    buf: *mut MaybeUninit<RawChannel>,
  );

  fn v8_inspector__V8Inspector__Channel__sendResponse(
    this: *mut RawChannel,
    call_id: int,
    message: UniquePtr<StringBuffer>,
  );
  fn v8_inspector__V8Inspector__Channel__sendNotification(
    this: *mut RawChannel,
    message: UniquePtr<StringBuffer>,
  );
  fn v8_inspector__V8Inspector__Channel__flushProtocolNotifications(
    this: *mut RawChannel,
  );

  fn v8_inspector__V8InspectorClient__BASE__CONSTRUCT(
    buf: *mut MaybeUninit<RawV8InspectorClient>,
  );

  fn v8_inspector__V8InspectorClient__generateUniqueId(
    this: *mut RawV8InspectorClient,
  ) -> i64;
  fn v8_inspector__V8InspectorClient__runMessageLoopOnPause(
    this: *mut RawV8InspectorClient,
    context_group_id: int,
  );
  fn v8_inspector__V8InspectorClient__quitMessageLoopOnPause(
    this: *mut RawV8InspectorClient,
  );
  fn v8_inspector__V8InspectorClient__runIfWaitingForDebugger(
    this: *mut RawV8InspectorClient,
    context_group_id: int,
  );
  fn v8_inspector__V8InspectorClient__consoleAPIMessage(
    this: *mut RawV8InspectorClient,
    context_group_id: int,
    level: int,
    message: &StringView,
    url: &StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: &mut V8StackTrace,
  );

  fn v8_inspector__V8InspectorSession__DELETE(this: *mut RawV8InspectorSession);
  fn v8_inspector__V8InspectorSession__dispatchProtocolMessage(
    session: *mut RawV8InspectorSession,
    message: StringView,
  );
  fn v8_inspector__V8InspectorSession__releaseObjectGroup(
    session: *mut RawV8InspectorSession,
    object_group: StringView,
  );
  fn v8_inspector__V8InspectorSession__wrapObject(
    session: *mut RawV8InspectorSession,
    context: *const Context,
    value: *const Value,
    object_group: StringView,
    generate_preview: bool,
  ) -> *mut RawRemoteObject;
  fn v8_inspector__V8InspectorSession__unwrapObject(
    session: *mut RawV8InspectorSession,
    error: *mut *mut StringBuffer,
    object_id: StringView,
    value: *mut *const Value,
    context: *mut *const Context,
    object_group: *mut *mut StringBuffer,
  ) -> bool;
  fn v8_inspector__RemoteObject__DELETE(this: *mut RawRemoteObject);
  fn v8_inspector__RemoteObject__toBytes(
    this: *const RawRemoteObject,
  ) -> *mut CppVecU8;
  fn v8_inspector__V8InspectorSession__schedulePauseOnNextStatement(
    session: *mut RawV8InspectorSession,
    break_reason: StringView,
    break_details: StringView,
  );
  fn v8_inspector__V8InspectorSession__cancelPauseOnNextStatement(
    session: *mut RawV8InspectorSession,
  );
  fn v8_inspector__V8InspectorSession__canDispatchMethod(
    method: StringView,
  ) -> bool;
  fn v8_inspector__V8InspectorSession__Inspectable__NEW(
    rust_impl: *mut c_void,
  ) -> *mut RawInspectable;
  fn v8_inspector__V8InspectorSession__Inspectable__DELETE(
    inspectable: *mut RawInspectable,
  );
  fn v8_inspector__V8InspectorSession__addInspectedObject(
    session: *mut RawV8InspectorSession,
    inspectable: *mut RawInspectable,
  );

  fn v8_inspector__StringBuffer__DELETE(this: *mut StringBuffer);
  fn v8_inspector__StringBuffer__string(this: &StringBuffer) -> StringView<'_>;
  fn v8_inspector__StringBuffer__create(
    source: StringView,
  ) -> UniquePtr<StringBuffer>;

  fn v8_inspector__V8Inspector__DELETE(this: *mut RawV8Inspector);
  fn v8_inspector__V8Inspector__create(
    isolate: *mut RealIsolate,
    client: *mut RawV8InspectorClient,
  ) -> *mut RawV8Inspector;
  fn v8_inspector__V8Inspector__connect(
    inspector: *mut RawV8Inspector,
    context_group_id: int,
    channel: *mut RawChannel,
    state: StringView,
    client_trust_level: V8InspectorClientTrustLevel,
  ) -> *mut RawV8InspectorSession;
  fn v8_inspector__V8Inspector__contextCreated(
    this: *mut RawV8Inspector,
    context: *const Context,
    contextGroupId: int,
    humanReadableName: StringView,
    auxData: StringView,
  );
  fn v8_inspector__V8Inspector__contextDestroyed(
    this: *mut RawV8Inspector,
    context: *const Context,
  );
  fn v8_inspector__V8Inspector__idleStarted(this: *mut RawV8Inspector);
  fn v8_inspector__V8Inspector__idleFinished(this: *mut RawV8Inspector);
  fn v8_inspector__V8Inspector__asyncTaskScheduled(
    this: *mut RawV8Inspector,
    task_name: StringView,
    task: *const c_void,
    recurring: bool,
  );
  fn v8_inspector__V8Inspector__asyncTaskCanceled(
    this: *mut RawV8Inspector,
    task: *const c_void,
  );
  fn v8_inspector__V8Inspector__asyncTaskStarted(
    this: *mut RawV8Inspector,
    task: *const c_void,
  );
  fn v8_inspector__V8Inspector__asyncTaskFinished(
    this: *mut RawV8Inspector,
    task: *const c_void,
  );
  fn v8_inspector__V8Inspector__allAsyncTasksCanceled(
    this: *mut RawV8Inspector,
  );
  fn v8_inspector__V8Inspector__exceptionThrown(
    this: *mut RawV8Inspector,
    context: *const Context,
    message: StringView,
    exception: *const Value,
    detailed_message: StringView,
    url: StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: *mut V8StackTrace,
    script_id: int,
  ) -> u32;
  fn v8_inspector__V8Inspector__createStackTrace(
    this: *mut RawV8Inspector,
    stack_trace: *const StackTrace,
  ) -> *mut V8StackTrace;
  fn v8_inspector__V8StackTrace__DELETE(this: *mut V8StackTrace);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__sendResponse(
  this: *mut RawChannel,
  call_id: int,
  message: UniquePtr<StringBuffer>,
) {
  unsafe {
    let channel = ChannelHeap::from_raw(this);
    channel.imp.send_response(call_id, message);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__sendNotification(
  this: *mut RawChannel,
  message: UniquePtr<StringBuffer>,
) {
  unsafe {
    ChannelHeap::from_raw(this).imp.send_notification(message);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__flushProtocolNotifications(
  this: *mut RawChannel,
) {
  unsafe {
    ChannelHeap::from_raw(this)
      .imp
      .flush_protocol_notifications();
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__generateUniqueId(
  this: *mut RawV8InspectorClient,
) -> i64 {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .generate_unique_id()
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__runMessageLoopOnPause(
  this: *mut RawV8InspectorClient,
  context_group_id: int,
) {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .run_message_loop_on_pause(context_group_id);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__quitMessageLoopOnPause(
  this: *mut RawV8InspectorClient,
) {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .quit_message_loop_on_pause();
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__runIfWaitingForDebugger(
  this: *mut RawV8InspectorClient,
  context_group_id: int,
) {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .run_if_waiting_for_debugger(context_group_id);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__consoleAPIMessage(
  this: *mut RawV8InspectorClient,
  context_group_id: int,
  level: int,
  message: &StringView,
  url: &StringView,
  line_number: u32,
  column_number: u32,
  stack_trace: &mut V8StackTrace,
) {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .console_api_message(
        context_group_id,
        level,
        message,
        url,
        line_number,
        column_number,
        stack_trace,
      );
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__valueSubtype(
  this: *mut RawV8InspectorClient,
  context: Local<Context>,
  value: Local<Value>,
) -> *mut StringBuffer {
  let scope = pin!(unsafe { CallbackScope::new(context) });
  let mut scope = scope.init();
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .value_subtype(&mut scope, value)
      .and_then(|mut v| v.take())
      .map(|r| r.into_raw())
      .unwrap_or(std::ptr::null_mut())
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__descriptionForValueSubtype(
  this: *mut RawV8InspectorClient,
  context: Local<Context>,
  value: Local<Value>,
) -> *mut StringBuffer {
  let scope = pin!(unsafe { CallbackScope::new(context) });
  let mut scope = scope.init();
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .description_for_value_subtype(&mut scope, value)
      .and_then(|mut v| v.take())
      .map(|r| r.into_raw())
      .unwrap_or(std::ptr::null_mut())
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__ensureDefaultContextInGroup(
  this: *mut RawV8InspectorClient,
  context_group_id: int,
) -> *const Context {
  unsafe {
    match V8InspectorClientHeap::from_raw(this)
      .imp
      .ensure_default_context_in_group(context_group_id)
    {
      Some(h) => &*h,
      None => std::ptr::null_mut(),
    }
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__resourceNameToUrl(
  this: *mut RawV8InspectorClient,
  resource_name: &StringView,
) -> *mut StringBuffer {
  unsafe {
    V8InspectorClientHeap::from_raw(this)
      .imp
      .resource_name_to_url(resource_name)
      .and_then(|mut v| v.take())
      .map(|r| r.into_raw())
      .unwrap_or(std::ptr::null_mut())
  }
}

#[repr(C)]
#[derive(Debug)]
struct RawChannel {
  _cxx_vtable: CxxVTable,
}

#[repr(C)]
pub struct Channel {
  heap: Pin<Box<ChannelHeap>>,
}

impl std::fmt::Debug for Channel {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Channel").finish()
  }
}

#[repr(C)]
struct ChannelHeap {
  raw: UnsafeCell<RawChannel>,
  imp: Box<dyn ChannelImpl>,
  _pinned: PhantomPinned,
}

impl ChannelHeap {
  unsafe fn from_raw<'b>(this: *const RawChannel) -> &'b ChannelHeap {
    unsafe { &(*this.cast::<ChannelHeap>()) }
  }
}

impl Channel {
  pub fn new(imp: Box<dyn ChannelImpl>) -> Self {
    let heap = Box::into_raw(Box::new(MaybeUninit::<ChannelHeap>::uninit()))
      .cast::<ChannelHeap>();

    unsafe {
      let raw = &raw mut (*heap).raw;
      v8_inspector__V8Inspector__Channel__BASE__CONSTRUCT(raw.cast());
      let imp_ptr = &raw mut (*heap).imp;
      imp_ptr.write(imp);
    }

    Self {
      heap: unsafe { Box::into_pin(Box::from_raw(heap.cast::<ChannelHeap>())) },
    }
  }

  fn raw(&self) -> *mut RawChannel {
    self.heap.raw.get()
  }

  pub fn send_response(&self, call_id: i32, message: UniquePtr<StringBuffer>) {
    unsafe {
      v8_inspector__V8Inspector__Channel__sendResponse(
        self.raw(),
        call_id,
        message,
      );
    }
  }
  pub fn send_notification(&self, message: UniquePtr<StringBuffer>) {
    unsafe {
      v8_inspector__V8Inspector__Channel__sendNotification(self.raw(), message);
    }
  }
  pub fn flush_protocol_notifications(&self) {
    unsafe {
      v8_inspector__V8Inspector__Channel__flushProtocolNotifications(
        self.raw(),
      );
    }
  }
}

pub trait ChannelImpl {
  fn send_response(&self, call_id: i32, message: UniquePtr<StringBuffer>);
  fn send_notification(&self, message: UniquePtr<StringBuffer>);
  fn flush_protocol_notifications(&self);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::support::UniquePtr;
  use std::sync::atomic::AtomicUsize;
  use std::sync::atomic::Ordering::SeqCst;

  static MESSAGE: &[u8] = b"Hello Pluto!";
  static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

  // Using repr(C) to preserve field ordering and test that everything works
  // when the ChannelBase field is not the first element of the struct.
  #[repr(C)]
  #[derive(Debug)]
  pub struct TestChannel {
    field1: i32,
    field2: u64,
  }

  impl ChannelImpl for TestChannel {
    fn send_response(
      &self,
      call_id: i32,
      mut message: UniquePtr<StringBuffer>,
    ) {
      assert_eq!(call_id, 999);
      assert_eq!(message.as_mut().unwrap().string().len(), MESSAGE.len());
      self.log_call();
    }
    fn send_notification(&self, mut message: UniquePtr<StringBuffer>) {
      assert_eq!(message.as_mut().unwrap().string().len(), MESSAGE.len());
      self.log_call();
    }
    fn flush_protocol_notifications(&self) {
      self.log_call();
    }
  }

  impl TestChannel {
    pub fn new() -> Self {
      Self {
        field1: -42,
        field2: 420,
      }
    }

    fn log_call(&self) {
      assert_eq!(self.field1, -42);
      assert_eq!(self.field2, 420);
      CALL_COUNT.fetch_add(1, SeqCst);
    }
  }

  #[test]
  fn test_channel() {
    let channel = TestChannel::new();
    let msg_view = StringView::from(MESSAGE);
    channel.send_response(999, StringBuffer::create(msg_view));
    assert_eq!(CALL_COUNT.swap(0, SeqCst), 1);
    channel.send_notification(StringBuffer::create(msg_view));
    assert_eq!(CALL_COUNT.swap(0, SeqCst), 1);
    channel.flush_protocol_notifications();
    assert_eq!(CALL_COUNT.swap(0, SeqCst), 1);
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct RawV8InspectorClient {
  _cxx_vtable: CxxVTable,
}

impl V8InspectorClient {
  pub fn run_message_loop_on_pause(&self, context_group_id: i32) {
    unsafe {
      v8_inspector__V8InspectorClient__runMessageLoopOnPause(
        self.raw(),
        context_group_id,
      );
    }
  }

  pub fn quit_message_loop_on_pause(&self) {
    unsafe {
      v8_inspector__V8InspectorClient__quitMessageLoopOnPause(self.raw())
    }
  }

  pub fn run_if_waiting_for_debugger(&self, context_group_id: i32) {
    unsafe {
      v8_inspector__V8InspectorClient__runIfWaitingForDebugger(
        self.raw(),
        context_group_id,
      );
    }
  }

  #[allow(clippy::too_many_arguments)]
  pub fn console_api_message(
    &self,
    context_group_id: i32,
    level: i32,
    message: &StringView,
    url: &StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: &mut V8StackTrace,
  ) {
    unsafe {
      v8_inspector__V8InspectorClient__consoleAPIMessage(
        self.raw(),
        context_group_id,
        level,
        message,
        url,
        line_number,
        column_number,
        stack_trace,
      );
    }
  }

  pub fn generate_unique_id(&self) -> i64 {
    unsafe { v8_inspector__V8InspectorClient__generateUniqueId(self.raw()) }
  }
}

#[allow(unused_variables)]
pub trait V8InspectorClientImpl {
  fn run_message_loop_on_pause(&self, context_group_id: i32) {}
  fn quit_message_loop_on_pause(&self) {}
  fn run_if_waiting_for_debugger(&self, context_group_id: i32) {}

  fn generate_unique_id(&self) -> i64 {
    0 // 0 = let V8 pick a unique id itself
  }

  #[allow(clippy::too_many_arguments)]
  fn console_api_message(
    &self,
    context_group_id: i32,
    level: i32,
    message: &StringView,
    url: &StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: &mut V8StackTrace,
  ) {
  }

  /// Returns a custom Chrome DevTools Protocol `Runtime.RemoteObject` subtype
  /// for `value`. Use one of the protocol's defined subtype enum values.
  /// Returning `Some` causes V8 to call
  /// [`Self::description_for_value_subtype`].
  ///
  /// The callback scope uses the isolate's current context as a best-effort
  /// context; it is not necessarily the context in which `value` originated.
  /// If the isolate has no current context, V8 skips this callback entirely.
  /// This callback runs while the inspector is constructing a value mirror.
  /// Operations such as property access can execute JavaScript through getters
  /// or proxies, so wrap them in a [`crate::TryCatch`] to avoid leaving a
  /// pending exception in the inspector's mirror-building path.
  fn value_subtype<'s>(
    &self,
    scope: &mut PinScope<'s, '_>,
    value: Local<'s, Value>,
  ) -> Option<UniquePtr<StringBuffer>> {
    None
  }

  /// Returns the description for a value whose custom subtype was returned by
  /// [`Self::value_subtype`]. Returning `None` makes V8 fall back to the default
  /// object mirror, which also discards the custom subtype unless it is
  /// `"error"` or `"array"`.
  ///
  /// Like [`Self::value_subtype`], this callback runs while the inspector is
  /// constructing a value mirror. Wrap operations that can execute JavaScript
  /// in a [`crate::TryCatch`] so an exception does not remain pending.
  fn description_for_value_subtype<'s>(
    &self,
    scope: &mut PinScope<'s, '_>,
    value: Local<'s, Value>,
  ) -> Option<UniquePtr<StringBuffer>> {
    None
  }

  fn ensure_default_context_in_group(
    &self,
    context_group_id: i32,
  ) -> Option<Local<'_, Context>> {
    None
  }

  fn resource_name_to_url(
    &self,
    resource_name: &StringView,
  ) -> Option<UniquePtr<StringBuffer>> {
    None
  }
}

// V8 will hold onto a raw pointer to the RawV8InspectorClient, so we need to
// make sure it stays pinned.
#[repr(C)]
struct V8InspectorClientHeap {
  raw: UnsafeCell<RawV8InspectorClient>,
  // this doesn't need to be pinned, but it's convenient to keep it here
  // so we can access it from a pointer to the RawV8InspectorClient
  imp: Box<dyn V8InspectorClientImpl>,
  _pinned: PhantomPinned,
}

impl V8InspectorClientHeap {
  unsafe fn from_raw<'b>(
    this: *const RawV8InspectorClient,
  ) -> &'b V8InspectorClientHeap {
    unsafe { &(*this.cast::<V8InspectorClientHeap>()) }
  }
}

pub struct V8InspectorClient {
  heap: Pin<Box<V8InspectorClientHeap>>,
}

impl V8InspectorClient {
  pub fn new(imp: Box<dyn V8InspectorClientImpl>) -> V8InspectorClient {
    let heap = unsafe {
      let heap =
        Box::into_raw(Box::new(MaybeUninit::<V8InspectorClientHeap>::uninit()));
      let raw = &raw mut (*heap.cast::<V8InspectorClientHeap>()).raw;
      v8_inspector__V8InspectorClient__BASE__CONSTRUCT(
        raw.cast::<MaybeUninit<RawV8InspectorClient>>(),
      );
      let imp_ptr = &raw mut (*heap.cast::<V8InspectorClientHeap>()).imp;
      imp_ptr.write(imp);
      Box::into_pin(Box::from_raw(heap.cast::<V8InspectorClientHeap>()))
    };

    Self { heap }
  }

  fn raw(&self) -> *mut RawV8InspectorClient {
    self.heap.raw.get()
  }
}

impl Debug for V8InspectorClient {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("V8InspectorClient").finish()
  }
}

#[repr(C)]
#[derive(Debug)]
struct RawInspectable(Opaque);

impl Drop for RawInspectable {
  fn drop(&mut self) {
    unsafe {
      v8_inspector__V8InspectorSession__Inspectable__DELETE(self);
    }
  }
}

// A trait object is a fat pointer, so box it once more before passing it through
// the FFI as a thin `*mut c_void`.
struct InspectableData {
  imp: Box<dyn InspectableImpl>,
}

/// Supplies the value of an object added to the inspector's `$0` through `$4`
/// history.
pub trait InspectableImpl {
  /// Called by the inspector from within a V8 callback when the console
  /// dereferences one of `$0` through `$4`.
  ///
  /// There is no way to return an empty handle; if no value is available,
  /// return `undefined`.
  fn get<'s>(
    &self,
    scope: &mut PinScope<'s, '_>,
    context: Local<'s, Context>,
  ) -> Local<'s, Value>;
}

/// An object that can be added to an inspector session's `$0` through `$4`
/// history.
pub struct Inspectable {
  raw: UniqueRef<RawInspectable>,
}

impl Inspectable {
  pub fn new(imp: Box<dyn InspectableImpl>) -> Self {
    let data = Box::into_raw(Box::new(InspectableData { imp })).cast();
    let raw = unsafe {
      UniqueRef::from_raw(v8_inspector__V8InspectorSession__Inspectable__NEW(
        data,
      ))
    };
    Self { raw }
  }
}

impl Debug for Inspectable {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("Inspectable").finish()
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorSession__Inspectable__BASE__get(
  rust_impl: *mut c_void,
  context: Local<Context>,
) -> *const Value {
  let data = unsafe { &*rust_impl.cast::<InspectableData>() };
  // SAFETY: `CallbackScope::new(context)` must not open its own HandleScope.
  // `NewCallbackScope for Local<Context>` has `NEEDS_SCOPE == false`, and
  // `make_new_callback_scope` constructs it with `needs_scope == false`. The
  // handle returned here is allocated in the EscapableHandleScope opened by
  // the C++ shim and escaped there. If Rust opens its own HandleScope, that
  // scope is destroyed before C++ escapes the handle, causing a use-after-free.
  let scope = pin!(unsafe { CallbackScope::new(context) });
  let mut scope = scope.init();
  data.imp.get(&mut scope, context).as_non_null().as_ptr()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorSession__Inspectable__BASE__DROP(
  rust_impl: *mut c_void,
) {
  unsafe {
    drop(Box::from_raw(rust_impl.cast::<InspectableData>()));
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct RawV8InspectorSession(Opaque);

pub struct V8InspectorSession {
  raw: UniqueRef<RawV8InspectorSession>,
  // this isn't actually used, but it needs to live
  // as long as the session
  _channel: Channel,
  isolate: IsolateHandle,
}

impl V8InspectorSession {
  pub fn can_dispatch_method(method: StringView) -> bool {
    unsafe { v8_inspector__V8InspectorSession__canDispatchMethod(method) }
  }

  pub fn dispatch_protocol_message(&self, message: StringView) {
    unsafe {
      v8_inspector__V8InspectorSession__dispatchProtocolMessage(
        self.raw.as_ptr(),
        message,
      );
    }
  }

  pub fn release_object_group(&self, object_group: StringView) {
    unsafe {
      v8_inspector__V8InspectorSession__releaseObjectGroup(
        self.raw.as_ptr(),
        object_group,
      );
    }
  }

  /// Wraps a V8 value in an Inspector `Runtime.RemoteObject`.
  ///
  /// With `generate_preview == false`, V8 uses `kIdOnly`: object results still
  /// include metadata such as `className` and `description`, but no property
  /// preview. With `true`, V8 uses `kPreview`, which additionally includes a
  /// property preview.
  ///
  /// `context` is used only to obtain an execution-context ID and look up the
  /// corresponding `InspectedContext` in this session's context group. The
  /// selected `InjectedScript` wraps `value` in that inspected context's stored
  /// V8 context. Returns `None` if no matching inspected context is registered,
  /// for example before `V8Inspector::context_created`.
  ///
  /// `_scope` is intentionally unused by Rust; borrowing it keeps the caller's
  /// V8 `HandleScope` alive while `ValueMirror::create` allocates local handles.
  pub fn wrap_object<'s>(
    &self,
    _scope: &mut PinScope<'s, '_>,
    context: Local<'s, Context>,
    value: Local<'s, Value>,
    object_group: StringView,
    generate_preview: bool,
  ) -> Option<RemoteObject> {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    unsafe {
      UniqueRef::try_from_raw(v8_inspector__V8InspectorSession__wrapObject(
        self.raw.as_ptr(),
        &*context,
        &*value,
        object_group,
        generate_preview,
      ))
      .map(|raw| RemoteObject { raw })
    }
  }

  /// Resolves an Inspector-generated object ID to its V8 value and context.
  ///
  /// On success, the returned object group contains the group name supplied
  /// when the object was wrapped. An empty group name is returned as a
  /// non-null buffer containing an empty string. On failure, the error buffer
  /// is non-null and contains the Inspector's error message. This includes an
  /// invalid ID or an ID made stale by releasing its object group.
  ///
  /// `_scope` is intentionally unused by Rust; borrowing it keeps the caller's
  /// V8 `HandleScope` alive and ties the returned local handles to that scope.
  #[allow(clippy::type_complexity)]
  pub fn unwrap_object<'s>(
    &self,
    _scope: &mut PinScope<'s, '_>,
    object_id: StringView,
  ) -> Result<
    (
      Local<'s, Value>,
      Local<'s, Context>,
      UniquePtr<StringBuffer>,
    ),
    UniquePtr<StringBuffer>,
  > {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    let mut error = std::ptr::null_mut();
    let mut value = std::ptr::null();
    let mut context = std::ptr::null();
    let mut object_group = std::ptr::null_mut();
    let success = unsafe {
      v8_inspector__V8InspectorSession__unwrapObject(
        self.raw.as_ptr(),
        &mut error,
        object_id,
        &mut value,
        &mut context,
        &mut object_group,
      )
    };

    if !success {
      return Err(unsafe { UniquePtr::from_raw(error) });
    }

    Ok((
      unsafe { Local::from_raw(value).unwrap() },
      unsafe { Local::from_raw(context).unwrap() },
      unsafe { UniquePtr::from_raw(object_group) },
    ))
  }

  pub fn schedule_pause_on_next_statement(
    &self,
    reason: StringView,
    detail: StringView,
  ) {
    unsafe {
      v8_inspector__V8InspectorSession__schedulePauseOnNextStatement(
        self.raw.as_ptr(),
        reason,
        detail,
      );
    }
  }

  /// Cancel a pause previously scheduled by
  /// [`Self::schedule_pause_on_next_statement`] if it hasn't fired yet.
  pub fn cancel_pause_on_next_statement(&self) {
    unsafe {
      v8_inspector__V8InspectorSession__cancelPauseOnNextStatement(
        self.raw.as_ptr(),
      );
    }
  }

  pub fn add_inspected_object(&self, inspectable: Inspectable) {
    unsafe {
      v8_inspector__V8InspectorSession__addInspectedObject(
        self.raw.as_ptr(),
        inspectable.raw.into_raw(),
      );
    }
  }
}

impl Drop for V8InspectorSession {
  fn drop(&mut self) {
    unsafe { v8_inspector__V8InspectorSession__DELETE(self.raw.as_ptr()) };
  }
}

/// An opaque, owned Inspector `Runtime.RemoteObject`.
pub struct RemoteObject {
  raw: UniqueRef<RawRemoteObject>,
}

#[repr(C)]
#[derive(Debug)]
struct RawRemoteObject(Opaque);

impl RemoteObject {
  /// Serializes this remote object to its CRDTP/CBOR representation.
  pub fn to_bytes(&self) -> Vec<u8> {
    unsafe {
      CppVecU8::take_from_raw(v8_inspector__RemoteObject__toBytes(
        self.raw.as_ptr(),
      ))
    }
  }
}

impl Debug for RemoteObject {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("RemoteObject").finish()
  }
}

impl Drop for RawRemoteObject {
  fn drop(&mut self) {
    unsafe { v8_inspector__RemoteObject__DELETE(self) }
  }
}

// TODO: in C++, this class is intended to be user-extensible, just like
// like `Task`, `Client`, `Channel`. In Rust this would ideally also be the
// case, but currently to obtain a `UniquePtr<StringBuffer>` is by making a
// copy using `StringBuffer::create()`.
#[repr(C)]
#[derive(Debug)]
pub struct StringBuffer {
  _cxx_vtable: CxxVTable,
}

// TODO: make it possible to obtain a `UniquePtr<StringBuffer>` directly from
// an owned `Vec<u8>` or `Vec<u16>`,
impl StringBuffer {
  // The C++ class definition does not declare `string()` to be a const method,
  // therefore we declare self as mutable here.
  // TODO: figure out whether it'd be safe to assume a const receiver here.
  // That would make it possible to implement `Deref<Target = StringBuffer>`.
  pub fn string(&self) -> StringView<'_> {
    unsafe { v8_inspector__StringBuffer__string(self) }
  }

  /// This method copies contents.
  pub fn create(source: StringView) -> UniquePtr<StringBuffer> {
    unsafe { v8_inspector__StringBuffer__create(source) }
  }
}

impl Drop for StringBuffer {
  fn drop(&mut self) {
    unsafe { v8_inspector__StringBuffer__DELETE(self) }
  }
}

unsafe impl Send for StringBuffer {}
use std::iter::ExactSizeIterator;
use std::iter::IntoIterator;
use std::marker::PhantomData;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::pin::Pin;
use std::ptr::NonNull;
use std::ptr::null;
use std::slice;
use std::string;

// Notes:
//  * This class is ported, not wrapped using bindings.
//  * Since Rust `repr(bool)` is not allowed, we're assuming that `bool` and
//    `u8` have the same size. This is assumption is checked in 'support.h'.
//    TODO: find/open upstream issue to allow #[repr(bool)] support.

#[derive(Clone, Debug, Copy)]
#[repr(u8)]
pub enum StringView<'a> {
  // Do not reorder!
  U16(CharacterArray<'a, u16>),
  U8(CharacterArray<'a, u8>),
}

impl StringView<'static> {
  pub fn empty() -> Self {
    Self::U8(CharacterArray::<'static, u8>::empty())
  }
}

impl fmt::Display for StringView<'_> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Self::U16(v) => write!(f, "{v}"),
      Self::U8(v) => write!(f, "{v}"),
    }
  }
}

impl<'a> From<&'a [u8]> for StringView<'a> {
  fn from(v: &'a [u8]) -> Self {
    Self::U8(CharacterArray::<'a, u8>::from(v))
  }
}

impl<'a> From<&'a [u16]> for StringView<'a> {
  fn from(v: &'a [u16]) -> Self {
    Self::U16(CharacterArray::<'a, u16>::from(v))
  }
}

impl StringView<'_> {
  pub fn is_8bit(&self) -> bool {
    match self {
      Self::U16(..) => false,
      Self::U8(..) => true,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  pub fn len(&self) -> usize {
    match self {
      Self::U16(v) => v.len(),
      Self::U8(v) => v.len(),
    }
  }

  pub fn characters8(&self) -> Option<&[u8]> {
    match self {
      Self::U16(..) => None,
      Self::U8(v) => Some(v),
    }
  }

  pub fn characters16(&self) -> Option<&[u16]> {
    match self {
      Self::U16(v) => Some(v),
      Self::U8(..) => None,
    }
  }
}

impl<'a> IntoIterator for StringView<'a> {
  type IntoIter = StringViewIterator<'a>;
  type Item = u16;

  fn into_iter(self) -> Self::IntoIter {
    StringViewIterator { view: self, pos: 0 }
  }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CharacterArray<'a, T> {
  m_length: usize,
  m_characters: *const T,
  _phantom: PhantomData<&'a T>,
}

impl CharacterArray<'static, u8> {
  pub fn empty() -> Self {
    Self {
      m_length: 0,
      m_characters: null(),
      _phantom: PhantomData,
    }
  }
}

impl<T> CharacterArray<'_, T>
where
  T: Copy,
{
  #[inline(always)]
  fn len(&self) -> usize {
    self.m_length
  }

  #[inline(always)]
  fn get_at(&self, index: usize) -> Option<T> {
    if index < self.m_length {
      Some(unsafe { *self.m_characters.add(index) })
    } else {
      None
    }
  }
}

unsafe impl<T> Send for CharacterArray<'_, T> where T: Copy {}
unsafe impl<T> Sync for CharacterArray<'_, T> where T: Sync {}

impl fmt::Display for CharacterArray<'_, u8> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(
      self
        .iter()
        .cloned()
        .map(char::from)
        .collect::<string::String>()
        .as_str(),
    )
  }
}

impl fmt::Display for CharacterArray<'_, u16> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str(&string::String::from_utf16_lossy(self))
  }
}

impl<'a, T> From<&'a [T]> for CharacterArray<'a, T> {
  fn from(v: &'a [T]) -> Self {
    Self {
      m_length: v.len(),
      m_characters: v.as_ptr(),
      _phantom: PhantomData,
    }
  }
}

impl<T> Deref for CharacterArray<'_, T> {
  type Target = [T];

  fn deref(&self) -> &[T] {
    let Self {
      m_length,
      mut m_characters,
      ..
    } = *self;
    if m_characters.is_null() {
      assert_eq!(m_length, 0);
      m_characters = NonNull::dangling().as_ptr();
    };
    unsafe { slice::from_raw_parts(m_characters, m_length) }
  }
}

#[derive(Copy, Clone, Debug)]
pub struct StringViewIterator<'a> {
  view: StringView<'a>,
  pos: usize,
}

impl Iterator for StringViewIterator<'_> {
  type Item = u16;

  fn next(&mut self) -> Option<Self::Item> {
    let result = Some(match self.view {
      StringView::U16(v) => v.get_at(self.pos)?,
      StringView::U8(v) => u16::from(v.get_at(self.pos)?),
    });
    self.pos += 1;
    result
  }
}

impl ExactSizeIterator for StringViewIterator<'_> {
  fn len(&self) -> usize {
    self.view.len()
  }
}

#[test]
fn string_view_display() {
  let ok: [u16; 2] = [111, 107];
  assert_eq!("ok", format!("{}", StringView::from(&ok[..])));
  assert_eq!("ok", format!("{}", StringView::from(&b"ok"[..])));
  assert_eq!("ØÞ", format!("{}", StringView::from(&[216u8, 222u8][..])));
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
#[repr(C)]
pub enum V8InspectorClientTrustLevel {
  Untrusted = 0,
  FullyTrusted = 1,
}

#[repr(C)]
#[derive(Debug)]
pub struct RawV8Inspector(Opaque);

pub struct V8Inspector {
  raw: UniqueRef<RawV8Inspector>,
  _client: V8InspectorClient,
  isolate: IsolateHandle,
}

impl V8Inspector {
  pub fn create(
    isolate: &mut Isolate,
    client: V8InspectorClient,
  ) -> V8Inspector {
    let raw = unsafe {
      UniqueRef::from_raw(v8_inspector__V8Inspector__create(
        isolate.as_real_ptr(),
        client.raw(),
      ))
    };
    V8Inspector {
      raw,
      _client: client,
      isolate: isolate.thread_safe_handle(),
    }
  }

  // note: in theory v8 could mutate through this pointer.
  // this is fine, though, because we never create a rust reference
  // to the actual RawV8Inspector, we only use raw pointers which
  // don't enforce the immutability guarantee
  fn raw(&self) -> *mut RawV8Inspector {
    self.raw.as_ptr()
  }

  pub fn connect(
    &self,
    context_group_id: i32,
    channel: Channel,
    state: StringView,
    client_trust_level: V8InspectorClientTrustLevel,
  ) -> V8InspectorSession {
    let raw = unsafe {
      UniqueRef::from_raw(v8_inspector__V8Inspector__connect(
        self.raw(),
        context_group_id,
        channel.raw(),
        state,
        client_trust_level,
      ))
    };

    V8InspectorSession {
      raw,
      _channel: channel,
      isolate: self.isolate.clone(),
    }
  }

  /// Note: this method deviates from the C++ API here because it's a lot of
  /// work to bind the V8ContextInfo, which is not used elsewhere.
  pub fn context_created(
    &self,
    context: Local<Context>,
    context_group_id: i32,
    human_readable_name: StringView,
    aux_data: StringView,
  ) {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    unsafe {
      v8_inspector__V8Inspector__contextCreated(
        self.raw(),
        &*context,
        context_group_id,
        human_readable_name,
        aux_data,
      );
    }
  }

  pub fn context_destroyed(&self, context: Local<Context>) {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    unsafe {
      v8_inspector__V8Inspector__contextDestroyed(self.raw(), &*context)
    }
  }

  /// Tell the inspector the runtime entered an idle period. Pairs with
  /// [`Self::idle_finished`].
  pub fn idle_started(&self) {
    unsafe { v8_inspector__V8Inspector__idleStarted(self.raw()) }
  }

  /// Tell the inspector the runtime left an idle period.
  pub fn idle_finished(&self) {
    unsafe { v8_inspector__V8Inspector__idleFinished(self.raw()) }
  }

  /// Notify the inspector that an async task has been scheduled — used to
  /// build async stack traces. `task` is an opaque identity pointer that
  /// must match the one later passed to
  /// [`Self::async_task_started`]/[`Self::async_task_finished`]/
  /// [`Self::async_task_canceled`]. Set `recurring` to `true` for
  /// repeating tasks such as `setInterval` where the same identity fires
  /// multiple times.
  ///
  /// # Safety
  /// `task` must be a stable pointer for the lifetime of the scheduled
  /// async task; the inspector stores it as an opaque key.
  pub unsafe fn async_task_scheduled(
    &self,
    task_name: StringView,
    task: *const c_void,
    recurring: bool,
  ) {
    unsafe {
      v8_inspector__V8Inspector__asyncTaskScheduled(
        self.raw(),
        task_name,
        task,
        recurring,
      )
    }
  }

  /// Notify the inspector that a previously scheduled async task was
  /// cancelled and will not run. `task` must match the pointer used in
  /// the corresponding [`Self::async_task_scheduled`] call.
  ///
  /// # Safety
  /// See [`Self::async_task_scheduled`].
  pub unsafe fn async_task_canceled(&self, task: *const c_void) {
    unsafe { v8_inspector__V8Inspector__asyncTaskCanceled(self.raw(), task) }
  }

  /// Notify the inspector that an async task has begun executing. Must be
  /// paired with [`Self::async_task_finished`] before the JS callback
  /// returns to V8.
  ///
  /// # Safety
  /// See [`Self::async_task_scheduled`].
  pub unsafe fn async_task_started(&self, task: *const c_void) {
    unsafe { v8_inspector__V8Inspector__asyncTaskStarted(self.raw(), task) }
  }

  /// Notify the inspector that an async task has finished executing.
  ///
  /// # Safety
  /// See [`Self::async_task_scheduled`].
  pub unsafe fn async_task_finished(&self, task: *const c_void) {
    unsafe { v8_inspector__V8Inspector__asyncTaskFinished(self.raw(), task) }
  }

  /// Notify the inspector that every outstanding async task is being
  /// cancelled, e.g. during runtime shutdown.
  pub fn all_async_tasks_canceled(&self) {
    unsafe { v8_inspector__V8Inspector__allAsyncTasksCanceled(self.raw()) }
  }

  #[allow(clippy::too_many_arguments)]
  pub fn exception_thrown(
    &self,
    context: Local<Context>,
    message: StringView,
    exception: Local<Value>,
    detailed_message: StringView,
    url: StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: UniquePtr<V8StackTrace>,
    script_id: i32,
  ) -> u32 {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    unsafe {
      v8_inspector__V8Inspector__exceptionThrown(
        self.raw(),
        &*context,
        message,
        &*exception,
        detailed_message,
        url,
        line_number,
        column_number,
        stack_trace.into_raw(),
        script_id,
      )
    }
  }

  pub fn create_stack_trace(
    &self,
    stack_trace: Option<Local<StackTrace>>,
  ) -> UniquePtr<V8StackTrace> {
    crate::handle::assert_locals_in_isolate_of(&self.isolate);
    unsafe {
      UniquePtr::from_raw(v8_inspector__V8Inspector__createStackTrace(
        self.raw(),
        stack_trace.map_or(null(), |v| &*v),
      ))
    }
  }
}

impl Drop for V8Inspector {
  fn drop(&mut self) {
    unsafe { v8_inspector__V8Inspector__DELETE(self.raw()) };
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct V8StackTrace {
  _cxx_vtable: CxxVTable,
}

impl Drop for V8StackTrace {
  fn drop(&mut self) {
    unsafe { v8_inspector__V8StackTrace__DELETE(self) };
  }
}

// TODO(bnoordhuis) This needs to be fleshed out more but that can wait
// until it's actually needed.
