// Copyright 2024 the Deno authors. All rights reserved. MIT license.

#include <memory>

#include "support.h"
#include "v8-inspector-protocol.h"
#include "v8/third_party/inspector_protocol/crdtp/cbor.h"
#include "v8/third_party/inspector_protocol/crdtp/dispatch.h"
#include "v8/third_party/inspector_protocol/crdtp/frontend_channel.h"
#include "v8/third_party/inspector_protocol/crdtp/json.h"
#include "v8/third_party/inspector_protocol/crdtp/parser_handler.h"

using namespace support;
using namespace v8_crdtp;

extern "C" {
void crdtp__FrontendChannel__BASE__sendProtocolResponse(FrontendChannel* self,
                                                        int call_id,
                                                        Serializable* message);
void crdtp__FrontendChannel__BASE__sendProtocolNotification(
    FrontendChannel* self, Serializable* message);
void crdtp__FrontendChannel__BASE__flushProtocolNotifications(
    FrontendChannel* self);
void crdtp__FallthroughCallback__Run(
    void* callback, int call_id, const uint8_t* method_data, size_t method_len,
    const uint8_t* message_data, size_t message_len,
    const uint8_t* associated_data, size_t associated_data_len);
void crdtp__FallthroughCallback__Drop(void* callback);
}  // extern "C"

struct crdtp__FrontendChannel__BASE : public FrontendChannel {
  void SendProtocolResponse(int call_id,
                            std::unique_ptr<Serializable> message) override {
    crdtp__FrontendChannel__BASE__sendProtocolResponse(this, call_id,
                                                       message.release());
  }
  void SendProtocolNotification(
      std::unique_ptr<Serializable> message) override {
    crdtp__FrontendChannel__BASE__sendProtocolNotification(this,
                                                           message.release());
  }
  void FlushProtocolNotifications() override {
    crdtp__FrontendChannel__BASE__flushProtocolNotifications(this);
  }
};

static_assert(sizeof(crdtp__FrontendChannel__BASE) <= sizeof(void*),
              "FrontendChannel BASE must fit in a single vtable pointer");

