#include "edge_js_udp_wrap.h"

#include <algorithm>
#include <arpa/inet.h>
#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

#include <uv.h>

#include "edge_async_wrap.h"
#include "edge_runtime.h"
#include "edge_udp_listener.h"
#include "edge_udp_wrap.h"

namespace {

napi_value GetRefValue(napi_env env, napi_ref ref);
bool IsFunction(napi_env env, napi_value value);
bool ExtractBytes(napi_env env, napi_value value, const uint8_t** data, size_t* len);
std::string ValueToUtf8(napi_env env, napi_value value);
napi_value MakeInt32(napi_env env, int32_t value);
napi_value BuildChunksArray(napi_env env, uv_buf_t* bufs, size_t nbufs);
napi_value BuildAddressObject(napi_env env, const sockaddr* addr);

class JsUdpWrap final : public EdgeUdpWrapBase {
 public:
  explicit JsUdpWrap(napi_env env_in) : env(env_in), async_id(EdgeAsyncWrapNextId(env_in)) {}

  int RecvStart() override { return CallMethodReturningInt32("onreadstart", 0, nullptr, UV_EPROTO); }
  int RecvStop() override { return CallMethodReturningInt32("onreadstop", 0, nullptr, UV_EPROTO); }

  ssize_t Send(uv_buf_t* bufs, size_t nbufs, const sockaddr* addr) override {
    if (listener() == nullptr) return UV_EBADF;

    size_t total_len = 0;
    for (size_t i = 0; i < nbufs; ++i) total_len += bufs[i].len;

    napi_value self = GetRefValue(env, wrapper_ref);
    if (self == nullptr) return UV_EBADF;

    napi_value onwrite = nullptr;
    if (napi_get_named_property(env, self, "onwrite", &onwrite) != napi_ok || !IsFunction(env, onwrite)) {
      return UV_EPROTO;
    }

    EdgeUdpSendWrap* send_wrap = listener()->CreateSendWrap(total_len);
    if (send_wrap == nullptr) return UV_ENOSYS;

    napi_value req_obj = send_wrap->object(env);
    napi_value chunks = BuildChunksArray(env, bufs, nbufs);
    napi_value address = BuildAddressObject(env, addr);
    if (req_obj == nullptr || chunks == nullptr || address == nullptr) return UV_ENOSYS;

    napi_value argv[3] = {req_obj, chunks, address};
    napi_value result = nullptr;
    if (EdgeMakeCallback(env, self, onwrite, 3, argv, &result) != napi_ok || result == nullptr) {
      (void)EdgeHandlePendingExceptionNow(env, nullptr);
      return UV_EPROTO;
    }

    int64_t rc = UV_EPROTO;
    if (napi_get_value_int64(env, result, &rc) != napi_ok) return UV_EPROTO;
    return static_cast<ssize_t>(rc);
  }

  int32_t CallMethodReturningInt32(const char* name,
                                   size_t argc,
                                   napi_value* argv,
                                   int32_t fallback) {
    napi_value self = GetRefValue(env, wrapper_ref);
    if (self == nullptr) return fallback;
    napi_value fn = nullptr;
    if (napi_get_named_property(env, self, name, &fn) != napi_ok || !IsFunction(env, fn)) return fallback;
    napi_value result = nullptr;
    if (EdgeMakeCallback(env, self, fn, argc, argv, &result) != napi_ok || result == nullptr) {
      (void)EdgeHandlePendingExceptionNow(env, nullptr);
      return fallback;
    }
    int32_t out = fallback;
    if (napi_get_value_int32(env, result, &out) != napi_ok) return fallback;
    return out;
  }

  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  int32_t provider_type = kEdgeProviderJsUdpWrap;
  int64_t async_id = -1;
};

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok) return nullptr;
  return value;
}

bool IsFunction(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool ExtractBytes(napi_env env, napi_value value, const uint8_t** data, size_t* len) {
  if (env == nullptr || value == nullptr || data == nullptr || len == nullptr) return false;
  *data = nullptr;
  *len = 0;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    if (napi_get_buffer_info(env, value, &raw, len) != napi_ok) return false;
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    napi_value arraybuffer = nullptr;
    size_t offset = 0;
    void* raw = nullptr;
    if (napi_get_typedarray_info(env, value, &type, len, &raw, &arraybuffer, &offset) != napi_ok) {
      return false;
    }
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    napi_value arraybuffer = nullptr;
    size_t offset = 0;
    void* raw = nullptr;
    if (napi_get_dataview_info(env, value, len, &raw, &arraybuffer, &offset) != napi_ok) return false;
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    if (napi_get_arraybuffer_info(env, value, &raw, len) != napi_ok) return false;
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  return false;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return {};
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value BuildChunksArray(napi_env env, uv_buf_t* bufs, size_t nbufs) {
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, nbufs, &out) != napi_ok || out == nullptr) return nullptr;
  for (size_t i = 0; i < nbufs; ++i) {
    napi_value buffer = nullptr;
    void* copy = nullptr;
    if (napi_create_buffer_copy(env, bufs[i].len, bufs[i].base, &copy, &buffer) != napi_ok || buffer == nullptr) {
      return nullptr;
    }
    napi_set_element(env, out, static_cast<uint32_t>(i), buffer);
  }
  return out;
}

