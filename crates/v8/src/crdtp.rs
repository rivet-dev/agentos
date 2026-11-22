// Copyright 2024 the Deno authors. All rights reserved. MIT license.

use crate::support::CxxVTable;
use crate::support::Opaque;
use std::cell::UnsafeCell;
use std::ffi::CString;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::ptr::NonNull;

unsafe extern "C" {
  fn crdtp__FrontendChannel__BASE__CONSTRUCT(
    buf: *mut MaybeUninit<RawFrontendChannel>,
  );

  fn crdtp__Serializable__DELETE(this: *mut RawSerializable);
  fn crdtp__Serializable__AppendSerialized(
    this: *const RawSerializable,
    out: *mut CppVecU8,
  );

  fn crdtp__Dispatchable__new(
    data: *const u8,
    len: usize,
    associated_data: *const u8,
    associated_data_len: usize,
    fallthrough_callback: *mut c_void,
  ) -> *mut Dispatchable;
  fn crdtp__Dispatchable__DELETE(this: *mut Dispatchable);
  fn crdtp__Dispatchable__ok(this: *const Dispatchable) -> bool;
  fn crdtp__Dispatchable__callId(this: *const Dispatchable) -> i32;
  fn crdtp__Dispatchable__hasCallId(this: *const Dispatchable) -> bool;
  fn crdtp__Dispatchable__methodLen(this: *const Dispatchable) -> usize;
  fn crdtp__Dispatchable__methodCopy(this: *const Dispatchable, out: *mut u8);
  fn crdtp__Dispatchable__sessionIdLen(this: *const Dispatchable) -> usize;
  fn crdtp__Dispatchable__sessionIdCopy(
    this: *const Dispatchable,
    out: *mut u8,
  );
  fn crdtp__Dispatchable__paramsLen(this: *const Dispatchable) -> usize;
  fn crdtp__Dispatchable__paramsCopy(this: *const Dispatchable, out: *mut u8);
  fn crdtp__Dispatchable__associatedDataLen(this: *const Dispatchable)
  -> usize;
  fn crdtp__Dispatchable__associatedDataCopy(
    this: *const Dispatchable,
    out: *mut u8,
  );

  fn crdtp__DispatchResponse__Success() -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__FallThrough() -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__ParseError(
    msg: *const u8,
    len: usize,
  ) -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__InvalidRequest(
    msg: *const u8,
    len: usize,
  ) -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__MethodNotFound(
    msg: *const u8,
    len: usize,
  ) -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__InvalidParams(
    msg: *const u8,
    len: usize,
  ) -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__ServerError(
    msg: *const u8,
    len: usize,
  ) -> *mut DispatchResponseWrapper;
  fn crdtp__DispatchResponse__DELETE(this: *mut DispatchResponseWrapper);
  fn crdtp__DispatchResponse__isSuccess(
    this: *const DispatchResponseWrapper,
  ) -> bool;
  fn crdtp__DispatchResponse__isError(
    this: *const DispatchResponseWrapper,
  ) -> bool;
  fn crdtp__DispatchResponse__isFallThrough(
    this: *const DispatchResponseWrapper,
  ) -> bool;
  fn crdtp__DispatchResponse__code(this: *const DispatchResponseWrapper)
  -> i32;
  fn crdtp__DispatchResponse__messageLen(
    this: *const DispatchResponseWrapper,
  ) -> usize;
  fn crdtp__DispatchResponse__messageCopy(
    this: *const DispatchResponseWrapper,
    out: *mut u8,
  );

  fn crdtp__UberDispatcher__new(
    channel: *mut RawFrontendChannel,
  ) -> *mut UberDispatcher;
  fn crdtp__UberDispatcher__DELETE(this: *mut UberDispatcher);
  fn crdtp__UberDispatcher__channel(
    this: *mut UberDispatcher,
  ) -> *mut RawFrontendChannel;
  fn crdtp__UberDispatcher__Dispatch(
    this: *mut UberDispatcher,
    dispatchable: *mut Dispatchable,
  );

  fn crdtp__vec_u8__new() -> *mut CppVecU8;
  fn crdtp__vec_u8__DELETE(this: *mut CppVecU8);
  fn crdtp__vec_u8__size(this: *const CppVecU8) -> usize;
  fn crdtp__vec_u8__copy(this: *const CppVecU8, out: *mut u8);

  fn crdtp__json__ConvertJSONToCBOR(
    json_data: *const u8,
    json_len: usize,
    cbor_out: *mut CppVecU8,
  ) -> bool;
  fn crdtp__json__ConvertCBORToJSON(
    cbor_data: *const u8,
    cbor_len: usize,
    json_out: *mut CppVecU8,
  ) -> bool;

  fn crdtp__CreateErrorResponse(
    call_id: i32,
    response: *mut DispatchResponseWrapper,
  ) -> *mut RawSerializable;
  fn crdtp__CreateResponse(
    call_id: i32,
    params: *mut RawSerializable,
  ) -> *mut RawSerializable;
  fn crdtp__CreateNotification(
    method: *const std::ffi::c_char,
    params: *mut RawSerializable,
  ) -> *mut RawSerializable;
  fn crdtp__CreateErrorNotification(
    response: *mut DispatchResponseWrapper,
  ) -> *mut RawSerializable;

  fn crdtp__DomainDispatcher__new(
    channel: *mut RawFrontendChannel,
    rust_dispatcher: *mut std::ffi::c_void,
  ) -> *mut RawDomainDispatcher;
  fn crdtp__DomainDispatcher__sendResponse(
    this: *mut RawDomainDispatcher,
    call_id: i32,
    response: *mut DispatchResponseWrapper,
    result: *mut RawSerializable,
  );
  fn crdtp__UberDispatcher__WireBackend(
    uber: *mut UberDispatcher,
    domain_data: *const u8,
    domain_len: usize,
    dispatcher: *mut RawDomainDispatcher,
  );
}

