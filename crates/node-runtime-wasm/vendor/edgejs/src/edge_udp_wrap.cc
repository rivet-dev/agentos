#include "edge_udp_wrap.h"

#include <arpa/inet.h>

#include <climits>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include <uv.h>
#if !defined(_WIN32)
#include <net/if.h>
#include <sys/socket.h>
#endif

#include "edge_runtime.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_udp_listener.h"

namespace {

class UdpWrap;

void DeleteRef(napi_env env, napi_ref* ref);

struct UdpBindingState {
  explicit UdpBindingState(napi_env env_in) : env(env_in) {}
  ~UdpBindingState() {
    DeleteRef(env, &ctor_ref);
  }

  napi_env env = nullptr;
  napi_ref ctor_ref = nullptr;
};

struct SendWrap final : public EdgeUdpSendWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  napi_ref chunks_ref = nullptr;
  void* active_request_token = nullptr;
  int64_t async_id = -1;
  int32_t provider_type = kEdgeProviderUdpSendWrap;
  bool destroy_queued = false;
  bool active = false;

  napi_value object(napi_env env_in) const override;
};

void OnClosed(uv_handle_t* h);
void CloseUdpWrapForCleanup(void* data);

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value v = nullptr;
  if (napi_get_reference_value(env, ref, &v) != napi_ok) return nullptr;
  return v;
}

napi_value MakeInt32(napi_env env, int32_t v) {
  napi_value out = nullptr;
  napi_create_int32(env, v, &out);
  return out;
}

napi_value MakeBool(napi_env env, bool v) {
  napi_value out = nullptr;
  napi_get_boolean(env, v, &out);
  return out;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  size_t len = 0;
  if (napi_coerce_to_string(env, value, &value) != napi_ok ||
      napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) {
    return {};
  }
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

std::string FormatIPv6AddressWithScope(const sockaddr_in6* a6) {
  if (a6 == nullptr) return "";
  char ip[INET6_ADDRSTRLEN] = {0};
  uv_ip6_name(a6, ip, sizeof(ip));
  std::string out(ip);
  if (a6->sin6_scope_id == 0) return out;
#if defined(_WIN32)
  out += "%";
  out += std::to_string(static_cast<unsigned int>(a6->sin6_scope_id));
#else
  char ifname[IF_NAMESIZE] = {0};
  if (if_indextoname(a6->sin6_scope_id, ifname) != nullptr && ifname[0] != '\0') {
    out += "%";
    out += ifname;
  } else {
    out += "%";
    out += std::to_string(static_cast<unsigned int>(a6->sin6_scope_id));
  }
#endif
  return out;
}

size_t TypedArrayElementSize(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
      return 2;
    case napi_int32_array:
    case napi_uint32_array:
    case napi_float32_array:
      return 4;
    case napi_float64_array:
    case napi_bigint64_array:
    case napi_biguint64_array:
      return 8;
    default:
      return 1;
  }
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  return type == napi_function;
}

void DeleteRef(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

UdpBindingState& EnsureUdpBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<UdpBindingState>(
      env, kEdgeEnvironmentSlotUdpBindingState);
}

UdpBindingState* GetUdpBindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<UdpBindingState>(env, kEdgeEnvironmentSlotUdpBindingState);
}

napi_value SendWrap::object(napi_env env_in) const {
  napi_env use_env = env_in != nullptr ? env_in : env;
  napi_value value = GetRefValue(use_env, active_ref);
  if (value != nullptr) return value;
  return GetRefValue(use_env, wrapper_ref);
}

napi_value SendWrapGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<SendWrap*>(data);
  return wrap != nullptr ? wrap->object(env) : nullptr;
}

void CancelSendWrapRequest(void* data) {
  auto* wrap = static_cast<SendWrap*>(data);
  if (wrap == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&wrap->req));
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

bool GetInt32Like(napi_env env, napi_value value, int32_t* out) {
  if (value == nullptr || out == nullptr) return false;
  double number = 0;
  if (napi_get_value_double(env, value, &number) != napi_ok || !std::isfinite(number)) return false;
  const double truncated = std::trunc(number);
  if (truncated != number || truncated < static_cast<double>(INT32_MIN) ||
      truncated > static_cast<double>(INT32_MAX)) {
    return false;
  }
  *out = static_cast<int32_t>(truncated);
  return true;
}

bool ReadUint32Property(napi_env env, napi_value obj, const char* key, uint32_t* out) {
  if (obj == nullptr || out == nullptr) return false;
  napi_value v = nullptr;
  if (napi_get_named_property(env, obj, key, &v) != napi_ok || v == nullptr) return false;
  return napi_get_value_uint32(env, v, out) == napi_ok;
}

bool ExtractArrayBufferViewBytes(napi_env env, napi_value value, const char** src, size_t* len) {
  if (value == nullptr || src == nullptr || len == nullptr) return false;
  *src = nullptr;
  *len = 0;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    if (napi_get_buffer_info(env, value, &data, len) != napi_ok) return false;
    *src = static_cast<const char*>(data);
    return true;
  }

  bool is_typed = false;
  if (napi_is_typedarray(env, value, &is_typed) == napi_ok && is_typed) {
    napi_typedarray_type tt = napi_uint8_array;
    size_t element_len = 0;
    void* data = nullptr;
    napi_value ab = nullptr;
    size_t off = 0;
    if (napi_get_typedarray_info(env, value, &tt, &element_len, &data, &ab, &off) != napi_ok) {
      return false;
    }
    *src = static_cast<const char*>(data);
    uint32_t byte_len = 0;
    if (ReadUint32Property(env, value, "byteLength", &byte_len)) {
      *len = static_cast<size_t>(byte_len);
    } else {
      *len = element_len * TypedArrayElementSize(tt);
    }
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    void* data = nullptr;
    napi_value ab = nullptr;
    size_t off = 0;
    if (napi_get_dataview_info(env, value, len, &data, &ab, &off) != napi_ok) return false;
    *src = static_cast<const char*>(data);
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* data = nullptr;
    if (napi_get_arraybuffer_info(env, value, &data, len) != napi_ok) return false;
    *src = static_cast<const char*>(data);
    return true;
  }

  return false;
}

