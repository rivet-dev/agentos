#include "edge_pipe_wrap.h"

#include <cstddef>
#include <cstdint>
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
#include "edge_tcp_wrap.h"
#include "edge_udp_wrap.h"

namespace {

constexpr int kPipeSocket = 0;
constexpr int kPipeServer = 1;
constexpr int kPipeIPC = 2;

// Unique N-API type tag used to safely identify PipeWrap-backed JS objects
// before downcasting their unwrapped native pointer. Without this, napi_unwrap
// returns the native pointer of *any* wrapped object (e.g. a JsStreamWrap),
// and reading PipeWrap-specific fields off it is an out-of-bounds read.
constexpr napi_type_tag kPipeWrapTypeTag = {
    0xe1c4a2b3d5f60718ULL,
    0x2938475665748392ULL,
};

struct PipeWrap;

struct PipeConnectReqWrap {
  uv_connect_t req{};
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  void* active_request_token = nullptr;
  int64_t async_id = -1;
  bool destroy_queued = false;
  PipeWrap* pipe = nullptr;
};

struct PipeBindingState {
  explicit PipeBindingState(napi_env env_in) : env(env_in) {}
  ~PipeBindingState() {
    if (pipe_ctor_ref != nullptr) napi_delete_reference(env, pipe_ctor_ref);
    if (binding_ref != nullptr) napi_delete_reference(env, binding_ref);
  }

  napi_env env = nullptr;
  napi_ref pipe_ctor_ref = nullptr;
  napi_ref binding_ref = nullptr;
};

struct PipeWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
  uv_pipe_t handle{};
  int socket_type = kPipeSocket;
  bool ipc = false;
};

PipeBindingState* GetBindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<PipeBindingState>(env, kEdgeEnvironmentSlotPipeBindingState);
}

PipeBindingState& EnsureBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<PipeBindingState>(
      env, kEdgeEnvironmentSlotPipeBindingState);
}

PipeWrap* FromBase(EdgeStreamBase* base) {
  if (base == nullptr) return nullptr;
  return reinterpret_cast<PipeWrap*>(reinterpret_cast<char*>(base) - offsetof(PipeWrap, base));
}

uv_handle_t* PipeGetHandle(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  return wrap != nullptr ? reinterpret_cast<uv_handle_t*>(&wrap->handle) : nullptr;
}

uv_stream_t* PipeGetStreamForBase(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  return wrap != nullptr ? reinterpret_cast<uv_stream_t*>(&wrap->handle) : nullptr;
}

void PipeDestroy(EdgeStreamBase* base) {
  delete FromBase(base);
}

napi_value AcceptPendingHandleForIpc(EdgeStreamBase* base);

void PipeAfterClose(uv_handle_t* handle) {
  auto* wrap = handle != nullptr ? static_cast<PipeWrap*>(handle->data) : nullptr;
  if (wrap == nullptr) return;
  EdgeStreamBaseOnClosed(&wrap->base);
}

const EdgeStreamBaseOps kPipeOps = {
    PipeGetHandle,
    PipeGetStreamForBase,
    PipeAfterClose,
    PipeDestroy,
    AcceptPendingHandleForIpc,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
};

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
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

napi_value PipeConnectReqGetOwner(napi_env env, void* data) {
  auto* req = static_cast<PipeConnectReqWrap*>(data);
  if (req == nullptr) return nullptr;
  napi_value owner = GetRefValue(env, req->active_ref);
  if (owner != nullptr) return owner;
  return GetRefValue(env, req->wrapper_ref);
}