#[repr(C)]
pub struct Dispatchable(Opaque);

/// An owning handle to a [`Dispatchable`].
///
/// The serialized message and associated data are borrowed by the underlying
/// C++ object, so this handle keeps both buffers alive for as long as the
/// dispatchable can be accessed.
pub struct OwnedDispatchable {
  ptr: NonNull<Dispatchable>,
  _cbor_data: Box<[u8]>,
  _associated_data: Box<[u8]>,
}

impl Deref for OwnedDispatchable {
  type Target = Dispatchable;

  fn deref(&self) -> &Self::Target {
    unsafe { self.ptr.as_ref() }
  }
}

impl DerefMut for OwnedDispatchable {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { self.ptr.as_mut() }
  }
}

impl AsRef<Dispatchable> for OwnedDispatchable {
  fn as_ref(&self) -> &Dispatchable {
    self
  }
}

impl AsMut<Dispatchable> for OwnedDispatchable {
  fn as_mut(&mut self) -> &mut Dispatchable {
    self
  }
}

impl Drop for OwnedDispatchable {
  fn drop(&mut self) {
    unsafe {
      crdtp__Dispatchable__DELETE(self.ptr.as_ptr());
    }
  }
}

#[repr(C)]
struct DispatchResponseWrapper(Opaque);

#[repr(C)]
pub struct UberDispatcher(Opaque);

#[repr(C)]
pub(crate) struct CppVecU8(Opaque);

impl CppVecU8 {
  pub(crate) unsafe fn take_from_raw(ptr: *mut Self) -> Vec<u8> {
    unsafe {
      let len = crdtp__vec_u8__size(ptr);
      let mut result = vec![0u8; len];
      crdtp__vec_u8__copy(ptr, result.as_mut_ptr());
      crdtp__vec_u8__DELETE(ptr);
      result
    }
  }
}

#[repr(C)]
struct RawSerializable(Opaque);

#[repr(C)]
struct RawDomainDispatcher(Opaque);

pub struct Serializable {
  ptr: *mut RawSerializable,
}

impl Serializable {
  pub fn to_bytes(&self) -> Vec<u8> {
    unsafe {
      let vec = crdtp__vec_u8__new();
      crdtp__Serializable__AppendSerialized(self.ptr, vec);
      CppVecU8::take_from_raw(vec)
    }
  }

  fn into_raw(self) -> *mut RawSerializable {
    let ptr = self.ptr;
    std::mem::forget(self);
    ptr
  }
}