extern "C" {

void crdtp__FrontendChannel__BASE__CONSTRUCT(
    uninit_t<crdtp__FrontendChannel__BASE>* buf) {
  construct_in_place<crdtp__FrontendChannel__BASE>(buf);
}

void crdtp__Serializable__DELETE(Serializable* self) { delete self; }

// Serialize once into a caller-provided CppVecU8 to avoid double-serialization.
void crdtp__Serializable__AppendSerialized(const Serializable* self,
                                           std::vector<uint8_t>* out) {
  self->AppendSerialized(out);
}

Dispatchable* crdtp__Dispatchable__new(const uint8_t* data, size_t len,
                                       const uint8_t* associated_data,
                                       size_t associated_data_len,
                                       void* rust_callback) {
  FallthroughCallback fallthrough_callback;
  if (rust_callback) {
    auto callback =
        std::shared_ptr<void>(rust_callback, crdtp__FallthroughCallback__Drop);
    fallthrough_callback = [callback](
                               int call_id, span<uint8_t> method,
                               span<uint8_t> serialized_message,
                               std::string_view fallthrough_associated_data) {
      crdtp__FallthroughCallback__Run(
          callback.get(), call_id, method.data(), method.size(),
          serialized_message.data(), serialized_message.size(),
          reinterpret_cast<const uint8_t*>(fallthrough_associated_data.data()),
          fallthrough_associated_data.size());
    };
  }
  return new Dispatchable(
      span<uint8_t>(data, len),
      std::string_view(reinterpret_cast<const char*>(associated_data),
                       associated_data_len),
      std::move(fallthrough_callback));
}

void crdtp__Dispatchable__DELETE(Dispatchable* self) { delete self; }

bool crdtp__Dispatchable__ok(const Dispatchable* self) { return self->ok(); }

int32_t crdtp__Dispatchable__callId(const Dispatchable* self) {
  return self->CallId();
}

bool crdtp__Dispatchable__hasCallId(const Dispatchable* self) {
  return self->HasCallId();
}

size_t crdtp__Dispatchable__methodLen(const Dispatchable* self) {
  return self->Method().size();
}

void crdtp__Dispatchable__methodCopy(const Dispatchable* self, uint8_t* out) {
  span<uint8_t> method = self->Method();
  memcpy(out, method.data(), method.size());
}

size_t crdtp__Dispatchable__sessionIdLen(const Dispatchable* self) {
  return self->SessionId().size();
}

void crdtp__Dispatchable__sessionIdCopy(const Dispatchable* self,
                                        uint8_t* out) {
  span<uint8_t> session_id = self->SessionId();
  memcpy(out, session_id.data(), session_id.size());
}

size_t crdtp__Dispatchable__paramsLen(const Dispatchable* self) {
  return self->Params().size();
}

void crdtp__Dispatchable__paramsCopy(const Dispatchable* self, uint8_t* out) {
  span<uint8_t> params = self->Params();
  memcpy(out, params.data(), params.size());
}

size_t crdtp__Dispatchable__associatedDataLen(const Dispatchable* self) {
  return self->AssociatedData().size();
}

void crdtp__Dispatchable__associatedDataCopy(const Dispatchable* self,
                                             uint8_t* out) {
  std::string_view associated_data = self->AssociatedData();
  memcpy(out, associated_data.data(), associated_data.size());
}

struct DispatchResponseWrapper {
  DispatchResponse inner;

  explicit DispatchResponseWrapper(DispatchResponse&& r)
      : inner(std::move(r)) {}
};

DispatchResponseWrapper* crdtp__DispatchResponse__Success() {
  return new DispatchResponseWrapper(DispatchResponse::Success());
}

DispatchResponseWrapper* crdtp__DispatchResponse__FallThrough() {
  return new DispatchResponseWrapper(DispatchResponse::FallThrough());
}

DispatchResponseWrapper* crdtp__DispatchResponse__ParseError(const char* msg,
                                                             size_t len) {
  return new DispatchResponseWrapper(
      DispatchResponse::ParseError(std::string(msg, len)));
}

DispatchResponseWrapper* crdtp__DispatchResponse__InvalidRequest(
    const char* msg, size_t len) {
  return new DispatchResponseWrapper(
      DispatchResponse::InvalidRequest(std::string(msg, len)));
}

DispatchResponseWrapper* crdtp__DispatchResponse__MethodNotFound(
    const char* msg, size_t len) {
  return new DispatchResponseWrapper(
      DispatchResponse::MethodNotFound(std::string(msg, len)));
}

DispatchResponseWrapper* crdtp__DispatchResponse__InvalidParams(const char* msg,
                                                                size_t len) {
  return new DispatchResponseWrapper(
      DispatchResponse::InvalidParams(std::string(msg, len)));
}

DispatchResponseWrapper* crdtp__DispatchResponse__ServerError(const char* msg,
                                                              size_t len) {
  return new DispatchResponseWrapper(
      DispatchResponse::ServerError(std::string(msg, len)));
}

void crdtp__DispatchResponse__DELETE(DispatchResponseWrapper* self) {
  delete self;
}

bool crdtp__DispatchResponse__isSuccess(const DispatchResponseWrapper* self) {
  return self->inner.IsSuccess();
}

bool crdtp__DispatchResponse__isError(const DispatchResponseWrapper* self) {
  return self->inner.IsError();
}

bool crdtp__DispatchResponse__isFallThrough(
    const DispatchResponseWrapper* self) {
  return self->inner.IsFallThrough();
}

int crdtp__DispatchResponse__code(const DispatchResponseWrapper* self) {
  return static_cast<int>(self->inner.Code());
}

size_t crdtp__DispatchResponse__messageLen(
    const DispatchResponseWrapper* self) {
  return self->inner.Message().size();
}

void crdtp__DispatchResponse__messageCopy(const DispatchResponseWrapper* self,
                                          char* out) {
  const std::string& msg = self->inner.Message();
  memcpy(out, msg.data(), msg.size());
}

UberDispatcher* crdtp__UberDispatcher__new(FrontendChannel* channel) {
  return new UberDispatcher(channel);
}

void crdtp__UberDispatcher__DELETE(UberDispatcher* self) { delete self; }

FrontendChannel* crdtp__UberDispatcher__channel(UberDispatcher* self) {
  return self->channel();
}

void crdtp__UberDispatcher__Dispatch(UberDispatcher* self,
                                     Dispatchable* dispatchable) {
  self->Dispatch(*dispatchable);
}

// Convert JSON to CBOR
bool crdtp__json__ConvertJSONToCBOR(const uint8_t* json_data, size_t json_len,
                                    std::vector<uint8_t>* cbor_out) {
  json::ConvertJSONToCBOR(span<uint8_t>(json_data, json_len), cbor_out);
  return !cbor_out->empty();
}

// Convert CBOR to JSON
bool crdtp__json__ConvertCBORToJSON(const uint8_t* cbor_data, size_t cbor_len,
                                    std::vector<uint8_t>* json_out) {
  std::string json_str;
  Status status =
      json::ConvertCBORToJSON(span<uint8_t>(cbor_data, cbor_len), &json_str);
  if (!status.ok()) {
    return false;
  }
  json_out->assign(json_str.begin(), json_str.end());
  return true;
}

std::vector<uint8_t>* crdtp__vec_u8__new() {
  return new std::vector<uint8_t>();
}

std::vector<uint8_t>* v8_inspector__RemoteObject__toBytes(
    const v8_inspector::protocol::Runtime::API::RemoteObject* self) {
  auto* bytes = crdtp__vec_u8__new();
  self->AppendSerialized(bytes);
  return bytes;
}

void crdtp__vec_u8__DELETE(std::vector<uint8_t>* self) { delete self; }

size_t crdtp__vec_u8__size(const std::vector<uint8_t>* self) {
  return self->size();
}

void crdtp__vec_u8__copy(const std::vector<uint8_t>* self, uint8_t* out) {
  memcpy(out, self->data(), self->size());
}

Serializable* crdtp__CreateErrorResponse(int call_id,
                                         DispatchResponseWrapper* response) {
  return CreateErrorResponse(call_id, std::move(response->inner)).release();
}

Serializable* crdtp__CreateResponse(int call_id, Serializable* params) {
  std::unique_ptr<Serializable> params_ptr(params);
  return CreateResponse(call_id, std::move(params_ptr)).release();
}

// Owns a copy of the method string, since upstream Notification stores only
// a raw const char* pointer which leads to use-after-free when the Rust
// CString is dropped before AppendSerialized is called.
class OwnedNotification : public Serializable {
 public:
  OwnedNotification(const char* method, std::unique_ptr<Serializable> params)
      : method_(method), params_(std::move(params)) {}

  void AppendSerialized(std::vector<uint8_t>* out) const override {
    Status status;
    std::unique_ptr<ParserHandler> encoder = cbor::NewCBOREncoder(out, &status);
    encoder->HandleMapBegin();
    encoder->HandleString8(SpanFrom("method"));
    encoder->HandleString8(SpanFrom(method_));
    encoder->HandleString8(SpanFrom("params"));
    if (params_) {
      params_->AppendSerialized(out);
    } else {
      encoder->HandleMapBegin();
      encoder->HandleMapEnd();
    }
    encoder->HandleMapEnd();
    assert(status.ok());
  }

 private:
  std::string method_;
  std::unique_ptr<Serializable> params_;
};

Serializable* crdtp__CreateNotification(const char* method,
                                        Serializable* params) {
  std::unique_ptr<Serializable> params_ptr(params);
  return new OwnedNotification(method, std::move(params_ptr));
}

Serializable* crdtp__CreateErrorNotification(
    DispatchResponseWrapper* response) {
  return CreateErrorNotification(std::move(response->inner)).release();
}

}  // extern "C"

