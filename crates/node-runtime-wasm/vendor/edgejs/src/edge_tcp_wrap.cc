#include "edge_tcp_wrap.h"

#include <arpa/inet.h>

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <unordered_map>
#include <unordered_set>

#include <uv.h>

#include "edge_async_wrap.h"
#include "edge_active_resource.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_runtime.h"
#include "edge_stream_base.h"
#include "edge_stream_listener.h"
#include "edge_trace.h"

namespace {

constexpr int kTcpSocket = 0;
constexpr int kTcpServer = 1;

// Unique N-API type tag used to safely identify TcpWrap-backed JS objects
// before downcasting their unwrapped native pointer. See EdgePipeWrapGetStreamBase.
constexpr napi_type_tag kTcpWrapTypeTag = {
    0xa1b2c3d4e5f60789ULL,
    0x1122334455667788ULL,
};

struct TcpWrap;

struct ConnectReqWrap {
  uv_connect_t req{};
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  void* active_request_token = nullptr;
  int64_t async_id = -1;
  bool destroy_queued = false;
  TcpWrap* tcp = nullptr;
};

struct TcpWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
  uv_tcp_t handle{};
  int socket_type = kTcpSocket;
};

struct TcpBindingState {
  explicit TcpBindingState(napi_env env_in) : env(env_in) {}
  ~TcpBindingState() {
    if (tcp_ctor_ref != nullptr) napi_delete_reference(env, tcp_ctor_ref);
    if (connect_wrap_ctor_ref != nullptr) napi_delete_reference(env, connect_wrap_ctor_ref);
    if (tcp_binding_ref != nullptr) napi_delete_reference(env, tcp_binding_ref);
  }

  napi_env env = nullptr;
  napi_ref tcp_ctor_ref = nullptr;
  napi_ref connect_wrap_ctor_ref = nullptr;
  napi_ref tcp_binding_ref = nullptr;
};

TcpBindingState* GetBindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<TcpBindingState>(env, kEdgeEnvironmentSlotTcpBindingState);
}

TcpBindingState& EnsureBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<TcpBindingState>(
      env, kEdgeEnvironmentSlotTcpBindingState);
}

TcpWrap* FromBase(EdgeStreamBase* base) {
  if (base == nullptr) return nullptr;
  return reinterpret_cast<TcpWrap*>(reinterpret_cast<char*>(base) - offsetof(TcpWrap, base));
}

bool TraceNetEnabled() {
  return EDGE_TRACE_ENABLED("EDGE_TRACE_NET");
}

const char* SocketTypeName(int socket_type) {
  return socket_type == kTcpServer ? "server" : "socket";
}

std::string SockaddrToString(const sockaddr* addr) {
  if (addr == nullptr) return "<null>";
  char ip[INET6_ADDRSTRLEN] = {'\0'};
  uint16_t port = 0;
  if (addr->sa_family == AF_INET) {
    const auto* in = reinterpret_cast<const sockaddr_in*>(addr);
    inet_ntop(AF_INET, &in->sin_addr, ip, sizeof(ip));
    port = ntohs(in->sin_port);
  } else if (addr->sa_family == AF_INET6) {
    const auto* in6 = reinterpret_cast<const sockaddr_in6*>(addr);
    inet_ntop(AF_INET6, &in6->sin6_addr, ip, sizeof(ip));
    port = ntohs(in6->sin6_port);
  } else {
    return "family=" + std::to_string(addr->sa_family);
  }
  return std::string(ip[0] != '\0' ? ip : "<invalid>") + ":" + std::to_string(port);
}

uv_handle_t* TcpGetHandle(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  return wrap != nullptr ? reinterpret_cast<uv_handle_t*>(&wrap->handle) : nullptr;
}

uv_stream_t* TcpGetStreamForBase(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  return wrap != nullptr ? reinterpret_cast<uv_stream_t*>(&wrap->handle) : nullptr;
}

void TcpDestroy(EdgeStreamBase* base) {
  delete FromBase(base);
}

void TcpAfterClose(uv_handle_t* handle) {
  auto* wrap = handle != nullptr ? static_cast<TcpWrap*>(handle->data) : nullptr;
  if (wrap == nullptr) return;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp close type=%s handle=%p async_id=%llu\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 static_cast<unsigned long long>(wrap->base.async_id));
  }
  EdgeStreamBaseOnClosed(&wrap->base);
}