impl Drop for Serializable {
  fn drop(&mut self) {
    unsafe {
      crdtp__Serializable__DELETE(self.ptr);
    }
  }
}

impl Dispatchable {
  #[allow(clippy::new_ret_no_self)]
  pub fn new(cbor_data: &[u8]) -> OwnedDispatchable {
    Self::new_inner(cbor_data, &[], std::ptr::null_mut())
  }

  /// Creates a dispatchable with per-message associated data and a callback
  /// that receives commands which fall through the dispatcher.
  ///
  /// The callback may outlive this `Dispatchable` when an asynchronous command
  /// takes ownership of it.
  pub fn new_with_fallthrough(
    cbor_data: &[u8],
    associated_data: &[u8],
    fallthrough_callback: impl FnMut(i32, &[u8], &[u8], &[u8]) + 'static,
  ) -> OwnedDispatchable {
    let callback = Box::new(FallthroughCallbackData {
      callback: Box::new(fallthrough_callback),
    });
    let callback = Box::into_raw(callback).cast::<c_void>();
    Self::new_inner(cbor_data, associated_data, callback)
  }

  fn new_inner(
    cbor_data: &[u8],
    associated_data: &[u8],
    fallthrough_callback: *mut c_void,
  ) -> OwnedDispatchable {
    let cbor_data: Box<[u8]> = cbor_data.into();
    let associated_data: Box<[u8]> = associated_data.into();
    unsafe {
      let ptr = crdtp__Dispatchable__new(
        cbor_data.as_ptr(),
        cbor_data.len(),
        associated_data.as_ptr(),
        associated_data.len(),
        fallthrough_callback,
      );
      OwnedDispatchable {
        ptr: NonNull::new(ptr).unwrap(),
        _cbor_data: cbor_data,
        _associated_data: associated_data,
      }
    }
  }

  pub fn ok(&self) -> bool {
    unsafe { crdtp__Dispatchable__ok(self) }
  }

  pub fn call_id(&self) -> i32 {
    unsafe { crdtp__Dispatchable__callId(self) }
  }

  pub fn has_call_id(&self) -> bool {
    unsafe { crdtp__Dispatchable__hasCallId(self) }
  }

  pub fn method(&self) -> Vec<u8> {
    unsafe {
      let len = crdtp__Dispatchable__methodLen(self);
      let mut buf = vec![0u8; len];
      crdtp__Dispatchable__methodCopy(self, buf.as_mut_ptr());
      buf
    }
  }

  pub fn method_str(&self) -> String {
    String::from_utf8_lossy(&self.method()).into_owned()
  }

  pub fn session_id(&self) -> Vec<u8> {
    unsafe {
      let len = crdtp__Dispatchable__sessionIdLen(self);
      let mut buf = vec![0u8; len];
      crdtp__Dispatchable__sessionIdCopy(self, buf.as_mut_ptr());
      buf
    }
  }

  pub fn params(&self) -> Vec<u8> {
    unsafe {
      let len = crdtp__Dispatchable__paramsLen(self);
      let mut buf = vec![0u8; len];
      crdtp__Dispatchable__paramsCopy(self, buf.as_mut_ptr());
      buf
    }
  }

  pub fn associated_data(&self) -> Vec<u8> {
    unsafe {
      let len = crdtp__Dispatchable__associatedDataLen(self);
      let mut buf = vec![0u8; len];
      crdtp__Dispatchable__associatedDataCopy(self, buf.as_mut_ptr());
      buf
    }
  }
}

type FallthroughCallback = Box<dyn FnMut(i32, &[u8], &[u8], &[u8])>;