void CancelPipeConnectReq(void* data) {
  auto* req = static_cast<PipeConnectReqWrap*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

void QueuePipeConnectReqDestroyIfNeeded(PipeConnectReqWrap* req) {
  if (req == nullptr || req->destroy_queued || req->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(req->env, req->async_id);
  req->destroy_queued = true;
}

bool ActivatePipeConnectReq(PipeConnectReqWrap* req, napi_value req_obj) {
  if (req == nullptr || req->env == nullptr || req_obj == nullptr) return false;
  if (req->async_id <= 0 || req->destroy_queued) {
    req->async_id = EdgeAsyncWrapNextId(req->env);
    EdgeAsyncWrapEmitInit(
        req->env, req->async_id, kEdgeProviderPipeConnectWrap, EdgeAsyncWrapExecutionAsyncId(req->env), req_obj);
  }
  req->destroy_queued = false;
  DeleteRefIfPresent(req->env, &req->active_ref);
  return napi_create_reference(req->env, req_obj, 1, &req->active_ref) == napi_ok && req->active_ref != nullptr;
}

void ReleasePipeConnectReqState(PipeConnectReqWrap* req) {
  if (req == nullptr) return;
  if (req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(req->env, req->active_request_token);
    req->active_request_token = nullptr;
  }
  DeleteRefIfPresent(req->env, &req->active_ref);
  req->pipe = nullptr;
  req->req.data = nullptr;
}

void PipeConnectReqFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* req = static_cast<PipeConnectReqWrap*>(data);
  if (req == nullptr) return;
  if (req->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(env, req->active_request_token);
    req->active_request_token = nullptr;
  }
  QueuePipeConnectReqDestroyIfNeeded(req);
  DeleteRefIfPresent(env, &req->active_ref);
  DeleteRefIfPresent(env, &req->wrapper_ref);
  delete req;
}

napi_value GetThis(napi_env env,
                   napi_callback_info info,
                   size_t* argc_out,
                   napi_value* argv,
                   PipeWrap** wrap_out) {
  size_t argc = argc_out != nullptr ? *argc_out : 0;
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (argc_out != nullptr) *argc_out = argc;
  if (wrap_out != nullptr) {
    *wrap_out = nullptr;
    if (self != nullptr) napi_unwrap(env, self, reinterpret_cast<void**>(wrap_out));
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

void OnAlloc(uv_handle_t* handle, size_t suggested_size, uv_buf_t* buf) {
  auto* wrap = handle != nullptr ? static_cast<PipeWrap*>(handle->data) : nullptr;
  EdgeStreamBaseOnUvAlloc(wrap != nullptr ? &wrap->base : nullptr, suggested_size, buf);
}

void OnRead(uv_stream_t* stream, ssize_t nread, const uv_buf_t* buf) {
  auto* wrap = stream != nullptr ? static_cast<PipeWrap*>(stream->data) : nullptr;
  EdgeStreamBaseOnUvRead(wrap != nullptr ? &wrap->base : nullptr, nread, buf);
}

template <typename GetNameFn>
int PipeGetName(PipeWrap* wrap, GetNameFn fn, std::string* name_out) {
  if (wrap == nullptr || name_out == nullptr) return UV_EINVAL;
  size_t len = 256;
  std::string name(len, '\0');
  int rc = fn(&wrap->handle, name.data(), &len);
  if (rc == UV_ENOBUFS) {
    name.resize(len);
    rc = fn(&wrap->handle, name.data(), &len);
  }
  if (rc != 0) return rc;
  name.resize(len);
  *name_out = std::move(name);
  return 0;
}

void OnWriteDone(uv_write_t* req, int status) {
  (void)req;
  (void)status;
}

void OnConnectDone(uv_connect_t* req, int status) {
  auto* cr = static_cast<PipeConnectReqWrap*>(req->data);
  if (cr == nullptr) return;
  if (cr->env == nullptr) {
    ReleasePipeConnectReqState(cr);
    return;
  }
  edge::HandleScope scope(cr->env);
  if (!scope.is_open()) {
    QueuePipeConnectReqDestroyIfNeeded(cr);
    ReleasePipeConnectReqState(cr);
    return;
  }
  napi_value req_obj = PipeConnectReqGetOwner(cr->env, cr);
  napi_value pipe_obj = cr->pipe != nullptr ? EdgeStreamBaseGetWrapper(&cr->pipe->base) : nullptr;
  napi_value argv[5] = {
      EdgeStreamBaseMakeInt32(cr->env, status),
      pipe_obj,
      req_obj,
      EdgeStreamBaseMakeBool(cr->env, true),
      EdgeStreamBaseMakeBool(cr->env, true),
  };
  if (req_obj != nullptr) {
    EdgeStreamBaseSetReqError(cr->env, req_obj, status);
    if (auto* environment = EdgeEnvironmentGet(cr->env);
        environment == nullptr || environment->can_call_into_js()) {
      napi_value oncomplete = nullptr;
      if (napi_get_named_property(cr->env, req_obj, "oncomplete", &oncomplete) == napi_ok &&
          IsFunction(cr->env, oncomplete)) {
        napi_value ignored = nullptr;
        (void)EdgeAsyncWrapMakeCallback(
            cr->env, cr->async_id, req_obj, req_obj, oncomplete, 5, argv, &ignored, kEdgeMakeCallbackNone);
      }
    }
  }
  QueuePipeConnectReqDestroyIfNeeded(cr);
  ReleasePipeConnectReqState(cr);
}

napi_value PipeCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (self == nullptr) return nullptr;

  int32_t socket_type = kPipeSocket;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_int32(env, argv[0], &socket_type);
  }

  auto* wrap = new PipeWrap();
  wrap->env = env;
  wrap->socket_type = socket_type;
  wrap->ipc = socket_type == kPipeIPC;
  uv_loop_t* loop = EdgeGetEnvLoop(env);
  if (loop == nullptr || uv_pipe_init(loop, &wrap->handle, wrap->ipc ? 1 : 0) != 0) {
    delete wrap;
    return EdgeStreamBaseUndefined(env);
  }
  wrap->handle.data = wrap;
  int32_t provider = kEdgeProviderPipeWrap;
  if (socket_type == kPipeServer) provider = kEdgeProviderPipeServerWrap;
  EdgeStreamBaseInit(&wrap->base, env, &kPipeOps, provider);
  napi_wrap(env, self, wrap, [](napi_env finalize_env, void* data, void*) {
    auto* pipe_wrap = static_cast<PipeWrap*>(data);
    if (pipe_wrap == nullptr) return;
    EdgeStreamBaseFinalize(&pipe_wrap->base);
  }, nullptr, &wrap->base.wrapper_ref);
  (void)napi_type_tag_object(env, self, &kPipeWrapTypeTag);
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->base.wrapper_ref);
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, false, false);
  if (socket_type != kPipeIPC) {
    napi_value undefined = EdgeStreamBaseUndefined(env);
    if (undefined != nullptr) {
      napi_set_named_property(env, self, "getsockname", undefined);
      napi_set_named_property(env, self, "getpeername", undefined);
    }
  }
  return self;
}