const EdgeStreamBaseOps kTcpOps = {
    TcpGetHandle,
    TcpGetStreamForBase,
    TcpAfterClose,
    TcpDestroy,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
};

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

bool IsFunction(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

napi_value ConnectReqGetOwner(napi_env env, void* data) {
  auto* req = static_cast<ConnectReqWrap*>(data);
  if (req == nullptr) return nullptr;
  napi_value owner = GetRefValue(env, req->active_ref);
  if (owner != nullptr) return owner;
  return GetRefValue(env, req->wrapper_ref);
}

void CancelConnectReq(void* data) {
  auto* req = static_cast<ConnectReqWrap*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

void QueueConnectReqDestroyIfNeeded(ConnectReqWrap* req) {
  if (req == nullptr || req->destroy_queued || req->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(req->env, req->async_id);
  req->destroy_queued = true;
}

bool ActivateConnectReq(ConnectReqWrap* req, napi_value req_obj) {
  if (req == nullptr || req->env == nullptr || req_obj == nullptr) return false;
  if (req->async_id <= 0 || req->destroy_queued) {
    req->async_id = EdgeAsyncWrapNextId(req->env);
    EdgeAsyncWrapEmitInit(
        req->env, req->async_id, kEdgeProviderTcpConnectWrap, EdgeAsyncWrapExecutionAsyncId(req->env), req_obj);
  }
  req->destroy_queued = false;
  DeleteRefIfPresent(req->env, &req->active_ref);
  return napi_create_reference(req->env, req_obj, 1, &req->active_ref) == napi_ok && req->active_ref != nullptr;
}

void ReleaseConnectReqState(ConnectReqWrap* req) {
  if (req == nullptr) return;
  if (req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(req->env, req->active_request_token);
    req->active_request_token = nullptr;
  }
  DeleteRefIfPresent(req->env, &req->active_ref);
  req->tcp = nullptr;
  req->req.data = nullptr;
}

void ConnectReqFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* req = static_cast<ConnectReqWrap*>(data);
  if (req == nullptr) return;
  if (req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(env, req->active_request_token);
    req->active_request_token = nullptr;
  }
  QueueConnectReqDestroyIfNeeded(req);
  DeleteRefIfPresent(env, &req->active_ref);
  DeleteRefIfPresent(env, &req->wrapper_ref);
  delete req;
}

napi_value GetThis(napi_env env,
                   napi_callback_info info,
                   size_t* argc_out,
                   napi_value* argv,
                   TcpWrap** wrap_out) {
  size_t argc = argc_out != nullptr ? *argc_out : 0;
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (argc_out != nullptr) *argc_out = argc;
  if (wrap_out != nullptr) {
    *wrap_out = nullptr;
    if (self != nullptr) {
      napi_unwrap(env, self, reinterpret_cast<void**>(wrap_out));
    }
  }
  return self;
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

void FillSockAddr(napi_env env, napi_value out, const sockaddr* sa) {
  if (env == nullptr || out == nullptr || sa == nullptr) return;
  char ip[INET6_ADDRSTRLEN] = {0};
  const char* family = "IPv4";
  int port = 0;
  if (sa->sa_family == AF_INET6) {
    family = "IPv6";
    const auto* a6 = reinterpret_cast<const sockaddr_in6*>(sa);
    uv_ip6_name(a6, ip, sizeof(ip));
    port = ntohs(a6->sin6_port);
  } else {
    const auto* a4 = reinterpret_cast<const sockaddr_in*>(sa);
    uv_ip4_name(a4, ip, sizeof(ip));
    port = ntohs(a4->sin_port);
  }
  napi_value address = nullptr;
  napi_value family_value = nullptr;
  napi_value port_value = nullptr;
  napi_create_string_utf8(env, ip, NAPI_AUTO_LENGTH, &address);
  napi_create_string_utf8(env, family, NAPI_AUTO_LENGTH, &family_value);
  napi_create_int32(env, port, &port_value);
  if (address != nullptr) napi_set_named_property(env, out, "address", address);
  if (family_value != nullptr) napi_set_named_property(env, out, "family", family_value);
  if (port_value != nullptr) napi_set_named_property(env, out, "port", port_value);
}

void OnAlloc(uv_handle_t* handle, size_t suggested_size, uv_buf_t* buf) {
  auto* wrap = handle != nullptr ? static_cast<TcpWrap*>(handle->data) : nullptr;
  EdgeStreamBaseOnUvAlloc(wrap != nullptr ? &wrap->base : nullptr, suggested_size, buf);
}

void OnRead(uv_stream_t* stream, ssize_t nread, const uv_buf_t* buf) {
  auto* wrap = stream != nullptr ? static_cast<TcpWrap*>(stream->data) : nullptr;
  EdgeStreamBaseOnUvRead(wrap != nullptr ? &wrap->base : nullptr, nread, buf);
}

void OnConnectDone(uv_connect_t* req, int status) {
  auto* cr = static_cast<ConnectReqWrap*>(req->data);
  if (cr == nullptr) return;
  if (cr->env == nullptr) {
    ReleaseConnectReqState(cr);
    return;
  }
  edge::HandleScope scope(cr->env);
  if (!scope.is_open()) {
    QueueConnectReqDestroyIfNeeded(cr);
    ReleaseConnectReqState(cr);
    return;
  }
  napi_value req_obj = ConnectReqGetOwner(cr->env, cr);
  napi_value tcp_obj = cr->tcp != nullptr ? EdgeStreamBaseGetWrapper(&cr->tcp->base) : nullptr;
  bool readable = false;
  bool writable = false;
  if (status == 0 && cr->tcp != nullptr) {
    uv_stream_t* stream = reinterpret_cast<uv_stream_t*>(&cr->tcp->handle);
    readable = uv_is_readable(stream) != 0;
    writable = uv_is_writable(stream) != 0;
  }
  napi_value argv[5] = {
      EdgeStreamBaseMakeInt32(cr->env, status),
      tcp_obj,
      req_obj,
      EdgeStreamBaseMakeBool(cr->env, readable),
      EdgeStreamBaseMakeBool(cr->env, writable),
  };
  if (req_obj != nullptr) {
    EdgeStreamBaseSetReqError(cr->env, req_obj, status);
    if (auto* environment = EdgeEnvironmentGet(cr->env);
        environment == nullptr || environment->can_call_into_js()) {
      napi_value oncomplete = nullptr;
      if (napi_get_named_property(cr->env, req_obj, "oncomplete", &oncomplete) == napi_ok && IsFunction(cr->env, oncomplete)) {
        napi_value ignored = nullptr;
        (void)EdgeAsyncWrapMakeCallback(
            cr->env, cr->async_id, req_obj, req_obj, oncomplete, 5, argv, &ignored, kEdgeMakeCallbackNone);
      }
    }
  }
  QueueConnectReqDestroyIfNeeded(cr);
  ReleaseConnectReqState(cr);
}

napi_value TcpCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (self == nullptr) return nullptr;

  int32_t socket_type = kTcpSocket;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_int32(env, argv[0], &socket_type);
  }

  auto* wrap = new TcpWrap();
  wrap->env = env;
  wrap->socket_type = socket_type;
  uv_loop_t* loop = EdgeGetEnvLoop(env);
  if (loop == nullptr || uv_tcp_init(loop, &wrap->handle) != 0) {
    delete wrap;
    return EdgeStreamBaseUndefined(env);
  }
  wrap->handle.data = wrap;
  const int32_t provider = (socket_type == kTcpServer) ? kEdgeProviderTcpServerWrap : kEdgeProviderTcpWrap;
  EdgeStreamBaseInit(&wrap->base, env, &kTcpOps, provider);
  napi_wrap(env, self, wrap, [](napi_env finalize_env, void* data, void*) {
    auto* tcp_wrap = static_cast<TcpWrap*>(data);
    if (tcp_wrap == nullptr) return;
    EdgeStreamBaseFinalize(&tcp_wrap->base);
  }, nullptr, &wrap->base.wrapper_ref);
  (void)napi_type_tag_object(env, self, &kTcpWrapTypeTag);
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->base.wrapper_ref);
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, true, true);
  return self;
}