struct FallthroughCallbackData {
  callback: FallthroughCallback,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__FallthroughCallback__Run(
  callback: *mut c_void,
  call_id: i32,
  method_data: *const u8,
  method_len: usize,
  message_data: *const u8,
  message_len: usize,
  associated_data: *const u8,
  associated_data_len: usize,
) {
  unsafe {
    unsafe fn as_slice<'a>(data: *const u8, len: usize) -> &'a [u8] {
      if len == 0 {
        &[]
      } else {
        unsafe { std::slice::from_raw_parts(data, len) }
      }
    }

    let callback = &mut *(callback as *mut FallthroughCallbackData);
    (callback.callback)(
      call_id,
      as_slice(method_data, method_len),
      as_slice(message_data, message_len),
      as_slice(associated_data, associated_data_len),
    );
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__FallthroughCallback__Drop(callback: *mut c_void) {
  unsafe {
    drop(Box::from_raw(callback as *mut FallthroughCallbackData));
  }
}

pub struct DispatchResponse {
  ptr: *mut DispatchResponseWrapper,
}

impl DispatchResponse {
  pub fn success() -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__Success(),
      }
    }
  }

  pub fn fall_through() -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__FallThrough(),
      }
    }
  }

  pub fn parse_error(message: &str) -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__ParseError(
          message.as_ptr(),
          message.len(),
        ),
      }
    }
  }

  pub fn invalid_request(message: &str) -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__InvalidRequest(
          message.as_ptr(),
          message.len(),
        ),
      }
    }
  }

  pub fn method_not_found(message: &str) -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__MethodNotFound(
          message.as_ptr(),
          message.len(),
        ),
      }
    }
  }

  pub fn invalid_params(message: &str) -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__InvalidParams(
          message.as_ptr(),
          message.len(),
        ),
      }
    }
  }

  pub fn server_error(message: &str) -> Self {
    unsafe {
      Self {
        ptr: crdtp__DispatchResponse__ServerError(
          message.as_ptr(),
          message.len(),
        ),
      }
    }
  }

  pub fn is_success(&self) -> bool {
    unsafe { crdtp__DispatchResponse__isSuccess(self.ptr) }
  }

  pub fn is_error(&self) -> bool {
    unsafe { crdtp__DispatchResponse__isError(self.ptr) }
  }

  /// Returns true if this is a fall-through response.
  pub fn is_fall_through(&self) -> bool {
    unsafe { crdtp__DispatchResponse__isFallThrough(self.ptr) }
  }

  /// Get the error code.
  pub fn code(&self) -> i32 {
    unsafe { crdtp__DispatchResponse__code(self.ptr) }
  }

  /// Get the error message.
  pub fn message(&self) -> String {
    unsafe {
      let len = crdtp__DispatchResponse__messageLen(self.ptr);
      let mut buf = vec![0u8; len];
      crdtp__DispatchResponse__messageCopy(self.ptr, buf.as_mut_ptr());
      String::from_utf8_lossy(&buf).into_owned()
    }
  }

  fn into_raw(self) -> *mut DispatchResponseWrapper {
    let ptr = self.ptr;
    std::mem::forget(self);
    ptr
  }
}

impl Drop for DispatchResponse {
  fn drop(&mut self) {
    unsafe {
      crdtp__DispatchResponse__DELETE(self.ptr);
    }
  }
}

/// Trait for sending protocol responses and notifications to clients.
pub trait FrontendChannelImpl {
  /// Send a response to a protocol request.
  fn send_protocol_response(&mut self, call_id: i32, message: Serializable);
  /// Send a notification (no call_id).
  fn send_protocol_notification(&mut self, message: Serializable);
  /// Flush any queued notifications.
  fn flush_protocol_notifications(&mut self);
}

#[repr(C)]
struct RawFrontendChannel {
  _cxx_vtable: CxxVTable,
}

/// Wraps a Rust `FrontendChannelImpl` for use with the C++ dispatcher.
pub struct FrontendChannel {
  raw: UnsafeCell<RawFrontendChannel>,
  imp: Box<dyn FrontendChannelImpl>,
}