napi_value PipeOpen(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t fd = -1;
  napi_get_value_int32(env, argv[0], &fd);
  return EdgeStreamBaseMakeInt32(env, uv_pipe_open(&wrap->handle, static_cast<uv_file>(fd)));
}

napi_value PipeBind(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  std::string path = ValueToUtf8(env, argv[0]);
  return EdgeStreamBaseMakeInt32(
      env,
      uv_pipe_bind2(&wrap->handle, path.c_str(), path.size(), UV_PIPE_NO_TRUNCATE));
}

void OnConnection(uv_stream_t* server, int status) {
  auto* server_wrap = server != nullptr ? static_cast<PipeWrap*>(server->data) : nullptr;
  if (server_wrap == nullptr) return;
  napi_env env = server_wrap->env;
  if (env == nullptr) return;
  edge::HandleScope scope(env);
  if (!scope.is_open()) return;
  napi_value server_obj = EdgeStreamBaseGetWrapper(&server_wrap->base);
  napi_value onconnection = nullptr;
  if (server_obj == nullptr ||
      napi_get_named_property(env, server_obj, "onconnection", &onconnection) != napi_ok ||
      !IsFunction(env, onconnection)) {
    return;
  }

  napi_value argv[2] = {EdgeStreamBaseMakeInt32(env, status), EdgeStreamBaseUndefined(env)};
  if (status == 0) {
    PipeBindingState* state = GetBindingState(env);
    napi_value ctor = state != nullptr ? GetRefValue(env, state->pipe_ctor_ref) : nullptr;
    napi_value type_arg = EdgeStreamBaseMakeInt32(env, kPipeSocket);
    napi_value client_obj = nullptr;
    if (ctor == nullptr ||
        type_arg == nullptr ||
        napi_new_instance(env, ctor, 1, &type_arg, &client_obj) != napi_ok ||
        client_obj == nullptr) {
      return;
    }
    PipeWrap* client_wrap = nullptr;
    if (napi_unwrap(env, client_obj, reinterpret_cast<void**>(&client_wrap)) != napi_ok || client_wrap == nullptr) {
      return;
    }
    int rc = uv_accept(server, reinterpret_cast<uv_stream_t*>(&client_wrap->handle));
    argv[0] = EdgeStreamBaseMakeInt32(env, rc);
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

napi_value PipeListen(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t backlog = 511;
  napi_get_value_int32(env, argv[0], &backlog);
  return EdgeStreamBaseMakeInt32(
      env,
      uv_listen(reinterpret_cast<uv_stream_t*>(&wrap->handle), backlog, OnConnection));
}

napi_value PipeConnect(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);

  auto* cr = static_cast<PipeConnectReqWrap*>(nullptr);
  if (argv[0] == nullptr || napi_unwrap(env, argv[0], reinterpret_cast<void**>(&cr)) != napi_ok || cr == nullptr) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  cr->env = env;
  cr->pipe = wrap;
  if (!ActivatePipeConnectReq(cr, argv[0])) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  cr->req.data = cr;
  cr->active_request_token =
      EdgeRegisterActiveRequest(env,
                                argv[0],
                                "PipeConnectWrap",
                                cr,
                                CancelPipeConnectReq,
                                PipeConnectReqGetOwner);
  std::string path = ValueToUtf8(env, argv[1]);
  int rc = uv_pipe_connect2(&cr->req,
                            &wrap->handle,
                            path.c_str(),
                            path.size(),
                            UV_PIPE_NO_TRUNCATE,
                            OnConnectDone);
  if (rc != 0) {
    EdgeStreamBaseSetReqError(env, argv[0], rc);
    QueuePipeConnectReqDestroyIfNeeded(cr);
    ReleasePipeConnectReqState(cr);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value PipeReadStart(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int rc = uv_read_start(reinterpret_cast<uv_stream_t*>(&wrap->handle), OnAlloc, OnRead);
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, rc == 0);
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value PipeReadStop(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int rc = uv_read_stop(reinterpret_cast<uv_stream_t*>(&wrap->handle));
  if (rc == UV_EALREADY) rc = 0;
  EdgeStreamBaseSetReading(&wrap->base, false);
  return EdgeStreamBaseMakeInt32(env, rc);
}

uv_stream_t* GetSendHandle(napi_env env, napi_value value) {
  uv_stream_t* stream = EdgeStreamBaseGetLibuvStream(env, value);
  if (stream != nullptr) return stream;
  uv_handle_t* udp_handle = EdgeUdpWrapGetHandle(env, value);
  return reinterpret_cast<uv_stream_t*>(udp_handle);
}

napi_value PipeWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  uv_stream_t* send_handle = nullptr;
  napi_value send_handle_obj = nullptr;
  if (argc >= 3 && argv[2] != nullptr) {
    send_handle_obj = argv[2];
    send_handle = GetSendHandle(env, argv[2]);
  }
  return EdgeLibuvStreamWriteBuffer(&wrap->base, argv[0], argv[1], send_handle, send_handle_obj);
}

napi_value PipeWriteString(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  napi_value self = nullptr;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, &data);
  PipeWrap* wrap = nullptr;
  if (self != nullptr) napi_unwrap(env, self, reinterpret_cast<void**>(&wrap));
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  uv_stream_t* send_handle = nullptr;
  napi_value send_handle_obj = nullptr;
  if (argc >= 3 && argv[2] != nullptr) {
    send_handle_obj = argv[2];
    send_handle = GetSendHandle(env, argv[2]);
  }
  return EdgeLibuvStreamWriteString(&wrap->base,
                                   argv[0],
                                   argv[1],
                                   static_cast<const char*>(data),
                                   send_handle,
                                   send_handle_obj);
}

napi_value PipeWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool all_buffers = false;
  if (argc > 2 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &all_buffers);
  return EdgeLibuvStreamWriteV(&wrap->base, argv[0], argv[1], all_buffers, nullptr, nullptr);
}