void ReleaseSendWrapState(SendWrap* wrap) {
  if (wrap == nullptr) return;
  if (wrap->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(wrap->env, wrap->active_request_token);
    wrap->active_request_token = nullptr;
  }
  DeleteRef(wrap->env, &wrap->active_ref);
  DeleteRef(wrap->env, &wrap->chunks_ref);
  delete[] wrap->bufs;
  wrap->bufs = nullptr;
  wrap->nbufs = 0;
  wrap->msg_size = 0;
  wrap->have_callback = false;
  wrap->active = false;
  wrap->req = uv_udp_send_t{};
}

void QueueSendWrapDestroyIfNeeded(SendWrap* wrap) {
  if (wrap == nullptr || wrap->destroy_queued || wrap->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(wrap->env, wrap->async_id);
  wrap->destroy_queued = true;
}

void QueueUdpWrapDestroyIfNeeded(UdpWrap* wrap);

bool CreateStrongRef(napi_env env, napi_value value, napi_ref* out_ref) {
  if (out_ref == nullptr) return false;
  DeleteRef(env, out_ref);
  if (value == nullptr) return true;
  return napi_create_reference(env, value, 1, out_ref) == napi_ok;
}

void PopulateUvExceptionInfo(napi_env env, napi_value ctx, int rc, const char* syscall) {
  if (ctx == nullptr) return;
  napi_value errno_v = nullptr;
  napi_value code_v = nullptr;
  napi_value message_v = nullptr;
  napi_value syscall_v = nullptr;
  napi_create_int32(env, rc, &errno_v);
  napi_create_string_utf8(env, uv_err_name(rc), NAPI_AUTO_LENGTH, &code_v);
  napi_create_string_utf8(env, uv_strerror(rc), NAPI_AUTO_LENGTH, &message_v);
  napi_create_string_utf8(env, syscall, NAPI_AUTO_LENGTH, &syscall_v);
  if (errno_v != nullptr) napi_set_named_property(env, ctx, "errno", errno_v);
  if (code_v != nullptr) napi_set_named_property(env, ctx, "code", code_v);
  if (message_v != nullptr) napi_set_named_property(env, ctx, "message", message_v);
  if (syscall_v != nullptr) napi_set_named_property(env, ctx, "syscall", syscall_v);
}

void ExternalBufferFinalize(napi_env env, void* data, void* hint) {
  (void)env;
  (void)hint;
  free(data);
}

napi_value CreateExternalBuffer(napi_env env, char* data, size_t len) {
  if (len == 0) {
    free(data);
    void* out = nullptr;
    napi_value buffer = nullptr;
    if (napi_create_buffer(env, 0, &out, &buffer) != napi_ok) return nullptr;
    return buffer;
  }

  napi_value buffer = nullptr;
  if (napi_create_external_buffer(env, len, data, ExternalBufferFinalize, nullptr, &buffer) != napi_ok ||
      buffer == nullptr) {
    free(data);
    return nullptr;
  }
  return buffer;
}

napi_value CreateErrorValue(napi_env env, const char* message) {
  napi_value msg = nullptr;
  napi_value err = nullptr;
  napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &msg);
  napi_create_error(env, nullptr, msg, &err);
  return err;
}

napi_value BuildRinfoObject(napi_env env, const sockaddr* addr, ssize_t nread) {
  if (addr == nullptr) return nullptr;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  std::string ip;
  int port = 0;
  const char* family = nullptr;
  if (addr->sa_family == AF_INET6) {
    const auto* a6 = reinterpret_cast<const sockaddr_in6*>(addr);
    ip = FormatIPv6AddressWithScope(a6);
    port = ntohs(a6->sin6_port);
    family = "IPv6";
  } else if (addr->sa_family == AF_INET) {
    char ip4[INET6_ADDRSTRLEN] = {0};
    const auto* a4 = reinterpret_cast<const sockaddr_in*>(addr);
    uv_ip4_name(a4, ip4, sizeof(ip4));
    ip = ip4;
    port = ntohs(a4->sin_port);
    family = "IPv4";
  } else {
    return nullptr;
  }

  napi_value ip_v = nullptr;
  napi_value family_v = nullptr;
  napi_value port_v = nullptr;
  napi_value size_v = nullptr;
  napi_create_string_utf8(env, ip.c_str(), NAPI_AUTO_LENGTH, &ip_v);
  napi_create_string_utf8(env, family, NAPI_AUTO_LENGTH, &family_v);
  napi_create_int32(env, port, &port_v);
  napi_create_int32(env, nread >= 0 ? static_cast<int32_t>(nread) : 0, &size_v);
  if (ip_v != nullptr) napi_set_named_property(env, out, "address", ip_v);
  if (family_v != nullptr) napi_set_named_property(env, out, "family", family_v);
  if (port_v != nullptr) napi_set_named_property(env, out, "port", port_v);
  if (size_v != nullptr) napi_set_named_property(env, out, "size", size_v);
  return out;
}

void CallOptionalCallback(napi_env env,
                          napi_value self,
                          const char* name,
                          size_t argc,
                          napi_value* argv,
                          int64_t async_id = 0) {
  if (self == nullptr) return;
  napi_value callback = nullptr;
  if (napi_get_named_property(env, self, name, &callback) != napi_ok || !IsFunction(env, callback)) return;
  napi_value ignored = nullptr;
  if (async_id > 0) {
    (void)EdgeAsyncWrapMakeCallback(env, async_id, self, self, callback, argc, argv, &ignored, kEdgeMakeCallbackNone);
  } else {
    EdgeMakeCallback(env, self, callback, argc, argv, &ignored);
  }
  (void)EdgeHandlePendingExceptionNow(env, nullptr);
}

void CallOnError(napi_env env, napi_value self, int64_t async_id, ssize_t nread, napi_value error) {
  napi_value argv[3] = {MakeInt32(env, static_cast<int32_t>(nread)), self, error};
  CallOptionalCallback(env, self, "onerror", 3, argv, async_id);
}

void ThrowInvalidUdpReceiver(napi_env env) {
  napi_throw_type_error(env, nullptr, "Invalid UDP receiver");
}

class UdpWrap final : public EdgeUdpWrapBase, public EdgeUdpListener {
 public:
 explicit UdpWrap(napi_env env_in) : async_id(EdgeAsyncWrapNextId(env_in)) {
    EdgeHandleWrapInit(&handle_wrap, env_in);
    uv_loop_t* loop = EdgeGetEnvLoop(env_in);
    if (loop != nullptr) {
      if (uv_udp_init(loop, &handle) == 0) {
        handle_wrap.state = kEdgeHandleInitialized;
      }
    }
    handle.data = this;
    if (handle_wrap.state == kEdgeHandleInitialized) {
      EdgeHandleWrapAttach(&handle_wrap,
                          this,
                          reinterpret_cast<uv_handle_t*>(&handle),
                          CloseUdpWrapForCleanup);
    }
    set_listener(this);
  }