impl FrontendChannel {
  /// Create a new FrontendChannel wrapping the given implementation.
  pub fn new(imp: Box<dyn FrontendChannelImpl>) -> Pin<Box<Self>> {
    let channel = Box::new(Self {
      raw: UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() }),
      imp,
    });
    unsafe {
      crdtp__FrontendChannel__BASE__CONSTRUCT(
        channel.raw.get() as *mut _ as *mut MaybeUninit<RawFrontendChannel>
      );
    }
    Box::into_pin(channel)
  }

  fn raw_ptr(&self) -> *mut RawFrontendChannel {
    self.raw.get()
  }

  unsafe fn from_raw<'a>(ptr: *mut RawFrontendChannel) -> &'a mut Self {
    unsafe {
      let channel_ptr = ptr as *mut u8;
      let offset = std::mem::offset_of!(FrontendChannel, raw);
      let self_ptr = channel_ptr.sub(offset) as *mut Self;
      &mut *self_ptr
    }
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__FrontendChannel__BASE__sendProtocolResponse(
  this: *mut RawFrontendChannel,
  call_id: i32,
  message: *mut RawSerializable,
) {
  unsafe {
    let channel = FrontendChannel::from_raw(this);
    let msg = Serializable { ptr: message };
    channel.imp.send_protocol_response(call_id, msg);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__FrontendChannel__BASE__sendProtocolNotification(
  this: *mut RawFrontendChannel,
  message: *mut RawSerializable,
) {
  unsafe {
    let channel = FrontendChannel::from_raw(this);
    let msg = Serializable { ptr: message };
    channel.imp.send_protocol_notification(msg);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__FrontendChannel__BASE__flushProtocolNotifications(
  this: *mut RawFrontendChannel,
) {
  unsafe {
    let channel = FrontendChannel::from_raw(this);
    channel.imp.flush_protocol_notifications();
  }
}

impl UberDispatcher {
  /// Create a new UberDispatcher with the given frontend channel.
  pub fn new(channel: &Pin<Box<FrontendChannel>>) -> Box<Self> {
    unsafe {
      let ptr = crdtp__UberDispatcher__new(channel.raw_ptr());
      Box::from_raw(ptr)
    }
  }

  /// Dispatch a protocol message immediately.
  pub fn dispatch(&mut self, dispatchable: &mut Dispatchable) {
    unsafe {
      crdtp__UberDispatcher__Dispatch(self, dispatchable);
    }
  }
}

impl Drop for UberDispatcher {
  fn drop(&mut self) {
    unsafe {
      crdtp__UberDispatcher__DELETE(self);
    }
  }
}

/// Convert JSON bytes to CBOR bytes.
pub fn json_to_cbor(json: &[u8]) -> Option<Vec<u8>> {
  unsafe {
    let vec = crdtp__vec_u8__new();
    let ok = crdtp__json__ConvertJSONToCBOR(json.as_ptr(), json.len(), vec);
    if ok {
      Some(CppVecU8::take_from_raw(vec))
    } else {
      crdtp__vec_u8__DELETE(vec);
      None
    }
  }
}

/// Convert CBOR bytes to JSON bytes.
pub fn cbor_to_json(cbor: &[u8]) -> Option<Vec<u8>> {
  unsafe {
    let vec = crdtp__vec_u8__new();
    let ok = crdtp__json__ConvertCBORToJSON(cbor.as_ptr(), cbor.len(), vec);
    if ok {
      Some(CppVecU8::take_from_raw(vec))
    } else {
      crdtp__vec_u8__DELETE(vec);
      None
    }
  }
}

/// Create an error response message.
pub fn create_error_response(
  call_id: i32,
  response: DispatchResponse,
) -> Serializable {
  unsafe {
    let ptr = crdtp__CreateErrorResponse(call_id, response.into_raw());
    Serializable { ptr }
  }
}

/// Create an error notification message.
pub fn create_error_notification(response: DispatchResponse) -> Serializable {
  unsafe {
    let ptr = crdtp__CreateErrorNotification(response.into_raw());
    Serializable { ptr }
  }
}

/// Create a success response message with optional result params.
pub fn create_response(
  call_id: i32,
  params: Option<Serializable>,
) -> Serializable {
  unsafe {
    let params_ptr = match params {
      Some(p) => p.into_raw(),
      None => std::ptr::null_mut(),
    };
    let ptr = crdtp__CreateResponse(call_id, params_ptr);
    Serializable { ptr }
  }
}

/// Create a notification message with a method name and optional params.
///
/// # Panics
/// Panics if `method` contains interior null bytes.
pub fn create_notification(
  method: &str,
  params: Option<Serializable>,
) -> Serializable {
  unsafe {
    let method_cstr =
      CString::new(method).expect("method name must not contain null bytes");
    let params_ptr = match params {
      Some(p) => p.into_raw(),
      None => std::ptr::null_mut(),
    };
    let ptr = crdtp__CreateNotification(method_cstr.as_ptr(), params_ptr);
    Serializable { ptr }
  }
}

/// Trait for implementing a domain-specific protocol dispatcher.
///
/// The `dispatch` method executes the command immediately and returns whether
/// the command was handled.
pub trait DomainDispatcherImpl {
  fn dispatch(
    &mut self,
    command: &[u8],
    dispatchable: &Dispatchable,
    handle: &DomainDispatcherHandle,
  ) -> bool;
}

/// Handle to the C++ DomainDispatcher, used to send responses.
pub struct DomainDispatcherHandle {
  ptr: *mut RawDomainDispatcher,
}

impl DomainDispatcherHandle {
  /// Send a response for a dispatched command.
  pub fn send_response(
    &self,
    call_id: i32,
    response: DispatchResponse,
    result: Option<Serializable>,
  ) {
    unsafe {
      let result_ptr = match result {
        Some(r) => r.into_raw(),
        None => std::ptr::null_mut(),
      };
      crdtp__DomainDispatcher__sendResponse(
        self.ptr,
        call_id,
        response.into_raw(),
        result_ptr,
      );
    }
  }
}

/// A domain dispatcher that delegates to a Rust `DomainDispatcherImpl`.
///
/// Ownership model: the Rust `DomainDispatcher` is heap-allocated and its
/// pointer is stored in the C++ `crdtp__DomainDispatcher__BASE`. When the
/// C++ side is destroyed (by `UberDispatcher`'s destructor), it calls back
/// into Rust via `crdtp__DomainDispatcher__BASE__Drop` to free the Rust
/// allocation.
struct DomainDispatcherData {
  ptr: *mut RawDomainDispatcher,
  imp: Box<dyn DomainDispatcherImpl>,
  domain_bytes: Vec<u8>,
}

pub struct DomainDispatcher;

impl DomainDispatcher {
  /// Wire a Rust domain dispatcher implementation to an `UberDispatcher`.
  ///
  /// The implementation will handle commands for the given `domain` name.
  /// Ownership of `imp` is transferred to the C++ `UberDispatcher`; it
  /// will be dropped when the `UberDispatcher` is destroyed.
  pub fn wire(
    uber: &mut UberDispatcher,
    domain: &str,
    imp: Box<dyn DomainDispatcherImpl>,
  ) {
    // Keep domain bytes alive as long as the DomainDispatcherData, since
    // UberDispatcher stores domain as a span (pointer + length).
    let domain_bytes = domain.as_bytes().to_vec();

    let mut dd = Box::new(DomainDispatcherData {
      ptr: std::ptr::null_mut(),
      imp,
      domain_bytes,
    });

    unsafe {
      let rust_ptr =
        &mut *dd as *mut DomainDispatcherData as *mut std::ffi::c_void;
      let channel = crdtp__UberDispatcher__channel(uber);
      let raw = crdtp__DomainDispatcher__new(channel, rust_ptr);
      dd.ptr = raw;

      crdtp__UberDispatcher__WireBackend(
        uber,
        dd.domain_bytes.as_ptr(),
        dd.domain_bytes.len(),
        raw,
      );
    }

    // Transfer ownership to the C++ side. The C++ destructor will call
    // crdtp__DomainDispatcher__BASE__Drop to reclaim this allocation.
    let _ = Box::into_raw(dd);
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__DomainDispatcher__BASE__Dispatch(
  rust_dispatcher: *mut std::ffi::c_void,
  command_data: *const u8,
  command_len: usize,
  dispatchable: &Dispatchable,
) -> bool {
  unsafe {
    let dd = &mut *(rust_dispatcher as *mut DomainDispatcherData);
    let command = std::slice::from_raw_parts(command_data, command_len);
    let handle = DomainDispatcherHandle { ptr: dd.ptr };
    dd.imp.dispatch(command, dispatchable, &handle)
  }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn crdtp__DomainDispatcher__BASE__Drop(
  rust_dispatcher: *mut std::ffi::c_void,
) {
  unsafe {
    drop(Box::from_raw(rust_dispatcher as *mut DomainDispatcherData));
  }
}