napi_value PipeShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeLibuvStreamShutdown(&wrap->base, argv[0]);
}

napi_value PipeClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseUndefined(env);
  return EdgeStreamBaseClose(&wrap->base, argc > 0 ? argv[0] : nullptr);
}

napi_value PipeSetBlocking(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  bool on = false;
  napi_get_value_bool(env, argv[0], &on);
  return EdgeStreamBaseMakeInt32(
      env,
      uv_stream_set_blocking(reinterpret_cast<uv_stream_t*>(&wrap->handle), on ? 1 : 0));
}

napi_value PipeSetPendingInstances(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t count = 0;
  napi_get_value_int32(env, argv[0], &count);
  uv_pipe_pending_instances(&wrap->handle, count);
  return EdgeStreamBaseMakeInt32(env, 0);
}

napi_value PipeFchmod(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  int32_t mode = 0;
  napi_get_value_int32(env, argv[0], &mode);
  return EdgeStreamBaseMakeInt32(env, uv_pipe_chmod(&wrap->handle, mode));
}

napi_value PipeUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  return EdgeStreamBaseUseUserBuffer(&wrap->base, argv[0]);
}

napi_value PipeRef(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseRef(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value PipeUnref(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseUnref(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value PipeHasRef(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseHasRefValue(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeGetAsyncId(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetAsyncId(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeGetProviderType(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetProviderType(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeAsyncReset(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseAsyncReset(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeGetOnRead(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetOnRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeSetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  return EdgeStreamBaseSetOnRead(wrap != nullptr ? &wrap->base : nullptr, argc > 0 ? argv[0] : nullptr);
}

napi_value PipeGetSockName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  std::string name;
  int rc = PipeGetName(wrap, uv_pipe_getsockname, &name);
  if (rc == 0) {
    napi_value value = nullptr;
    napi_create_string_utf8(env, name.c_str(), name.size(), &value);
    if (value != nullptr) napi_set_named_property(env, argv[0], "address", value);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value PipeGetPeerName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  PipeWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  std::string name;
  int rc = PipeGetName(wrap, uv_pipe_getpeername, &name);
  if (rc == 0) {
    napi_value value = nullptr;
    napi_create_string_utf8(env, name.c_str(), name.size(), &value);
    if (value != nullptr) napi_set_named_property(env, argv[0], "address", value);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value AcceptPendingHandleForIpc(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  napi_env env = wrap != nullptr ? wrap->env : nullptr;
  if (env == nullptr || wrap == nullptr) return EdgeStreamBaseUndefined(env);

  if (uv_pipe_pending_count(&wrap->handle) <= 0) return EdgeStreamBaseUndefined(env);

  uv_handle_type pending_type = uv_pipe_pending_type(&wrap->handle);
  napi_value handle_obj = nullptr;
  uv_stream_t* accept_target = nullptr;

  if (pending_type == UV_TCP) {
    napi_value tcp_ctor = EdgeGetTcpWrapConstructor(env);
    napi_value arg = EdgeStreamBaseMakeInt32(env, 0);
    if (tcp_ctor == nullptr ||
        arg == nullptr ||
        napi_new_instance(env, tcp_ctor, 1, &arg, &handle_obj) != napi_ok ||
        handle_obj == nullptr) {
      return EdgeStreamBaseUndefined(env);
    }
    accept_target = EdgeTcpWrapGetStream(env, handle_obj);
  } else if (pending_type == UV_NAMED_PIPE) {
    PipeBindingState* state = GetBindingState(env);
    napi_value ctor = state != nullptr ? GetRefValue(env, state->pipe_ctor_ref) : nullptr;
    napi_value arg = EdgeStreamBaseMakeInt32(env, kPipeSocket);
    if (ctor == nullptr ||
        arg == nullptr ||
        napi_new_instance(env, ctor, 1, &arg, &handle_obj) != napi_ok ||
        handle_obj == nullptr) {
      return EdgeStreamBaseUndefined(env);
    }
    accept_target = EdgePipeWrapGetStream(env, handle_obj);
  } else if (pending_type == UV_UDP) {
    napi_value udp_ctor = EdgeGetUdpWrapConstructor(env);
    if (udp_ctor == nullptr ||
        napi_new_instance(env, udp_ctor, 0, nullptr, &handle_obj) != napi_ok ||
        handle_obj == nullptr) {
      return EdgeStreamBaseUndefined(env);
    }
    accept_target = reinterpret_cast<uv_stream_t*>(EdgeUdpWrapGetHandle(env, handle_obj));
  } else {
    return EdgeStreamBaseUndefined(env);
  }

  if (accept_target == nullptr) return EdgeStreamBaseUndefined(env);
  if (uv_accept(reinterpret_cast<uv_stream_t*>(&wrap->handle), accept_target) != 0) {
    return EdgeStreamBaseUndefined(env);
  }
  return handle_obj;
}

napi_value PipeBytesReadGetter(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeBytesWrittenGetter(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesWritten(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeWriteQueueSizeGetter(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetWriteQueueSize(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeFdGetter(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetFd(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeExternalStreamGetter(napi_env env, napi_callback_info info) {
  PipeWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetExternal(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value PipeConnectWrapCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (self == nullptr) return nullptr;
  auto* req = new PipeConnectReqWrap();
  req->env = env;
  req->async_id = EdgeAsyncWrapNextId(env);
  if (napi_wrap(env, self, req, PipeConnectReqFinalize, nullptr, &req->wrapper_ref) != napi_ok) {
    delete req;
    return nullptr;
  }
  EdgeAsyncWrapEmitInit(
      env, req->async_id, kEdgeProviderPipeConnectWrap, EdgeAsyncWrapExecutionAsyncId(env), self);
  return self;
}

void SetNamedU32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value out = nullptr;
  napi_create_uint32(env, value, &out);
  if (out != nullptr) napi_set_named_property(env, obj, key, out);
}

}  // namespace

uv_stream_t* EdgePipeWrapGetStream(napi_env env, napi_value value) {
  EdgeStreamBase* base = EdgePipeWrapGetStreamBase(env, value);
  return base != nullptr ? reinterpret_cast<uv_stream_t*>(&FromBase(base)->handle) : nullptr;
}

EdgeStreamBase* EdgePipeWrapGetStreamBase(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok ||
      (type != napi_object && type != napi_function && type != napi_external)) {
    return nullptr;
  }
  // Confirm this JS object is genuinely a PipeWrap before downcasting the
  // unwrapped pointer. napi_unwrap succeeds for any wrapped native object, so
  // without this tag check we could read PipeWrap fields off a smaller wrap
  // (e.g. JsStreamWrap) and overrun its allocation.
  bool is_pipe = false;
  if (napi_check_object_type_tag(env, value, &kPipeWrapTypeTag, &is_pipe) != napi_ok || !is_pipe) {
    return nullptr;
  }
  PipeWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) return nullptr;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (handle->data != wrap || handle->type != UV_NAMED_PIPE) return nullptr;
  return &wrap->base;
}

bool EdgePipeWrapPushStreamListener(uv_stream_t* stream, EdgeStreamListener* listener) {
  if (stream == nullptr || listener == nullptr) return false;
  auto* wrap = static_cast<PipeWrap*>(stream->data);
  return wrap != nullptr && EdgeStreamBasePushListener(&wrap->base, listener);
}

bool EdgePipeWrapRemoveStreamListener(uv_stream_t* stream, EdgeStreamListener* listener) {
  if (stream == nullptr || listener == nullptr) return false;
  auto* wrap = static_cast<PipeWrap*>(stream->data);
  return wrap != nullptr && EdgeStreamBaseRemoveListener(&wrap->base, listener);
}

napi_value EdgeInstallPipeWrapBinding(napi_env env) {
  PipeBindingState& state = EnsureBindingState(env);
  napi_value cached = GetRefValue(env, state.binding_ref);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor pipe_props[] = {
      {"open", nullptr, PipeOpen, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setBlocking", nullptr, PipeSetBlocking, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"bind", nullptr, PipeBind, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"listen", nullptr, PipeListen, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"connect", nullptr, PipeConnect, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, PipeClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStart", nullptr, PipeReadStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStop", nullptr, PipeReadStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeBuffer", nullptr, PipeWriteBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writev", nullptr, PipeWritev, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeLatin1String", nullptr, PipeWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("latin1")},
      {"writeUtf8String", nullptr, PipeWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("utf8")},
      {"writeAsciiString", nullptr, PipeWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("ascii")},
      {"writeUcs2String", nullptr, PipeWriteString, nullptr, nullptr, nullptr, napi_default_method,
       const_cast<char*>("ucs2")},
      {"shutdown", nullptr, PipeShutdown, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setPendingInstances", nullptr, PipeSetPendingInstances, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"fchmod", nullptr, PipeFchmod, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"useUserBuffer", nullptr, PipeUseUserBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"ref", nullptr, PipeRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"unref", nullptr, PipeUnref, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"hasRef", nullptr, PipeHasRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, PipeGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType", nullptr, PipeGetProviderType, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"asyncReset", nullptr, PipeAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onread", nullptr, nullptr, PipeGetOnRead, PipeSetOnRead, nullptr, napi_default, nullptr},
      {"bytesRead", nullptr, nullptr, PipeBytesReadGetter, nullptr, nullptr, napi_default, nullptr},
      {"bytesWritten", nullptr, nullptr, PipeBytesWrittenGetter, nullptr, nullptr, napi_default, nullptr},
      {"writeQueueSize", nullptr, nullptr, PipeWriteQueueSizeGetter, nullptr, nullptr, napi_default, nullptr},
      {"fd", nullptr, nullptr, PipeFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"_externalStream", nullptr, nullptr, PipeExternalStreamGetter, nullptr, nullptr, napi_default, nullptr},
      {"getsockname", nullptr, PipeGetSockName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getpeername", nullptr, PipeGetPeerName, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };

  napi_value pipe_ctor = nullptr;
  if (napi_define_class(env,
                        "Pipe",
                        NAPI_AUTO_LENGTH,
                        PipeCtor,
                        nullptr,
                        sizeof(pipe_props) / sizeof(pipe_props[0]),
                        pipe_props,
                        &pipe_ctor) != napi_ok ||
      pipe_ctor == nullptr) {
    return nullptr;
  }
  if (state.pipe_ctor_ref != nullptr) napi_delete_reference(env, state.pipe_ctor_ref);
  napi_create_reference(env, pipe_ctor, 1, &state.pipe_ctor_ref);

  napi_value connect_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "PipeConnectWrap",
                        NAPI_AUTO_LENGTH,
                        PipeConnectWrapCtor,
                        nullptr,
                        0,
                        nullptr,
                        &connect_wrap_ctor) != napi_ok ||
      connect_wrap_ctor == nullptr) {
    return nullptr;
  }

  napi_value constants = nullptr;
  napi_create_object(env, &constants);
  SetNamedU32(env, constants, "SOCKET", kPipeSocket);
  SetNamedU32(env, constants, "SERVER", kPipeServer);
  SetNamedU32(env, constants, "IPC", kPipeIPC);
  SetNamedU32(env, constants, "UV_READABLE", static_cast<uint32_t>(UV_READABLE));
  SetNamedU32(env, constants, "UV_WRITABLE", static_cast<uint32_t>(UV_WRITABLE));

  napi_set_named_property(env, binding, "Pipe", pipe_ctor);
  napi_set_named_property(env, binding, "PipeConnectWrap", connect_wrap_ctor);
  napi_set_named_property(env, binding, "constants", constants);

  if (state.binding_ref != nullptr) napi_delete_reference(env, state.binding_ref);
  napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}