napi_value TcpOpen(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t fd = -1;
  napi_get_value_int32(env, argv[0], &fd);
  return EdgeStreamBaseMakeInt32(env, uv_tcp_open(&wrap->handle, static_cast<uv_os_sock_t>(fd)));
}

napi_value TcpSetBlocking(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool on = false;
  napi_get_value_bool(env, argv[0], &on);
  return EdgeStreamBaseMakeInt32(env,
                                uv_stream_set_blocking(reinterpret_cast<uv_stream_t*>(&wrap->handle), on ? 1 : 0));
}

napi_value TcpBind(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  std::string host = ValueToUtf8(env, argv[0]);
  int32_t port = 0;
  napi_get_value_int32(env, argv[1], &port);
  unsigned int flags = 0;
  if (argc > 2 && argv[2] != nullptr) {
    uint32_t tmp = 0;
    napi_get_value_uint32(env, argv[2], &tmp);
    flags = tmp;
#ifdef UV_TCP_IPV6ONLY
    flags &= ~UV_TCP_IPV6ONLY;
#endif
  }
  sockaddr_in addr{};
  int rc = uv_ip4_addr(host.c_str(), port, &addr);
  if (rc == 0) rc = uv_tcp_bind(&wrap->handle, reinterpret_cast<const sockaddr*>(&addr), flags);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp bind4 type=%s handle=%p host=%s port=%d flags=%u rc=%d(%s)\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 host.c_str(),
                 port,
                 flags,
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc));
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpBind6(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  std::string host = ValueToUtf8(env, argv[0]);
  int32_t port = 0;
  napi_get_value_int32(env, argv[1], &port);
  unsigned int flags = 0;
  if (argc > 2 && argv[2] != nullptr) {
    uint32_t tmp = 0;
    napi_get_value_uint32(env, argv[2], &tmp);
    flags = tmp;
  }
  sockaddr_in6 addr{};
  int rc = uv_ip6_addr(host.c_str(), port, &addr);
  if (rc == 0) rc = uv_tcp_bind(&wrap->handle, reinterpret_cast<const sockaddr*>(&addr), flags);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp bind6 type=%s handle=%p host=%s port=%d flags=%u rc=%d(%s)\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 host.c_str(),
                 port,
                 flags,
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc));
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