  int RecvStart() override {
    if (handle_wrap.state != kEdgeHandleInitialized ||
        uv_is_closing(reinterpret_cast<uv_handle_t*>(&handle))) {
      return UV_EBADF;
    }
    int rc = uv_udp_recv_start(&handle, OnAllocCallback, OnRecvCallback);
    if (rc == UV_EALREADY) rc = 0;
    return rc;
  }

  int RecvStop() override {
    if (handle_wrap.state != kEdgeHandleInitialized ||
        uv_is_closing(reinterpret_cast<uv_handle_t*>(&handle))) {
      return UV_EBADF;
    }
    return uv_udp_recv_stop(&handle);
  }

  ssize_t Send(uv_buf_t* bufs_ptr, size_t count, const sockaddr* addr) override {
    if (handle_wrap.state != kEdgeHandleInitialized ||
        uv_is_closing(reinterpret_cast<uv_handle_t*>(&handle))) {
      return UV_EBADF;
    }

    size_t msg_size = 0;
    for (size_t i = 0; i < count; i++) msg_size += bufs_ptr[i].len;

    int err = 0;
    if (!EdgeExecArgvHasFlag("--test-udp-no-try-send")) {
      err = uv_udp_try_send(&handle, bufs_ptr, count, addr);
      if (err == UV_ENOSYS || err == UV_EAGAIN) {
        err = 0;
      } else if (err >= 0) {
        size_t sent = static_cast<size_t>(err);
        while (count > 0 && bufs_ptr->len <= sent) {
          sent -= bufs_ptr->len;
          bufs_ptr++;
          count--;
        }
        if (count == 0) {
          return static_cast<ssize_t>(msg_size + 1);
        }
        bufs_ptr->base += sent;
        bufs_ptr->len -= sent;
        err = 0;
      }
    }

    if (err != 0) return err;
    if (count == 0) return static_cast<ssize_t>(msg_size + 1);

    auto* req_wrap = static_cast<SendWrap*>(listener()->CreateSendWrap(msg_size));
    if (req_wrap == nullptr) return UV_ENOSYS;

    req_wrap->bufs = new uv_buf_t[count];
    req_wrap->nbufs = count;
    std::memcpy(req_wrap->bufs, bufs_ptr, sizeof(uv_buf_t) * count);
    req_wrap->req.data = req_wrap;

    err = uv_udp_send(&req_wrap->req,
                      &handle,
                      req_wrap->bufs,
                      static_cast<unsigned int>(count),
                      addr,
                      OnSendDoneCallback);
    if (err != 0) {
      QueueSendWrapDestroyIfNeeded(req_wrap);
      ReleaseSendWrapState(req_wrap);
    }
    return err;
  }

  uv_buf_t OnAlloc(size_t suggested_size) override {
    const size_t alloc_size = suggested_size;
    char* base = static_cast<char*>(malloc(alloc_size > 0 ? alloc_size : 1));
    if (base == nullptr) return uv_buf_init(nullptr, 0);
    return uv_buf_init(base, static_cast<unsigned int>(alloc_size));
  }

  void OnRecv(ssize_t nread,
              const uv_buf_t& buf,
              const sockaddr* addr,
              unsigned int flags) override {
    (void)flags;
    if (handle_wrap.env == nullptr) {
      free(buf.base);
      return;
    }
    edge::HandleScope scope(handle_wrap.env);
    if (!scope.is_open()) {
      free(buf.base);
      return;
    }
    if (nread == 0 && addr == nullptr) {
      free(buf.base);
      return;
    }

    napi_value self = GetRefValue(handle_wrap.env, handle_wrap.wrapper_ref);
    if (self == nullptr) {
      free(buf.base);
      return;
    }

    napi_value onmessage = nullptr;
    if (napi_get_named_property(handle_wrap.env, self, "onmessage", &onmessage) != napi_ok ||
        !IsFunction(handle_wrap.env, onmessage)) {
      free(buf.base);
      return;
    }

    napi_value argv[4] = {
        MakeInt32(handle_wrap.env, static_cast<int32_t>(nread)),
        self,
        nullptr,
        nullptr,
    };
    napi_get_undefined(handle_wrap.env, &argv[2]);
    napi_get_undefined(handle_wrap.env, &argv[3]);

    if (nread < 0) {
      napi_value ignored = nullptr;
      (void)EdgeAsyncWrapMakeCallback(
          handle_wrap.env, async_id, self, self, onmessage, 4, argv, &ignored, kEdgeMakeCallbackNone);
      (void)EdgeHandlePendingExceptionNow(handle_wrap.env, nullptr);
      free(buf.base);
      return;
    }

    napi_value rinfo = BuildRinfoObject(handle_wrap.env, addr, nread);
    if (rinfo == nullptr) {
      napi_value error = CreateErrorValue(handle_wrap.env, "Failed to build UDP remote info");
      CallOnError(handle_wrap.env, self, async_id, nread, error);
      free(buf.base);
      return;
    }
    argv[3] = rinfo;

    char* owned = buf.base;
    if (nread > 0 && static_cast<size_t>(nread) != buf.len) {
      char* trimmed = static_cast<char*>(malloc(static_cast<size_t>(nread)));
      if (trimmed == nullptr) {
        napi_value error = CreateErrorValue(handle_wrap.env, "Failed to trim UDP receive buffer");
        CallOnError(handle_wrap.env, self, async_id, nread, error);
        free(owned);
        return;
      }
      std::memcpy(trimmed, owned, static_cast<size_t>(nread));
      free(owned);
      owned = trimmed;
    }

    argv[2] = CreateExternalBuffer(handle_wrap.env, owned, nread > 0 ? static_cast<size_t>(nread) : 0);
    if (argv[2] == nullptr) {
      napi_value error = CreateErrorValue(handle_wrap.env, "Failed to create UDP buffer");
      CallOnError(handle_wrap.env, self, async_id, nread, error);
      return;
    }

    napi_value ignored = nullptr;
    (void)EdgeAsyncWrapMakeCallback(
        handle_wrap.env, async_id, self, self, onmessage, 4, argv, &ignored, kEdgeMakeCallbackNone);
    (void)EdgeHandlePendingExceptionNow(handle_wrap.env, nullptr);
  }