// DomainDispatcher binding - allows Rust to implement domain dispatchers.

extern "C" {
// Rust callback: given a domain dispatcher pointer and a command name,
// executes the command and returns whether it was handled.
bool crdtp__DomainDispatcher__BASE__Dispatch(void* rust_dispatcher,
                                             const uint8_t* command_data,
                                             size_t command_len,
                                             const Dispatchable& dispatchable);
// Rust callback: destroy the Rust DomainDispatcher when C++ side is destroyed.
void crdtp__DomainDispatcher__BASE__Drop(void* rust_dispatcher);
}

struct crdtp__DomainDispatcher__BASE : public DomainDispatcher {
  void* rust_dispatcher_;

  crdtp__DomainDispatcher__BASE(FrontendChannel* channel, void* rust_dispatcher)
      : DomainDispatcher(channel), rust_dispatcher_(rust_dispatcher) {}

  ~crdtp__DomainDispatcher__BASE() override {
    crdtp__DomainDispatcher__BASE__Drop(rust_dispatcher_);
  }

  bool Dispatch(span<uint8_t> command_name,
                Dispatchable& dispatchable) override {
    return crdtp__DomainDispatcher__BASE__Dispatch(
        rust_dispatcher_, command_name.data(), command_name.size(),
        dispatchable);
  }
};

extern "C" {

crdtp__DomainDispatcher__BASE* crdtp__DomainDispatcher__new(
    FrontendChannel* channel, void* rust_dispatcher) {
  return new crdtp__DomainDispatcher__BASE(channel, rust_dispatcher);
}

void crdtp__DomainDispatcher__sendResponse(crdtp__DomainDispatcher__BASE* self,
                                           int call_id,
                                           DispatchResponseWrapper* response,
                                           Serializable* result) {
  std::unique_ptr<Serializable> result_ptr(result);
  self->sendResponse(call_id, std::move(response->inner),
                     std::move(result_ptr));
  delete response;
}

void crdtp__UberDispatcher__WireBackend(
    UberDispatcher* uber, const uint8_t* domain_data, size_t domain_len,
    crdtp__DomainDispatcher__BASE* dispatcher) {
  std::unique_ptr<DomainDispatcher> dispatcher_ptr(dispatcher);
  uber->WireBackend(span<uint8_t>(domain_data, domain_len),
                    std::vector<std::pair<span<uint8_t>, span<uint8_t>>>(),
                    std::move(dispatcher_ptr));
}

}  // extern "C"