void OnConnection(uv_stream_t* server, int status) {
  auto* server_wrap = server != nullptr ? static_cast<TcpWrap*>(server->data) : nullptr;
  if (server_wrap == nullptr) return;
  napi_env env = server_wrap->env;
  if (env == nullptr) return;
  edge::HandleScope scope(env);
  if (!scope.is_open()) return;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp onconnection server=%p status=%d(%s) async_id=%llu\n",
                 static_cast<void*>(server),
                 status,
                 status == 0 ? "OK" : uv_err_name(status),
                 static_cast<unsigned long long>(server_wrap->base.async_id));
  }
  napi_value server_obj = EdgeStreamBaseGetWrapper(&server_wrap->base);
  napi_value onconnection = nullptr;
  if (server_obj == nullptr ||
      napi_get_named_property(env, server_obj, "onconnection", &onconnection) != napi_ok ||
      !IsFunction(env, onconnection)) {
    return;
  }

  napi_value argv[2] = {EdgeStreamBaseMakeInt32(env, status), EdgeStreamBaseUndefined(env)};
  if (status == 0) {
    napi_value ctor = EdgeGetTcpWrapConstructor(env);
    napi_value arg = EdgeStreamBaseMakeInt32(env, kTcpSocket);
    napi_value client_obj = nullptr;
    if (ctor == nullptr ||
        arg == nullptr ||
        napi_new_instance(env, ctor, 1, &arg, &client_obj) != napi_ok ||
        client_obj == nullptr) {
      return;
    }
    TcpWrap* client_wrap = nullptr;
    if (napi_unwrap(env, client_obj, reinterpret_cast<void**>(&client_wrap)) != napi_ok || client_wrap == nullptr) {
      return;
    }
    int accept_rc = uv_accept(server, reinterpret_cast<uv_stream_t*>(&client_wrap->handle));
    if (TraceNetEnabled()) {
      sockaddr_storage peer{};
      int peer_len = sizeof(peer);
      int peer_rc = accept_rc == 0
                        ? uv_tcp_getpeername(&client_wrap->handle,
                                             reinterpret_cast<sockaddr*>(&peer),
                                             &peer_len)
                        : accept_rc;
      std::fprintf(stderr,
                   "EDGE_TRACE_NET tcp accept server=%p client=%p rc=%d(%s) peer_rc=%d(%s) peer=%s\n",
                   static_cast<void*>(server),
                   static_cast<void*>(&client_wrap->handle),
                   accept_rc,
                   accept_rc == 0 ? "OK" : uv_err_name(accept_rc),
                   peer_rc,
                   peer_rc == 0 ? "OK" : uv_err_name(peer_rc),
                   peer_rc == 0 ? SockaddrToString(reinterpret_cast<sockaddr*>(&peer)).c_str() : "<none>");
    }
    if (accept_rc != 0) {
      return;
    }
    argv[1] = client_obj;
  }

  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(env,
                           server_wrap->base.async_id,
                           server_obj,
                           server_obj,
                           onconnection,
                           2,
                           argv,
                           &ignored,
                           kEdgeMakeCallbackNone);
}