  EdgeUdpSendWrap* CreateSendWrap(size_t msg_size) override {
    if (current_send_req_obj == nullptr) return nullptr;

    SendWrap* req_wrap = nullptr;
    if (napi_unwrap(handle_wrap.env, current_send_req_obj, reinterpret_cast<void**>(&req_wrap)) != napi_ok ||
        req_wrap == nullptr) {
      return nullptr;
    }

    if (req_wrap->active) return nullptr;

    ReleaseSendWrapState(req_wrap);
    req_wrap->env = handle_wrap.env;
    req_wrap->msg_size = msg_size;
    req_wrap->have_callback = current_send_has_callback;
    req_wrap->provider_type = kEdgeProviderUdpSendWrap;
    if (req_wrap->async_id <= 0) {
      req_wrap->async_id = EdgeAsyncWrapNextId(handle_wrap.env);
      EdgeAsyncWrapEmitInit(
          handle_wrap.env,
          req_wrap->async_id,
          req_wrap->provider_type,
          EdgeAsyncWrapExecutionAsyncId(handle_wrap.env),
          current_send_req_obj);
    } else if (req_wrap->destroy_queued) {
      req_wrap->async_id = EdgeAsyncWrapNextId(handle_wrap.env);
      EdgeAsyncWrapEmitInit(
          handle_wrap.env,
          req_wrap->async_id,
          req_wrap->provider_type,
          EdgeAsyncWrapExecutionAsyncId(handle_wrap.env),
          current_send_req_obj);
    }
    req_wrap->destroy_queued = false;
    req_wrap->active = true;
    req_wrap->req.data = req_wrap;
    if (!CreateStrongRef(handle_wrap.env, current_send_req_obj, &req_wrap->active_ref)) {
      ReleaseSendWrapState(req_wrap);
      return nullptr;
    }
    req_wrap->active_request_token =
        EdgeRegisterActiveRequest(handle_wrap.env,
                                  current_send_req_obj,
                                  "SendWrap",
                                  req_wrap,
                                  CancelSendWrapRequest,
                                  SendWrapGetActiveOwner);
    if (current_send_chunks_obj != nullptr &&
        !CreateStrongRef(handle_wrap.env, current_send_chunks_obj, &req_wrap->chunks_ref)) {
      ReleaseSendWrapState(req_wrap);
      return nullptr;
    }
    return req_wrap;
  }

  void OnSendDone(EdgeUdpSendWrap* wrap, int status) override {
    auto* req_wrap = static_cast<SendWrap*>(wrap);
    if (req_wrap == nullptr || req_wrap->env == nullptr) {
      QueueSendWrapDestroyIfNeeded(req_wrap);
      ReleaseSendWrapState(req_wrap);
      return;
    }
    edge::HandleScope scope(req_wrap->env);
    if (!scope.is_open()) {
      QueueSendWrapDestroyIfNeeded(req_wrap);
      ReleaseSendWrapState(req_wrap);
      return;
    }
    napi_value req_obj = req_wrap->object(req_wrap->env);
    if (req_wrap->have_callback && req_obj != nullptr) {
      napi_value argv[2] = {
          MakeInt32(req_wrap->env, status),
          MakeInt32(req_wrap->env, static_cast<int32_t>(req_wrap->msg_size)),
      };
      CallOptionalCallback(req_wrap->env, req_obj, "oncomplete", 2, argv, req_wrap->async_id);
    }
    QueueSendWrapDestroyIfNeeded(req_wrap);
    ReleaseSendWrapState(req_wrap);
  }

  static void OnAllocCallback(uv_handle_t* handle,
                              size_t suggested_size,
                              uv_buf_t* buf) {
    auto* wrap = handle != nullptr ? static_cast<UdpWrap*>(reinterpret_cast<uv_udp_t*>(handle)->data) : nullptr;
    if (wrap == nullptr) {
      *buf = uv_buf_init(nullptr, 0);
      return;
    }
    *buf = wrap->listener()->OnAlloc(suggested_size);
  }

  static void OnRecvCallback(uv_udp_t* handle,
                             ssize_t nread,
                             const uv_buf_t* buf,
                             const sockaddr* addr,
                             unsigned int flags) {
    auto* wrap = handle != nullptr ? static_cast<UdpWrap*>(handle->data) : nullptr;
    if (wrap == nullptr) {
      if (buf != nullptr) free(buf->base);
      return;
    }
    const uv_buf_t recv_buf = buf != nullptr ? *buf : uv_buf_init(nullptr, 0);
    wrap->listener()->OnRecv(nread, recv_buf, addr, flags);
  }

  static void OnSendDoneCallback(uv_udp_send_t* req, int status) {
    auto* req_wrap = static_cast<SendWrap*>(req->data);
    auto* wrap = req->handle != nullptr ? static_cast<UdpWrap*>(req->handle->data) : nullptr;
    if (wrap == nullptr) {
      QueueSendWrapDestroyIfNeeded(req_wrap);
      ReleaseSendWrapState(req_wrap);
      return;
    }
    wrap->listener()->OnSendDone(req_wrap, status);
  }

  EdgeHandleWrap handle_wrap{};
  uv_udp_t handle{};
  int32_t provider_type = kEdgeProviderUdpWrap;
  int64_t async_id = -1;
  napi_value current_send_req_obj = nullptr;
  napi_value current_send_chunks_obj = nullptr;
  bool current_send_has_callback = false;
};

bool UdpHandleHasRef(void* data) {
  auto* wrap = static_cast<UdpWrap*>(data);
  return wrap != nullptr &&
         EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle));
}

napi_value UdpGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<UdpWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

UdpWrap* UnwrapUdpWrap(napi_env env, napi_value value, bool throw_type_error) {
  if (value == nullptr) {
    if (throw_type_error) ThrowInvalidUdpReceiver(env);
    return nullptr;
  }

  UdpWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) {
    if (throw_type_error) ThrowInvalidUdpReceiver(env);
    return nullptr;
  }
  return wrap;
}

napi_value GetThis(napi_env env,
                   napi_callback_info info,
                   size_t* argc_out,
                   napi_value* argv,
                   UdpWrap** wrap_out,
                   bool throw_type_error = false) {
  size_t argc = argc_out != nullptr ? *argc_out : 0;
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (argc_out != nullptr) *argc_out = argc;
  if (wrap_out != nullptr) *wrap_out = UnwrapUdpWrap(env, self, throw_type_error);
  return self;
}