napi_value BuildAddressObject(napi_env env, const sockaddr* addr) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  if (addr == nullptr) return out;

  std::string address;
  int32_t port = 0;
  const char* family = nullptr;
  if (addr->sa_family == AF_INET) {
    char ip[INET_ADDRSTRLEN] = {0};
    const auto* a4 = reinterpret_cast<const sockaddr_in*>(addr);
    uv_ip4_name(a4, ip, sizeof(ip));
    address = ip;
    port = ntohs(a4->sin_port);
    family = "IPv4";
  } else if (addr->sa_family == AF_INET6) {
    char ip[INET6_ADDRSTRLEN] = {0};
    const auto* a6 = reinterpret_cast<const sockaddr_in6*>(addr);
    uv_ip6_name(a6, ip, sizeof(ip));
    address = ip;
    port = ntohs(a6->sin6_port);
    family = "IPv6";
  } else {
    return out;
  }

  napi_value address_v = nullptr;
  napi_value family_v = nullptr;
  napi_value port_v = nullptr;
  napi_create_string_utf8(env, address.c_str(), NAPI_AUTO_LENGTH, &address_v);
  napi_create_string_utf8(env, family, NAPI_AUTO_LENGTH, &family_v);
  napi_create_int32(env, port, &port_v);
  if (address_v != nullptr) napi_set_named_property(env, out, "address", address_v);
  if (family_v != nullptr) napi_set_named_property(env, out, "family", family_v);
  if (port_v != nullptr) napi_set_named_property(env, out, "port", port_v);
  return out;
}

JsUdpWrap* UnwrapJsUdpWrap(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  JsUdpWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

void JsUdpWrapFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<JsUdpWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->async_id > 0) EdgeAsyncWrapQueueDestroyId(env, wrap->async_id);
  if (wrap->wrapper_ref != nullptr) napi_delete_reference(env, wrap->wrapper_ref);
  delete wrap;
}

napi_value JsUdpWrapCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* wrap = new JsUdpWrap(env);
  napi_wrap(env, self, wrap, JsUdpWrapFinalize, nullptr, &wrap->wrapper_ref);
  return self;
}

napi_value JsUdpWrapRecvStart(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  return MakeInt32(env, wrap != nullptr ? wrap->RecvStart() : UV_EBADF);
}

napi_value JsUdpWrapRecvStop(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  return MakeInt32(env, wrap != nullptr ? wrap->RecvStop() : UV_EBADF);
}

napi_value JsUdpWrapEmitReceived(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  if (wrap == nullptr || wrap->listener() == nullptr || argc < 5) return undefined;

  const uint8_t* data = nullptr;
  size_t len = 0;
  if (!ExtractBytes(env, argv[0], &data, &len)) return undefined;

  int32_t family = 0;
  int32_t port = 0;
  int32_t flags = 0;
  napi_get_value_int32(env, argv[1], &family);
  napi_get_value_int32(env, argv[3], &port);
  napi_get_value_int32(env, argv[4], &flags);
  const std::string address = ValueToUtf8(env, argv[2]);

  sockaddr_storage storage{};
  int rc = 0;
  if (family == 4) {
    rc = uv_ip4_addr(address.c_str(), port, reinterpret_cast<sockaddr_in*>(&storage));
  } else {
    rc = uv_ip6_addr(address.c_str(), port, reinterpret_cast<sockaddr_in6*>(&storage));
  }
  if (rc != 0) return undefined;

  while (len != 0) {
    uv_buf_t buf = wrap->listener()->OnAlloc(len);
    if (buf.base == nullptr || buf.len == 0) break;
    const size_t available = std::min(len, static_cast<size_t>(buf.len));
    std::memcpy(buf.base, data, available);
    data += available;
    len -= available;
    wrap->listener()->OnRecv(static_cast<ssize_t>(available),
                             buf,
                             reinterpret_cast<const sockaddr*>(&storage),
                             static_cast<unsigned int>(flags));
  }

  return undefined;
}

napi_value JsUdpWrapOnSendDone(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  if (wrap == nullptr || wrap->listener() == nullptr || argc < 2) return undefined;

  EdgeUdpSendWrap* req_wrap = EdgeUdpWrapUnwrapSendWrap(env, argv[0]);
  if (req_wrap == nullptr) return undefined;
  int32_t status = 0;
  napi_get_value_int32(env, argv[1], &status);
  wrap->listener()->OnSendDone(req_wrap, status);
  return undefined;
}

napi_value JsUdpWrapOnAfterBind(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  if (wrap != nullptr && wrap->listener() != nullptr) wrap->listener()->OnAfterBind();
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value JsUdpWrapGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  napi_value out = nullptr;
  napi_create_int64(env, wrap != nullptr ? wrap->async_id : -1, &out);
  return out;
}

napi_value JsUdpWrapAsyncReset(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  if (wrap != nullptr) EdgeAsyncWrapReset(env, &wrap->async_id);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value JsUdpWrapGetProviderType(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  JsUdpWrap* wrap = UnwrapJsUdpWrap(env, self);
  return MakeInt32(env, wrap != nullptr ? wrap->provider_type : kEdgeProviderNone);
}

}  // namespace

napi_value EdgeInstallJsUdpWrapBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor props[] = {
      {"recvStart", nullptr, JsUdpWrapRecvStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"recvStop", nullptr, JsUdpWrapRecvStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"emitReceived", nullptr, JsUdpWrapEmitReceived, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onSendDone", nullptr, JsUdpWrapOnSendDone, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onAfterBind", nullptr, JsUdpWrapOnAfterBind, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, JsUdpWrapGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, JsUdpWrapAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType",
       nullptr,
       JsUdpWrapGetProviderType,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
  };

  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "JSUDPWrap",
                        NAPI_AUTO_LENGTH,
                        JsUdpWrapCtor,
                        nullptr,
                        sizeof(props) / sizeof(props[0]),
                        props,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "JSUDPWrap", ctor);
  return binding;
}