napi_value TcpListen(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t backlog = 511;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &backlog);
  int rc = uv_listen(reinterpret_cast<uv_stream_t*>(&wrap->handle), backlog, OnConnection);
  if (TraceNetEnabled()) {
    sockaddr_storage name{};
    int name_len = sizeof(name);
    int name_rc = uv_tcp_getsockname(&wrap->handle, reinterpret_cast<sockaddr*>(&name), &name_len);
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp listen type=%s handle=%p backlog=%d rc=%d(%s) sock_rc=%d(%s) sock=%s\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 backlog,
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc),
                 name_rc,
                 name_rc == 0 ? "OK" : uv_err_name(name_rc),
                 name_rc == 0 ? SockaddrToString(reinterpret_cast<sockaddr*>(&name)).c_str() : "<none>");
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpConnectImpl(napi_env env,
                         TcpWrap* wrap,
                         napi_value req_obj,
                         const std::string& host,
                         int32_t port,
                         bool ipv6) {
  auto* cr = static_cast<ConnectReqWrap*>(nullptr);
  if (req_obj == nullptr || napi_unwrap(env, req_obj, reinterpret_cast<void**>(&cr)) != napi_ok || cr == nullptr) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (wrap->base.closed || wrap->base.closing || uv_is_closing(handle)) {
    EdgeStreamBaseSetReqError(env, req_obj, UV_ECANCELED);
    return EdgeStreamBaseMakeInt32(env, UV_ECANCELED);
  }
  cr->env = env;
  cr->tcp = wrap;
  if (!ActivateConnectReq(cr, req_obj)) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  cr->req.data = cr;
  cr->active_request_token =
      EdgeRegisterActiveRequest(env,
                                req_obj,
                                "TCPConnectWrap",
                                cr,
                                CancelConnectReq,
                                ConnectReqGetOwner);

  int rc = 0;
  if (ipv6) {
    sockaddr_in6 addr6{};
    rc = uv_ip6_addr(host.c_str(), port, &addr6);
    if (rc == 0) {
      rc = uv_tcp_connect(&cr->req, &wrap->handle, reinterpret_cast<const sockaddr*>(&addr6), OnConnectDone);
    }
  } else {
    sockaddr_in addr4{};
    rc = uv_ip4_addr(host.c_str(), port, &addr4);
    if (rc == 0) {
      rc = uv_tcp_connect(&cr->req, &wrap->handle, reinterpret_cast<const sockaddr*>(&addr4), OnConnectDone);
    }
  }

  if (rc != 0) {
    EdgeStreamBaseSetReqError(env, req_obj, rc);
    QueueConnectReqDestroyIfNeeded(cr);
    ReleaseConnectReqState(cr);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpConnect(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t port = -1;
  if (argc > 2 && argv[2] != nullptr) napi_get_value_int32(env, argv[2], &port);
  return TcpConnectImpl(env, wrap, argv[0], ValueToUtf8(env, argv[1]), port, false);
}

napi_value TcpConnect6(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 3) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t port = 0;
  napi_get_value_int32(env, argv[2], &port);
  return TcpConnectImpl(env, wrap, argv[0], ValueToUtf8(env, argv[1]), port, true);
}

napi_value TcpShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeLibuvStreamShutdown(&wrap->base, argv[0]);
}