void QueueUdpWrapDestroyIfNeeded(UdpWrap* wrap) {
  if (wrap == nullptr || wrap->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(wrap->handle_wrap.env, wrap->async_id);
  wrap->async_id = -1;
}

void DeleteUdpWrap(void* data) {
  auto* wrap = static_cast<UdpWrap*>(data);
  if (wrap == nullptr) return;
  QueueUdpWrapDestroyIfNeeded(wrap);
  delete wrap;
}

void SendWrapFinalize(napi_env env, void* data, void* hint) {
  (void)hint;
  auto* wrap = static_cast<SendWrap*>(data);
  if (wrap == nullptr) return;
  wrap->env = env != nullptr ? env : wrap->env;
  QueueSendWrapDestroyIfNeeded(wrap);
  ReleaseSendWrapState(wrap);
  DeleteRef(env, &wrap->wrapper_ref);
  delete wrap;
}

void UdpFinalize(napi_env env, void* data, void* hint) {
  (void)hint;
  EdgeHandleWrapFinalizeNative(env, &static_cast<UdpWrap*>(data)->handle_wrap, data, DeleteUdpWrap);
}

napi_value SendWrapCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* wrap = new SendWrap();
  wrap->env = env;
  wrap->async_id = EdgeAsyncWrapNextId(env);
  napi_wrap(env, self, wrap, SendWrapFinalize, nullptr, &wrap->wrapper_ref);
  EdgeAsyncWrapEmitInit(
      env, wrap->async_id, wrap->provider_type, EdgeAsyncWrapExecutionAsyncId(env), self);
  return self;
}

napi_value UdpCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* wrap = new UdpWrap(env);
  napi_wrap(env, self, wrap, UdpFinalize, nullptr, &wrap->handle_wrap.wrapper_ref);
  // EdgeRegisterActiveHandle's keepalive_ref keeps the wrapper alive while the
  // handle is active; no separate pin needed (see EdgeHandleWrapCompleteClose).
  wrap->handle_wrap.active_handle_token =
      EdgeRegisterActiveHandle(
          env, self, "UDPWRAP", UdpHandleHasRef, UdpGetActiveOwner, wrap, CloseUdpWrapForCleanup);
  EdgeAsyncWrapEmitInit(env, wrap->async_id, wrap->provider_type, EdgeAsyncWrapExecutionAsyncId(env), self);

  // Match Node's dgram handle aliasing for udp6 sockets.
  const char* mutable_methods[] = {"bind", "bind6", "connect", "connect6", "send", "send6"};
  for (const char* key : mutable_methods) {
    napi_value fn = nullptr;
    if (napi_get_named_property(env, self, key, &fn) == napi_ok && fn != nullptr) {
      napi_property_descriptor desc = {key,
                                       nullptr,
                                       nullptr,
                                       nullptr,
                                       nullptr,
                                       fn,
                                       static_cast<napi_property_attributes>(napi_writable | napi_configurable),
                                       nullptr};
      napi_define_properties(env, self, 1, &desc);
    }
  }
  return self;
}

void OnClosed(uv_handle_t* h) {
  auto* wrap = static_cast<UdpWrap*>(h->data);
  if (wrap == nullptr) return;
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteUdpWrap);
}

void CloseUdpWrapForCleanup(void* data) {
  auto* wrap = static_cast<UdpWrap*>(data);
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (!uv_is_closing(handle)) {
    wrap->handle_wrap.state = kEdgeHandleClosing;
    uv_close(handle, OnClosed);
  }
}

napi_value UdpBindImpl(napi_env env,
                       UdpWrap* wrap,
                       napi_value ip_value,
                       uint32_t port,
                       bool ipv6,
                       uint32_t flags) {
  if (wrap == nullptr) return MakeInt32(env, UV_EBADF);
  if (wrap->handle_wrap.state != kEdgeHandleInitialized ||
      uv_is_closing(reinterpret_cast<uv_handle_t*>(&wrap->handle))) {
    return MakeInt32(env, UV_EBADF);
  }

  std::string ip = ValueToUtf8(env, ip_value);
  int rc = 0;
  if (ipv6) {
    sockaddr_in6 a6{};
    rc = uv_ip6_addr(ip.c_str(), static_cast<int>(port), &a6);
    if (rc == 0) rc = uv_udp_bind(&wrap->handle, reinterpret_cast<const sockaddr*>(&a6), flags);
  } else {
    sockaddr_in a4{};
    rc = uv_ip4_addr(ip.c_str(), static_cast<int>(port), &a4);
    if (rc == 0) rc = uv_udp_bind(&wrap->handle, reinterpret_cast<const sockaddr*>(&a4), flags);
  }

  if (rc == 0) wrap->listener()->OnAfterBind();
  return MakeInt32(env, rc);
}

napi_value UdpBind(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  uint32_t port = 0;
  napi_get_value_uint32(env, argv[1], &port);
  uint32_t flags = 0;
  if (argc > 2 && argv[2] != nullptr) napi_get_value_uint32(env, argv[2], &flags);
  return UdpBindImpl(env, wrap, argv[0], port, false, flags);
}

napi_value UdpBind6(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  uint32_t port = 0;
  napi_get_value_uint32(env, argv[1], &port);
  uint32_t flags = 0;
  if (argc > 2 && argv[2] != nullptr) napi_get_value_uint32(env, argv[2], &flags);
  return UdpBindImpl(env, wrap, argv[0], port, true, flags);
}

napi_value UdpOpen(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  if (wrap->handle_wrap.state != kEdgeHandleInitialized ||
      uv_is_closing(reinterpret_cast<uv_handle_t*>(&wrap->handle))) {
    return MakeInt32(env, UV_EBADF);
  }
  int32_t fd = -1;
  napi_get_value_int32(env, argv[0], &fd);
  return MakeInt32(env, uv_udp_open(&wrap->handle, static_cast<uv_os_sock_t>(fd)));
}

napi_value UdpRecvStart(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return MakeInt32(env, UV_EBADF);
  return MakeInt32(env, wrap->RecvStart());
}

napi_value UdpRecvStop(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return MakeInt32(env, UV_EBADF);
  return MakeInt32(env, wrap->RecvStop());
}

