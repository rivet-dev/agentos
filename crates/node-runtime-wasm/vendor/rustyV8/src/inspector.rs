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

use crate::Context;
use crate::Isolate;
use crate::Local;
use crate::StackTrace;
use crate::Value;
use crate::support::CxxVTable;
use crate::support::FieldOffset;
use crate::support::Opaque;
use crate::support::RustVTable;
use crate::support::UniquePtr;
use crate::support::UniqueRef;
use crate::support::int;
use std::fmt::{self, Debug, Formatter};

unsafe extern "C" {
  fn v8_inspector__V8Inspector__Channel__BASE__CONSTRUCT(
    buf: &mut std::mem::MaybeUninit<Channel>,
  );

  fn v8_inspector__V8Inspector__Channel__sendResponse(
    this: &mut Channel,
    call_id: int,
    message: UniquePtr<StringBuffer>,
  );
  fn v8_inspector__V8Inspector__Channel__sendNotification(
    this: &mut Channel,
    message: UniquePtr<StringBuffer>,
  );
  fn v8_inspector__V8Inspector__Channel__flushProtocolNotifications(
    this: &mut Channel,
  );

  fn v8_inspector__V8InspectorClient__BASE__CONSTRUCT(
    buf: &mut std::mem::MaybeUninit<V8InspectorClient>,
  );

  fn v8_inspector__V8InspectorClient__generateUniqueId(
    this: &mut V8InspectorClient,
  ) -> i64;
  fn v8_inspector__V8InspectorClient__runMessageLoopOnPause(
    this: &mut V8InspectorClient,
    context_group_id: int,
  );
  fn v8_inspector__V8InspectorClient__quitMessageLoopOnPause(
    this: &mut V8InspectorClient,
  );
  fn v8_inspector__V8InspectorClient__runIfWaitingForDebugger(
    this: &mut V8InspectorClient,
    context_group_id: int,
  );
  fn v8_inspector__V8InspectorClient__consoleAPIMessage(
    this: &mut V8InspectorClient,
    context_group_id: int,
    level: int,
    message: &StringView,
    url: &StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: &mut V8StackTrace,
  );

  fn v8_inspector__V8InspectorSession__DELETE(this: *mut V8InspectorSession);
  fn v8_inspector__V8InspectorSession__dispatchProtocolMessage(
    session: *mut V8InspectorSession,
    message: StringView,
  );
  fn v8_inspector__V8InspectorSession__schedulePauseOnNextStatement(
    session: *mut V8InspectorSession,
    break_reason: StringView,
    break_details: StringView,
  );
  fn v8_inspector__V8InspectorSession__canDispatchMethod(
    method: StringView,
  ) -> bool;

  fn v8_inspector__StringBuffer__DELETE(this: *mut StringBuffer);
  fn v8_inspector__StringBuffer__string(this: &StringBuffer) -> StringView;
  fn v8_inspector__StringBuffer__create(
    source: StringView,
  ) -> UniquePtr<StringBuffer>;

  fn v8_inspector__V8Inspector__DELETE(this: *mut V8Inspector);
  fn v8_inspector__V8Inspector__create(
    isolate: *mut Isolate,
    client: *mut V8InspectorClient,
  ) -> *mut V8Inspector;
  fn v8_inspector__V8Inspector__connect(
    inspector: *mut V8Inspector,
    context_group_id: int,
    channel: *mut Channel,
    state: StringView,
    client_trust_level: V8InspectorClientTrustLevel,
  ) -> *mut V8InspectorSession;
  fn v8_inspector__V8Inspector__contextCreated(
    this: *mut V8Inspector,
    context: *const Context,
    contextGroupId: int,
    humanReadableName: StringView,
    auxData: StringView,
  );
  fn v8_inspector__V8Inspector__contextDestroyed(
    this: *mut V8Inspector,
    context: *const Context,
  );
  fn v8_inspector__V8Inspector__exceptionThrown(
    this: *mut V8Inspector,
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
    this: *mut V8Inspector,
    stack_trace: *const StackTrace,
  ) -> *mut V8StackTrace;
  fn v8_inspector__V8StackTrace__DELETE(this: *mut V8StackTrace);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__sendResponse(
  this: &mut Channel,
  call_id: int,
  message: UniquePtr<StringBuffer>,
) {
  unsafe {
    ChannelBase::dispatch_mut(this).send_response(call_id, message);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__sendNotification(
  this: &mut Channel,
  message: UniquePtr<StringBuffer>,
) {
  unsafe {
    ChannelBase::dispatch_mut(this).send_notification(message);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8Inspector__Channel__BASE__flushProtocolNotifications(
  this: &mut Channel,
) {
  unsafe {
    ChannelBase::dispatch_mut(this).flush_protocol_notifications();
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__generateUniqueId(
  this: &mut V8InspectorClient,
) -> i64 {
  unsafe { V8InspectorClientBase::dispatch_mut(this).generate_unique_id() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__runMessageLoopOnPause(
  this: &mut V8InspectorClient,
  context_group_id: int,
) {
  unsafe {
    V8InspectorClientBase::dispatch_mut(this)
      .run_message_loop_on_pause(context_group_id);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__quitMessageLoopOnPause(
  this: &mut V8InspectorClient,
) {
  unsafe {
    V8InspectorClientBase::dispatch_mut(this).quit_message_loop_on_pause();
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__runIfWaitingForDebugger(
  this: &mut V8InspectorClient,
  context_group_id: int,
) {
  unsafe {
    V8InspectorClientBase::dispatch_mut(this)
      .run_if_waiting_for_debugger(context_group_id);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__consoleAPIMessage(
  this: &mut V8InspectorClient,
  context_group_id: int,
  level: int,
  message: &StringView,
  url: &StringView,
  line_number: u32,
  column_number: u32,
  stack_trace: &mut V8StackTrace,
) {
  unsafe {
    V8InspectorClientBase::dispatch_mut(this).console_api_message(
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
unsafe extern "C" fn v8_inspector__V8InspectorClient__BASE__ensureDefaultContextInGroup(
  this: &mut V8InspectorClient,
  context_group_id: int,
) -> *const Context {
  unsafe {
    match V8InspectorClientBase::dispatch_mut(this)
      .ensure_default_context_in_group(context_group_id)
    {
      Some(h) => &*h,
      None => std::ptr::null_mut(),
    }
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct Channel {
  _cxx_vtable: CxxVTable,
}

impl Channel {
  pub fn send_response(
    &mut self,
    call_id: i32,
    message: UniquePtr<StringBuffer>,
  ) {
    unsafe {
      v8_inspector__V8Inspector__Channel__sendResponse(self, call_id, message);
    }
  }
  pub fn send_notification(&mut self, message: UniquePtr<StringBuffer>) {
    unsafe {
      v8_inspector__V8Inspector__Channel__sendNotification(self, message);
    }
  }
  pub fn flush_protocol_notifications(&mut self) {
    unsafe {
      v8_inspector__V8Inspector__Channel__flushProtocolNotifications(self);
    }
  }
}

pub trait AsChannel {
  fn as_channel(&self) -> &Channel;
  fn as_channel_mut(&mut self) -> &mut Channel;
}

impl AsChannel for Channel {
  fn as_channel(&self) -> &Channel {
    self
  }
  fn as_channel_mut(&mut self) -> &mut Channel {
    self
  }
}

impl<T> AsChannel for T
where
  T: ChannelImpl,
{
  fn as_channel(&self) -> &Channel {
    &self.base().cxx_base
  }
  fn as_channel_mut(&mut self) -> &mut Channel {
    &mut self.base_mut().cxx_base
  }
}

pub trait ChannelImpl: AsChannel {
  fn base(&self) -> &ChannelBase;
  fn base_mut(&mut self) -> &mut ChannelBase;
  /// This is used for calculating the offset to the base field, and care must be taken not to create any references in the process of creating the pointer because the *const Self pointer is not valid (thus resulting in instant UB)
  unsafe fn base_ptr(this: *const Self) -> *const ChannelBase
  where
    Self: Sized;

  fn send_response(&mut self, call_id: i32, message: UniquePtr<StringBuffer>);
  fn send_notification(&mut self, message: UniquePtr<StringBuffer>);
  fn flush_protocol_notifications(&mut self);
}

pub struct ChannelBase {
  cxx_base: Channel,
  offset_within_embedder: FieldOffset<Self>,
  rust_vtable: RustVTable<&'static dyn ChannelImpl>,
}

impl ChannelBase {
  fn construct_cxx_base() -> Channel {
    unsafe {
      let mut buf = std::mem::MaybeUninit::<Channel>::uninit();
      v8_inspector__V8Inspector__Channel__BASE__CONSTRUCT(&mut buf);
      buf.assume_init()
    }
  }

  fn get_cxx_base_offset() -> FieldOffset<Channel> {
    let buf = std::mem::MaybeUninit::<Self>::uninit();
    let base = unsafe { addr_of!((*buf.as_ptr()).cxx_base) };
    FieldOffset::from_ptrs(buf.as_ptr(), base)
  }

  fn get_offset_within_embedder<T>() -> FieldOffset<Self>
  where
    T: ChannelImpl,
  {
    let buf = std::mem::MaybeUninit::<T>::uninit();
    let embedder_ptr: *const T = buf.as_ptr();
    let self_ptr: *const Self = unsafe { T::base_ptr(embedder_ptr) };
    FieldOffset::from_ptrs(embedder_ptr, self_ptr)
  }

  fn get_rust_vtable<T>() -> RustVTable<&'static dyn ChannelImpl>
  where
    T: ChannelImpl,
  {
    let buf = std::mem::MaybeUninit::<T>::uninit();
    let embedder_ptr = buf.as_ptr();
    let trait_object: *const dyn ChannelImpl = embedder_ptr;
    let (data_ptr, vtable): (*const T, RustVTable<_>) =
      unsafe { std::mem::transmute(trait_object) };
    assert_eq!(data_ptr, embedder_ptr);
    vtable
  }

  pub fn new<T>() -> Self
  where
    T: ChannelImpl,
  {
    Self {
      cxx_base: Self::construct_cxx_base(),
      offset_within_embedder: Self::get_offset_within_embedder::<T>(),
      rust_vtable: Self::get_rust_vtable::<T>(),
    }
  }

  pub unsafe fn dispatch(channel: &Channel) -> &dyn ChannelImpl {
    unsafe {
      let this = Self::get_cxx_base_offset().to_embedder::<Self>(channel);
      let embedder = this.offset_within_embedder.to_embedder::<Opaque>(this);
      std::mem::transmute((embedder, this.rust_vtable))
    }
  }

  pub unsafe fn dispatch_mut(channel: &mut Channel) -> &mut dyn ChannelImpl {
    unsafe {
      let this = Self::get_cxx_base_offset().to_embedder_mut::<Self>(channel);
      let vtable = this.rust_vtable;
      let embedder =
        this.offset_within_embedder.to_embedder_mut::<Opaque>(this);
      std::mem::transmute((embedder, vtable))
    }
  }
}

impl Debug for ChannelBase {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("ChannelBase")
      .field("cxx_base", &self.cxx_base)
      .field("offset_within_embedder", &self.offset_within_embedder)
      .finish()
  }
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
    base: ChannelBase,
    field2: u64,
  }

  impl ChannelImpl for TestChannel {
    fn base(&self) -> &ChannelBase {
      &self.base
    }
    fn base_mut(&mut self) -> &mut ChannelBase {
      &mut self.base
    }
    unsafe fn base_ptr(_this: *const Self) -> *const ChannelBase
    where
      Self: Sized,
    {
      unsafe { addr_of!((*_this).base) }
    }
    fn send_response(
      &mut self,
      call_id: i32,
      mut message: UniquePtr<StringBuffer>,
    ) {
      assert_eq!(call_id, 999);
      assert_eq!(message.as_mut().unwrap().string().len(), MESSAGE.len());
      self.log_call();
    }
    fn send_notification(&mut self, mut message: UniquePtr<StringBuffer>) {
      assert_eq!(message.as_mut().unwrap().string().len(), MESSAGE.len());
      self.log_call();
    }
    fn flush_protocol_notifications(&mut self) {
      self.log_call();
    }
  }

  impl TestChannel {
    pub fn new() -> Self {
      Self {
        base: ChannelBase::new::<Self>(),
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
    let mut channel = TestChannel::new();
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
pub struct V8InspectorClient {
  _cxx_vtable: CxxVTable,
}

impl V8InspectorClient {
  pub fn run_message_loop_on_pause(&mut self, context_group_id: i32) {
    unsafe {
      v8_inspector__V8InspectorClient__runMessageLoopOnPause(
        self,
        context_group_id,
      );
    }
  }

  pub fn quit_message_loop_on_pause(&mut self) {
    unsafe { v8_inspector__V8InspectorClient__quitMessageLoopOnPause(self) }
  }

  pub fn run_if_waiting_for_debugger(&mut self, context_group_id: i32) {
    unsafe {
      v8_inspector__V8InspectorClient__runIfWaitingForDebugger(
        self,
        context_group_id,
      );
    }
  }

  #[allow(clippy::too_many_arguments)]
  pub fn console_api_message(
    &mut self,
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
        self,
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

  pub fn generate_unique_id(&mut self) -> i64 {
    unsafe { v8_inspector__V8InspectorClient__generateUniqueId(self) }
  }
}

pub trait AsV8InspectorClient {
  fn as_client(&self) -> &V8InspectorClient;
  fn as_client_mut(&mut self) -> &mut V8InspectorClient;
}

impl AsV8InspectorClient for V8InspectorClient {
  fn as_client(&self) -> &V8InspectorClient {
    self
  }
  fn as_client_mut(&mut self) -> &mut V8InspectorClient {
    self
  }
}

impl<T> AsV8InspectorClient for T
where
  T: V8InspectorClientImpl,
{
  fn as_client(&self) -> &V8InspectorClient {
    &self.base().cxx_base
  }
  fn as_client_mut(&mut self) -> &mut V8InspectorClient {
    &mut self.base_mut().cxx_base
  }
}

#[allow(unused_variables)]
pub trait V8InspectorClientImpl: AsV8InspectorClient {
  fn base(&self) -> &V8InspectorClientBase;
  fn base_mut(&mut self) -> &mut V8InspectorClientBase;
  /// This is used for calculating the offset to the base field, and care must be taken not to create any references in the process of creating the pointer because the *const Self pointer is not valid (thus resulting in instant UB)
  unsafe fn base_ptr(this: *const Self) -> *const V8InspectorClientBase
  where
    Self: Sized;

  fn run_message_loop_on_pause(&mut self, context_group_id: i32) {}
  fn quit_message_loop_on_pause(&mut self) {}
  fn run_if_waiting_for_debugger(&mut self, context_group_id: i32) {}

  fn generate_unique_id(&mut self) -> i64 {
    0 // 0 = let V8 pick a unique id itself
  }

  #[allow(clippy::too_many_arguments)]
  fn console_api_message(
    &mut self,
    context_group_id: i32,
    level: i32,
    message: &StringView,
    url: &StringView,
    line_number: u32,
    column_number: u32,
    stack_trace: &mut V8StackTrace,
  ) {
  }

  fn ensure_default_context_in_group(
    &mut self,
    context_group_id: i32,
  ) -> Option<Local<Context>> {
    None
  }
}

pub struct V8InspectorClientBase {
  cxx_base: V8InspectorClient,
  offset_within_embedder: FieldOffset<Self>,
  rust_vtable: RustVTable<&'static dyn V8InspectorClientImpl>,
}

impl V8InspectorClientBase {
  fn construct_cxx_base() -> V8InspectorClient {
    unsafe {
      let mut buf = std::mem::MaybeUninit::<V8InspectorClient>::uninit();
      v8_inspector__V8InspectorClient__BASE__CONSTRUCT(&mut buf);
      buf.assume_init()
    }
  }

  fn get_cxx_base_offset() -> FieldOffset<V8InspectorClient> {
    let buf = std::mem::MaybeUninit::<Self>::uninit();
    let base = unsafe { addr_of!((*buf.as_ptr()).cxx_base) };
    FieldOffset::from_ptrs(buf.as_ptr(), base)
  }

  fn get_offset_within_embedder<T>() -> FieldOffset<Self>
  where
    T: V8InspectorClientImpl,
  {
    let buf = std::mem::MaybeUninit::<T>::uninit();
    let embedder_ptr: *const T = buf.as_ptr();
    let self_ptr: *const Self = unsafe { T::base_ptr(embedder_ptr) };
    FieldOffset::from_ptrs(embedder_ptr, self_ptr)
  }

  fn get_rust_vtable<T>() -> RustVTable<&'static dyn V8InspectorClientImpl>
  where
    T: V8InspectorClientImpl,
  {
    let buf = std::mem::MaybeUninit::<T>::uninit();
    let embedder_ptr = buf.as_ptr();
    let trait_object: *const dyn V8InspectorClientImpl = embedder_ptr;
    let (data_ptr, vtable): (*const T, RustVTable<_>) =
      unsafe { std::mem::transmute(trait_object) };
    assert_eq!(data_ptr, embedder_ptr);
    vtable
  }

  pub fn new<T>() -> Self
  where
    T: V8InspectorClientImpl,
  {
    Self {
      cxx_base: Self::construct_cxx_base(),
      offset_within_embedder: Self::get_offset_within_embedder::<T>(),
      rust_vtable: Self::get_rust_vtable::<T>(),
    }
  }

  pub unsafe fn dispatch(
    client: &V8InspectorClient,
  ) -> &dyn V8InspectorClientImpl {
    unsafe {
      let this = Self::get_cxx_base_offset().to_embedder::<Self>(client);
      let embedder = this.offset_within_embedder.to_embedder::<Opaque>(this);
      std::mem::transmute((embedder, this.rust_vtable))
    }
  }

  pub unsafe fn dispatch_mut(
    client: &mut V8InspectorClient,
  ) -> &mut dyn V8InspectorClientImpl {
    unsafe {
      let this = Self::get_cxx_base_offset().to_embedder_mut::<Self>(client);
      let vtable = this.rust_vtable;
      let embedder =
        this.offset_within_embedder.to_embedder_mut::<Opaque>(this);
      std::mem::transmute((embedder, vtable))
    }
  }
}

impl Debug for V8InspectorClientBase {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.debug_struct("V8InspectorClientBase")
      .field("cxx_base", &self.cxx_base)
      .field("offset_within_embedder", &self.offset_within_embedder)
      .finish()
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct V8InspectorSession(Opaque);

impl V8InspectorSession {
  pub fn can_dispatch_method(method: StringView) -> bool {
    unsafe { v8_inspector__V8InspectorSession__canDispatchMethod(method) }
  }

  pub fn dispatch_protocol_message(&mut self, message: StringView) {
    unsafe {
      v8_inspector__V8InspectorSession__dispatchProtocolMessage(self, message);
    }
  }

  pub fn schedule_pause_on_next_statement(
    &mut self,
    reason: StringView,
    detail: StringView,
  ) {
    unsafe {
      v8_inspector__V8InspectorSession__schedulePauseOnNextStatement(
        self, reason, detail,
      );
    }
  }
}

impl Drop for V8InspectorSession {
  fn drop(&mut self) {
    unsafe { v8_inspector__V8InspectorSession__DELETE(self) };
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
  pub fn string(&self) -> StringView {
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
use std::ops::Deref;
use std::ptr::NonNull;
use std::ptr::addr_of;
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
pub struct V8Inspector(Opaque);

impl V8Inspector {
  pub fn create<T>(
    isolate: &mut Isolate,
    client: &mut T,
  ) -> UniqueRef<V8Inspector>
  where
    T: AsV8InspectorClient,
  {
    unsafe {
      UniqueRef::from_raw(v8_inspector__V8Inspector__create(
        isolate,
        client.as_client_mut(),
      ))
    }
  }

  pub fn connect<T>(
    &mut self,
    context_group_id: i32,
    channel: &mut T,
    state: StringView,
    client_trust_level: V8InspectorClientTrustLevel,
  ) -> UniqueRef<V8InspectorSession>
  where
    T: AsChannel,
  {
    unsafe {
      UniqueRef::from_raw(v8_inspector__V8Inspector__connect(
        self,
        context_group_id,
        channel.as_channel_mut(),
        state,
        client_trust_level,
      ))
    }
  }

  /// Note: this method deviates from the C++ API here because it's a lot of
  /// work to bind the V8ContextInfo, which is not used elsewhere.
  pub fn context_created(
    &mut self,
    context: Local<Context>,
    context_group_id: i32,
    human_readable_name: StringView,
    aux_data: StringView,
  ) {
    unsafe {
      v8_inspector__V8Inspector__contextCreated(
        self,
        &*context,
        context_group_id,
        human_readable_name,
        aux_data,
      );
    }
  }

  pub fn context_destroyed(&mut self, context: Local<Context>) {
    unsafe { v8_inspector__V8Inspector__contextDestroyed(self, &*context) }
  }

  #[allow(clippy::too_many_arguments)]
  pub fn exception_thrown(
    &mut self,
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
    unsafe {
      v8_inspector__V8Inspector__exceptionThrown(
        self,
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
    &mut self,
    stack_trace: Option<Local<StackTrace>>,
  ) -> UniquePtr<V8StackTrace> {
    unsafe {
      UniquePtr::from_raw(v8_inspector__V8Inspector__createStackTrace(
        self,
        stack_trace.map_or(null(), |v| &*v),
      ))
    }
  }
}

impl Drop for V8Inspector {
  fn drop(&mut self) {
    unsafe { v8_inspector__V8Inspector__DELETE(self) };
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