napi_value TcpClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseUndefined(env);
  return EdgeStreamBaseClose(&wrap->base, argc > 0 ? argv[0] : nullptr);
}

napi_value TcpReadStart(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int rc = uv_read_start(reinterpret_cast<uv_stream_t*>(&wrap->handle), OnAlloc, OnRead);
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, rc == 0);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp read_start type=%s handle=%p rc=%d(%s) async_id=%llu\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc),
                 static_cast<unsigned long long>(wrap->base.async_id));
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpReadStop(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int rc = uv_read_stop(reinterpret_cast<uv_stream_t*>(&wrap->handle));
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, false);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp read_stop type=%s handle=%p rc=%d(%s) async_id=%llu\n",
                 SocketTypeName(wrap->socket_type),
                 static_cast<void*>(&wrap->handle),
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc),
                 static_cast<unsigned long long>(wrap->base.async_id));
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeLibuvStreamWriteBuffer(&wrap->base, argv[0], argv[1], nullptr, nullptr);
}

napi_value TcpWriteString(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  napi_value self = nullptr;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, &data);
  TcpWrap* wrap = nullptr;
  if (self != nullptr) {
    napi_unwrap(env, self, reinterpret_cast<void**>(&wrap));
  }
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeLibuvStreamWriteString(&wrap->base,
                                   argv[0],
                                   argv[1],
                                   static_cast<const char*>(data),
                                   nullptr,
                                   nullptr);
}

napi_value TcpWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool all_buffers = false;
  if (argc > 2 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &all_buffers);
  return EdgeLibuvStreamWriteV(&wrap->base, argv[0], argv[1], all_buffers, nullptr, nullptr);
}

napi_value TcpSetNoDelay(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool on = false;
  napi_get_value_bool(env, argv[0], &on);
  return EdgeStreamBaseMakeInt32(env, uv_tcp_nodelay(&wrap->handle, on ? 1 : 0));
}

napi_value TcpSetKeepAlive(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool on = false;
  int32_t delay = 0;
  napi_get_value_bool(env, argv[0], &on);
  napi_get_value_int32(env, argv[1], &delay);
  return EdgeStreamBaseMakeInt32(env,
                                uv_tcp_keepalive(&wrap->handle, on ? 1 : 0, static_cast<unsigned int>(delay)));
}