napi_value UdpClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  napi_value self = GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  if (wrap->handle_wrap.state == kEdgeHandleInitialized && argc > 0 && argv[0] != nullptr) {
    EdgeHandleWrapSetOnCloseCallback(env, self, argv[0]);
  }

  if (wrap->handle_wrap.state == kEdgeHandleInitialized &&
      !uv_is_closing(reinterpret_cast<uv_handle_t*>(&wrap->handle))) {
    CloseUdpWrapForCleanup(wrap);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value UdpSendImpl(napi_env env, napi_callback_info info, bool ipv6) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return MakeInt32(env, UV_EBADF);
  if (argc != 4 && argc != 6) return MakeInt32(env, UV_EINVAL);

  napi_value req_obj = argv[0];
  napi_value chunks = argv[1];
  uint32_t count = 0;
  napi_get_value_uint32(env, argv[2], &count);

  std::vector<uv_buf_t> bufs;
  bufs.reserve(count);
  for (uint32_t i = 0; i < count; i++) {
    napi_value chunk = nullptr;
    if (napi_get_element(env, chunks, i, &chunk) != napi_ok || chunk == nullptr) {
      return MakeInt32(env, UV_EINVAL);
    }
    const char* data = nullptr;
    size_t len = 0;
    if (!ExtractArrayBufferViewBytes(env, chunk, &data, &len)) {
      return MakeInt32(env, UV_EINVAL);
    }
    char* base = const_cast<char*>(data);
    if (base == nullptr && len != 0) return MakeInt32(env, UV_EINVAL);
    bufs.emplace_back(uv_buf_init(base, static_cast<unsigned int>(len)));
  }

  const bool send_to = argc == 6;
  sockaddr_storage addr_storage{};
  const sockaddr* addr = nullptr;
  if (send_to) {
    uint32_t port = 0;
    napi_get_value_uint32(env, argv[3], &port);
    std::string ip = ValueToUtf8(env, argv[4]);
    int rc = 0;
    if (ipv6) {
      auto* a6 = reinterpret_cast<sockaddr_in6*>(&addr_storage);
      rc = uv_ip6_addr(ip.c_str(), static_cast<int>(port), a6);
    } else {
      auto* a4 = reinterpret_cast<sockaddr_in*>(&addr_storage);
      rc = uv_ip4_addr(ip.c_str(), static_cast<int>(port), a4);
    }
    if (rc != 0) return MakeInt32(env, rc);
    addr = reinterpret_cast<const sockaddr*>(&addr_storage);
  }

  bool have_callback = false;
  napi_get_value_bool(env, argv[send_to ? 5 : 3], &have_callback);
  wrap->current_send_req_obj = req_obj;
  wrap->current_send_chunks_obj = chunks;
  wrap->current_send_has_callback = have_callback;
  const ssize_t rc =
      wrap->Send(bufs.empty() ? nullptr : bufs.data(), static_cast<size_t>(count), addr);
  wrap->current_send_req_obj = nullptr;
  wrap->current_send_chunks_obj = nullptr;
  wrap->current_send_has_callback = false;
  return MakeInt32(env, static_cast<int32_t>(rc));
}

napi_value UdpSend(napi_env env, napi_callback_info info) {
  return UdpSendImpl(env, info, false);
}

napi_value UdpSend6(napi_env env, napi_callback_info info) {
  return UdpSendImpl(env, info, true);
}

void FillSockaddrObject(napi_env env, napi_value out, const sockaddr_storage& storage) {
  std::string ip;
  int port = 0;
  const char* family = nullptr;
  if (storage.ss_family == AF_INET6) {
    const auto* a6 = reinterpret_cast<const sockaddr_in6*>(&storage);
    ip = FormatIPv6AddressWithScope(a6);
    port = ntohs(a6->sin6_port);
    family = "IPv6";
  } else if (storage.ss_family == AF_INET) {
    char ip4[INET6_ADDRSTRLEN] = {0};
    const auto* a4 = reinterpret_cast<const sockaddr_in*>(&storage);
    uv_ip4_name(a4, ip4, sizeof(ip4));
    ip = ip4;
    port = ntohs(a4->sin_port);
    family = "IPv4";
  } else {
    return;
  }

  napi_value ip_v = nullptr;
  napi_value family_v = nullptr;
  napi_value port_v = nullptr;
  napi_create_string_utf8(env, ip.c_str(), NAPI_AUTO_LENGTH, &ip_v);
  napi_create_string_utf8(env, family, NAPI_AUTO_LENGTH, &family_v);
  napi_create_int32(env, port, &port_v);
  if (ip_v != nullptr) napi_set_named_property(env, out, "address", ip_v);
  if (family_v != nullptr) napi_set_named_property(env, out, "family", family_v);
  if (port_v != nullptr) napi_set_named_property(env, out, "port", port_v);
}

napi_value UdpGetSockName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  sockaddr_storage storage{};
  int len = sizeof(storage);
  const int rc = uv_udp_getsockname(&wrap->handle, reinterpret_cast<sockaddr*>(&storage), &len);
  if (rc == 0 && argv[0] != nullptr) FillSockaddrObject(env, argv[0], storage);
  return MakeInt32(env, rc);
}

napi_value UdpRef(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value UdpUnref(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value UdpHasRef(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return MakeBool(
      env,
      wrap != nullptr &&
          EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle)));
}

napi_value UdpGetAsyncId(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  napi_value out = nullptr;
  napi_create_int64(env, wrap != nullptr ? wrap->async_id : -1, &out);
  return out;
}

napi_value UdpAsyncReset(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (wrap != nullptr) {
    EdgeAsyncWrapReset(env, &wrap->async_id);
    EdgeAsyncWrapEmitInit(env, wrap->async_id, wrap->provider_type, EdgeAsyncWrapExecutionAsyncId(env), self);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value UdpGetProviderType(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return MakeInt32(env, wrap != nullptr ? wrap->provider_type : kEdgeProviderNone);
}

napi_value UdpFdGetter(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  UdpWrap* wrap = UnwrapUdpWrap(env, self, true);
  if (wrap == nullptr) return nullptr;

  int32_t fd = UV_EBADF;
#if !defined(_WIN32)
  uv_os_fd_t raw = -1;
  if (uv_fileno(reinterpret_cast<const uv_handle_t*>(&wrap->handle), &raw) == 0) {
    fd = static_cast<int32_t>(raw);
  }
#endif
  return MakeInt32(env, fd);
}

napi_value UdpBufferSize(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return MakeInt32(env, UV_EBADF);
  if (argc < 2) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  int32_t size = 0;
  bool recv = false;
  napi_get_value_bool(env, argv[1], &recv);
  const char* syscall = recv ? "uv_recv_buffer_size" : "uv_send_buffer_size";
  if (!GetInt32Like(env, argv[0], &size)) {
    if (argc > 2 && argv[2] != nullptr) PopulateUvExceptionInfo(env, argv[2], UV_EINVAL, syscall);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  int value = size;
  const int rc = recv ? uv_recv_buffer_size(reinterpret_cast<uv_handle_t*>(&wrap->handle), &value)
                      : uv_send_buffer_size(reinterpret_cast<uv_handle_t*>(&wrap->handle), &value);
  if (rc != 0) {
    if (argc > 2 && argv[2] != nullptr) PopulateUvExceptionInfo(env, argv[2], rc, syscall);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  return MakeInt32(env, value);
}

napi_value UdpSetBroadcast(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  bool on = false;
  napi_get_value_bool(env, argv[0], &on);
  return MakeInt32(env, uv_udp_set_broadcast(&wrap->handle, on ? 1 : 0));
}

napi_value UdpSetTTL(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  int32_t ttl = 0;
  napi_get_value_int32(env, argv[0], &ttl);
  return MakeInt32(env, uv_udp_set_ttl(&wrap->handle, ttl));
}

napi_value UdpSetMulticastTTL(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  int32_t ttl = 0;
  napi_get_value_int32(env, argv[0], &ttl);
  return MakeInt32(env, uv_udp_set_multicast_ttl(&wrap->handle, ttl));
}

napi_value UdpSetMulticastLoopback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  bool on = false;
  napi_get_value_bool(env, argv[0], &on);
  return MakeInt32(env, uv_udp_set_multicast_loop(&wrap->handle, on ? 1 : 0));
}

napi_value UdpSetMulticastInterface(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  std::string iface = ValueToUtf8(env, argv[0]);
  return MakeInt32(env, uv_udp_set_multicast_interface(&wrap->handle, iface.c_str()));
}

napi_value UdpMembershipImpl(napi_env env,
                             napi_callback_info info,
                             uv_membership membership) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) {
    return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  }

  std::string multicast = ValueToUtf8(env, argv[0]);
  const char* iface = nullptr;
  std::string iface_storage;
  if (argc > 1 && !IsNullOrUndefined(env, argv[1])) {
    iface_storage = ValueToUtf8(env, argv[1]);
    iface = iface_storage.c_str();
  }
  return MakeInt32(env, uv_udp_set_membership(&wrap->handle, multicast.c_str(), iface, membership));
}

napi_value UdpAddMembership(napi_env env, napi_callback_info info) {
  return UdpMembershipImpl(env, info, UV_JOIN_GROUP);
}

napi_value UdpDropMembership(napi_env env, napi_callback_info info) {
  return UdpMembershipImpl(env, info, UV_LEAVE_GROUP);
}

napi_value UdpSourceMembershipImpl(napi_env env,
                                   napi_callback_info info,
                                   uv_membership membership) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2 || argv[0] == nullptr || argv[1] == nullptr) {
    return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  }

#if UV_VERSION_MAJOR > 1 || (UV_VERSION_MAJOR == 1 && UV_VERSION_MINOR >= 32)
  std::string source = ValueToUtf8(env, argv[0]);
  std::string group = ValueToUtf8(env, argv[1]);
  const char* iface = nullptr;
  std::string iface_storage;
  if (argc > 2 && !IsNullOrUndefined(env, argv[2])) {
    iface_storage = ValueToUtf8(env, argv[2]);
    iface = iface_storage.c_str();
  }
  return MakeInt32(
      env,
      uv_udp_set_source_membership(&wrap->handle, group.c_str(), iface, source.c_str(), membership));
#else
  return MakeInt32(env, UV_ENOTSUP);
#endif
}

napi_value UdpAddSourceSpecificMembership(napi_env env, napi_callback_info info) {
  return UdpSourceMembershipImpl(env, info, UV_JOIN_GROUP);
}

napi_value UdpDropSourceSpecificMembership(napi_env env, napi_callback_info info) {
  return UdpSourceMembershipImpl(env, info, UV_LEAVE_GROUP);
}

napi_value UdpConnectImpl(napi_env env, napi_callback_info info, bool ipv6) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  if (wrap->handle_wrap.state != kEdgeHandleInitialized ||
      uv_is_closing(reinterpret_cast<uv_handle_t*>(&wrap->handle))) {
    return MakeInt32(env, UV_EBADF);
  }

  std::string host = ValueToUtf8(env, argv[0]);
  uint32_t port = 0;
  napi_get_value_uint32(env, argv[1], &port);
  if (ipv6) {
    sockaddr_in6 a6{};
    int rc = uv_ip6_addr(host.c_str(), static_cast<int>(port), &a6);
    if (rc != 0) return MakeInt32(env, rc);
    return MakeInt32(env, uv_udp_connect(&wrap->handle, reinterpret_cast<const sockaddr*>(&a6)));
  }
  sockaddr_in a4{};
  int rc = uv_ip4_addr(host.c_str(), static_cast<int>(port), &a4);
  if (rc != 0) return MakeInt32(env, rc);
  return MakeInt32(env, uv_udp_connect(&wrap->handle, reinterpret_cast<const sockaddr*>(&a4)));
}

napi_value UdpConnect(napi_env env, napi_callback_info info) {
  return UdpConnectImpl(env, info, false);
}

napi_value UdpConnect6(napi_env env, napi_callback_info info) {
  return UdpConnectImpl(env, info, true);
}

napi_value UdpDisconnect(napi_env env, napi_callback_info info) {
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return MakeInt32(env, UV_EBADF);
  return MakeInt32(env, uv_udp_connect(&wrap->handle, nullptr));
}

napi_value UdpGetPeerName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  UdpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return MakeInt32(env, wrap == nullptr ? UV_EBADF : UV_EINVAL);
  sockaddr_storage storage{};
  int len = sizeof(storage);
  const int rc = uv_udp_getpeername(&wrap->handle, reinterpret_cast<sockaddr*>(&storage), &len);
  if (rc == 0 && argv[0] != nullptr) FillSockaddrObject(env, argv[0], storage);
  return MakeInt32(env, rc);
}