napi_value TcpRef(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseRef(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value TcpUnref(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseUnref(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value TcpHasRef(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseHasRefValue(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpGetAsyncId(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetAsyncId(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpGetProviderType(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetProviderType(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpAsyncReset(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseAsyncReset(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpReset(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (wrap->base.closed || wrap->base.closing || uv_is_closing(handle)) {
    return EdgeStreamBaseMakeInt32(env, 0);
  }
  int rc = uv_tcp_close_reset(&wrap->handle, TcpAfterClose);
  if (rc == 0) {
    wrap->base.closing = true;
    if (argc > 0) EdgeStreamBaseSetCloseCallback(&wrap->base, argv[0]);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeStreamBaseUseUserBuffer(&wrap->base, argv[0]);
}

napi_value TcpGetSockName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  sockaddr_storage storage{};
  int len = sizeof(storage);
  int rc = uv_tcp_getsockname(&wrap->handle, reinterpret_cast<sockaddr*>(&storage), &len);
  if (rc == 0) FillSockAddr(env, argv[0], reinterpret_cast<const sockaddr*>(&storage));
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpGetPeerName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  sockaddr_storage storage{};
  int len = sizeof(storage);
  int rc = uv_tcp_getpeername(&wrap->handle, reinterpret_cast<sockaddr*>(&storage), &len);
  if (rc == 0) FillSockAddr(env, argv[0], reinterpret_cast<const sockaddr*>(&storage));
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TcpGetOnRead(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetOnRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpSetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TcpWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  return EdgeStreamBaseSetOnRead(wrap != nullptr ? &wrap->base : nullptr, argc > 0 ? argv[0] : nullptr);
}

napi_value TcpBytesReadGetter(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpBytesWrittenGetter(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesWritten(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpFdGetter(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetFd(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpExternalStreamGetter(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetExternal(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TcpWriteQueueSizeGetter(napi_env env, napi_callback_info info) {
  TcpWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetWriteQueueSize(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value ConnectWrapCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (self == nullptr) return nullptr;
  auto* req = new ConnectReqWrap();
  req->env = env;
  req->async_id = EdgeAsyncWrapNextId(env);
  if (napi_wrap(env, self, req, ConnectReqFinalize, nullptr, &req->wrapper_ref) != napi_ok) {
    delete req;
    return nullptr;
  }
  EdgeAsyncWrapEmitInit(
      env, req->async_id, kEdgeProviderTcpConnectWrap, EdgeAsyncWrapExecutionAsyncId(env), self);
  return self;
}

void SetNamedU32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value out = nullptr;
  napi_create_uint32(env, value, &out);
  if (out != nullptr) napi_set_named_property(env, obj, key, out);
}

}  // namespace

uv_stream_t* EdgeTcpWrapGetStream(napi_env env, napi_value value) {
  EdgeStreamBase* base = EdgeTcpWrapGetStreamBase(env, value);
  return base != nullptr ? reinterpret_cast<uv_stream_t*>(&FromBase(base)->handle) : nullptr;
}

EdgeStreamBase* EdgeTcpWrapGetStreamBase(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  napi_valuetype type = napi_undefined;
  napi_status type_status = napi_typeof(env, value, &type);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp get_stream_base value=%p type_status=%d type=%d\n",
                 static_cast<void*>(value),
                 static_cast<int>(type_status),
                 static_cast<int>(type));
  }
  if (type_status != napi_ok ||
      (type != napi_object && type != napi_function && type != napi_external)) {
    return nullptr;
  }
  // See EdgePipeWrapGetStreamBase: verify the type tag before downcasting the
  // unwrapped native pointer to avoid an out-of-bounds read on other wrap types.
  bool is_tcp = false;
  if (napi_check_object_type_tag(env, value, &kTcpWrapTypeTag, &is_tcp) != napi_ok || !is_tcp) {
    return nullptr;
  }
  TcpWrap* wrap = nullptr;
  napi_status unwrap_status = napi_unwrap(env, value, reinterpret_cast<void**>(&wrap));
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp get_stream_base unwrap_status=%d wrap=%p\n",
                 static_cast<int>(unwrap_status),
                 static_cast<void*>(wrap));
  }
  if (unwrap_status != napi_ok || wrap == nullptr) return nullptr;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET tcp get_stream_base base=%p handle=%p data=%p type=%d expected=%d\n",
                 static_cast<void*>(&wrap->base),
                 static_cast<void*>(handle),
                 handle != nullptr ? handle->data : nullptr,
                 handle != nullptr ? static_cast<int>(handle->type) : -1,
                 static_cast<int>(UV_TCP));
  }
  if (handle->data != wrap || handle->type != UV_TCP) return nullptr;
  return &wrap->base;
}

bool EdgeTcpWrapPushStreamListener(uv_stream_t* stream, EdgeStreamListener* listener) {
  if (stream == nullptr || listener == nullptr) return false;
  auto* wrap = static_cast<TcpWrap*>(stream->data);
  return wrap != nullptr && EdgeStreamBasePushListener(&wrap->base, listener);
}

bool EdgeTcpWrapRemoveStreamListener(uv_stream_t* stream, EdgeStreamListener* listener) {
  if (stream == nullptr || listener == nullptr) return false;
  auto* wrap = static_cast<TcpWrap*>(stream->data);
  return wrap != nullptr && EdgeStreamBaseRemoveListener(&wrap->base, listener);
}

napi_value EdgeGetTcpWrapConstructor(napi_env env) {
  TcpBindingState* state = GetBindingState(env);
  napi_value ctor = state != nullptr ? GetRefValue(env, state->tcp_ctor_ref) : nullptr;
  if (ctor != nullptr) return ctor;
  napi_value binding = EdgeInstallTcpWrapBinding(env);
  if (binding == nullptr) return nullptr;
  if (napi_get_named_property(env, binding, "TCP", &ctor) != napi_ok) return nullptr;
  return ctor;
}

napi_value EdgeInstallTcpWrapBinding(napi_env env) {
  TcpBindingState& state = EnsureBindingState(env);
  napi_value cached = GetRefValue(env, state.tcp_binding_ref);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor tcp_props[] = {
      {"open", nullptr, TcpOpen, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setBlocking", nullptr, TcpSetBlocking, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"bind", nullptr, TcpBind, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"bind6", nullptr, TcpBind6, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"listen", nullptr, TcpListen, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"connect", nullptr, TcpConnect, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"connect6", nullptr, TcpConnect6, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"shutdown", nullptr, TcpShutdown, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, TcpClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStart", nullptr, TcpReadStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStop", nullptr, TcpReadStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeBuffer", nullptr, TcpWriteBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writev", nullptr, TcpWritev, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeLatin1String", nullptr, TcpWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("latin1")},
      {"writeUtf8String", nullptr, TcpWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("utf8")},
      {"writeAsciiString", nullptr, TcpWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("ascii")},
      {"writeUcs2String", nullptr, TcpWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("ucs2")},
      {"setNoDelay", nullptr, TcpSetNoDelay, nullptr, nullptr, nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"setKeepAlive", nullptr, TcpSetKeepAlive, nullptr, nullptr, nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"getsockname", nullptr, TcpGetSockName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getpeername", nullptr, TcpGetPeerName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"ref", nullptr, TcpRef, nullptr, nullptr, nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"unref", nullptr, TcpUnref, nullptr, nullptr, nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"hasRef", nullptr, TcpHasRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, TcpGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType", nullptr, TcpGetProviderType, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, TcpAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"reset", nullptr, TcpReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"useUserBuffer", nullptr, TcpUseUserBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onread", nullptr, nullptr, TcpGetOnRead, TcpSetOnRead, nullptr, napi_default, nullptr},
      {"bytesRead", nullptr, nullptr, TcpBytesReadGetter, nullptr, nullptr, napi_default, nullptr},
      {"bytesWritten", nullptr, nullptr, TcpBytesWrittenGetter, nullptr, nullptr, napi_default, nullptr},
      {"fd", nullptr, nullptr, TcpFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"_externalStream", nullptr, nullptr, TcpExternalStreamGetter, nullptr, nullptr, napi_default, nullptr},
      {"writeQueueSize", nullptr, nullptr, TcpWriteQueueSizeGetter, nullptr, nullptr, napi_default, nullptr},
  };

  napi_value tcp_ctor = nullptr;
  if (napi_define_class(env,
                        "TCP",
                        NAPI_AUTO_LENGTH,
                        TcpCtor,
                        nullptr,
                        sizeof(tcp_props) / sizeof(tcp_props[0]),
                        tcp_props,
                        &tcp_ctor) != napi_ok ||
      tcp_ctor == nullptr) {
    return nullptr;
  }
  if (state.tcp_ctor_ref != nullptr) napi_delete_reference(env, state.tcp_ctor_ref);
  napi_create_reference(env, tcp_ctor, 1, &state.tcp_ctor_ref);

  napi_value connect_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "TCPConnectWrap",
                        NAPI_AUTO_LENGTH,
                        ConnectWrapCtor,
                        nullptr,
                        0,
                        nullptr,
                        &connect_wrap_ctor) != napi_ok ||
      connect_wrap_ctor == nullptr) {
    return nullptr;
  }
  if (state.connect_wrap_ctor_ref != nullptr) napi_delete_reference(env, state.connect_wrap_ctor_ref);
  napi_create_reference(env, connect_wrap_ctor, 1, &state.connect_wrap_ctor_ref);

  napi_value constants = nullptr;
  napi_create_object(env, &constants);
  SetNamedU32(env, constants, "SOCKET", kTcpSocket);
  SetNamedU32(env, constants, "SERVER", kTcpServer);
  SetNamedU32(env, constants, "UV_TCP_IPV6ONLY", static_cast<uint32_t>(UV_TCP_IPV6ONLY));
  SetNamedU32(env, constants, "UV_TCP_REUSEPORT", static_cast<uint32_t>(UV_TCP_REUSEPORT));

  napi_set_named_property(env, binding, "TCP", tcp_ctor);
  napi_set_named_property(env, binding, "TCPConnectWrap", connect_wrap_ctor);
  napi_set_named_property(env, binding, "constants", constants);

  if (state.tcp_binding_ref != nullptr) napi_delete_reference(env, state.tcp_binding_ref);
  napi_create_reference(env, binding, 1, &state.tcp_binding_ref);
  return binding;
}