napi_value UdpGetSendQueueSize(napi_env env, napi_callback_info info) {
#if UV_VERSION_MAJOR > 1 || (UV_VERSION_MAJOR == 1 && UV_VERSION_MINOR >= 19)
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  napi_value out = nullptr;
  napi_create_double(env,
                     static_cast<double>(wrap != nullptr ? uv_udp_get_send_queue_size(&wrap->handle) : 0),
                     &out);
  return out;
#else
  return MakeInt32(env, 0);
#endif
}

napi_value UdpGetSendQueueCount(napi_env env, napi_callback_info info) {
#if UV_VERSION_MAJOR > 1 || (UV_VERSION_MAJOR == 1 && UV_VERSION_MINOR >= 19)
  UdpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  napi_value out = nullptr;
  napi_create_double(env,
                     static_cast<double>(wrap != nullptr ? uv_udp_get_send_queue_count(&wrap->handle) : 0),
                     &out);
  return out;
#else
  return MakeInt32(env, 0);
#endif
}

SendWrap* UnwrapSendWrap(napi_env env, napi_value value) {
  if (value == nullptr) return nullptr;
  SendWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

napi_value SendWrapGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SendWrap* wrap = UnwrapSendWrap(env, self);
  napi_value out = nullptr;
  napi_create_int64(env, wrap != nullptr ? wrap->async_id : -1, &out);
  return out;
}

napi_value SendWrapAsyncReset(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SendWrap* wrap = UnwrapSendWrap(env, self);
  if (wrap != nullptr) {
    EdgeAsyncWrapReset(env, &wrap->async_id);
    wrap->destroy_queued = false;
    EdgeAsyncWrapEmitInit(env, wrap->async_id, wrap->provider_type, EdgeAsyncWrapExecutionAsyncId(env), self);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value SendWrapGetProviderType(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SendWrap* wrap = UnwrapSendWrap(env, self);
  return MakeInt32(env, wrap != nullptr ? wrap->provider_type : kEdgeProviderNone);
}

void SetNamedU32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value v = nullptr;
  napi_create_uint32(env, value, &v);
  if (v != nullptr) napi_set_named_property(env, obj, key, v);
}

}  // namespace

napi_value EdgeInstallUdpWrapBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor udp_props[] = {
      {"open", nullptr, UdpOpen, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"bind", nullptr, UdpBind, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"bind6", nullptr, UdpBind6, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"send", nullptr, UdpSend, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"send6", nullptr, UdpSend6, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"recvStart", nullptr, UdpRecvStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"recvStop", nullptr, UdpRecvStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getsockname", nullptr, UdpGetSockName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getpeername", nullptr, UdpGetPeerName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, UdpClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setBroadcast", nullptr, UdpSetBroadcast, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setTTL", nullptr, UdpSetTTL, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setMulticastTTL", nullptr, UdpSetMulticastTTL, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setMulticastLoopback",
       nullptr,
       UdpSetMulticastLoopback,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"setMulticastInterface",
       nullptr,
       UdpSetMulticastInterface,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"addMembership", nullptr, UdpAddMembership, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"dropMembership", nullptr, UdpDropMembership, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"addSourceSpecificMembership",
       nullptr,
       UdpAddSourceSpecificMembership,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"dropSourceSpecificMembership",
       nullptr,
       UdpDropSourceSpecificMembership,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"bufferSize", nullptr, UdpBufferSize, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"connect", nullptr, UdpConnect, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"connect6", nullptr, UdpConnect6, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"disconnect", nullptr, UdpDisconnect, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"ref", nullptr, UdpRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"unref", nullptr, UdpUnref, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"hasRef", nullptr, UdpHasRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, UdpGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, UdpAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType",
       nullptr,
       UdpGetProviderType,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
      {"fd", nullptr, nullptr, UdpFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"getSendQueueSize", nullptr, UdpGetSendQueueSize, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getSendQueueCount",
       nullptr,
       UdpGetSendQueueCount,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
  };

  napi_value udp_ctor = nullptr;
  if (napi_define_class(env,
                        "UDP",
                        NAPI_AUTO_LENGTH,
                        UdpCtor,
                        nullptr,
                        sizeof(udp_props) / sizeof(udp_props[0]),
                        udp_props,
                        &udp_ctor) != napi_ok ||
      udp_ctor == nullptr) {
    return nullptr;
  }
  auto& state = EnsureUdpBindingState(env);
  DeleteRef(env, &state.ctor_ref);
  napi_create_reference(env, udp_ctor, 1, &state.ctor_ref);

  napi_property_descriptor send_wrap_props[] = {
      {"getAsyncId", nullptr, SendWrapGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, SendWrapAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType",
       nullptr,
       SendWrapGetProviderType,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
  };

  napi_value send_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "SendWrap",
                        NAPI_AUTO_LENGTH,
                        SendWrapCtor,
                        nullptr,
                        sizeof(send_wrap_props) / sizeof(send_wrap_props[0]),
                        send_wrap_props,
                        &send_wrap_ctor) != napi_ok ||
      send_wrap_ctor == nullptr) {
    return nullptr;
  }

  napi_value constants = nullptr;
  napi_create_object(env, &constants);
  SetNamedU32(env, constants, "UV_UDP_IPV6ONLY", UV_UDP_IPV6ONLY);
  SetNamedU32(env, constants, "UV_UDP_REUSEADDR", UV_UDP_REUSEADDR);
  SetNamedU32(env, constants, "UV_UDP_REUSEPORT", UV_UDP_REUSEPORT);

  napi_set_named_property(env, binding, "UDP", udp_ctor);
  napi_set_named_property(env, binding, "SendWrap", send_wrap_ctor);
  napi_set_named_property(env, binding, "constants", constants);

  return binding;
}

napi_value EdgeGetUdpWrapConstructor(napi_env env) {
  if (env == nullptr) return nullptr;
  auto* state = GetUdpBindingState(env);
  if (state == nullptr || state->ctor_ref == nullptr) return nullptr;
  return GetRefValue(env, state->ctor_ref);
}

uv_handle_t* EdgeUdpWrapGetHandle(napi_env env, napi_value value) {
  UdpWrap* wrap = UnwrapUdpWrap(env, value, false);
  if (wrap == nullptr) return nullptr;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (handle->data != wrap || handle->type != UV_UDP) return nullptr;
  return handle;
}

EdgeUdpSendWrap* EdgeUdpWrapUnwrapSendWrap(napi_env env, napi_value value) {
  return static_cast<EdgeUdpSendWrap*>(UnwrapSendWrap(env, value));
}
