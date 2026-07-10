#include "edge_signal_wrap.h"

#include <cstdint>
#include <map>
#include <mutex>

#include <uv.h>

// #include "edge_async_wrap.h"
#include "edge_active_resource.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_runtime.h"

namespace {

struct SignalWrap {
  EdgeHandleWrap handle_wrap{};
  uv_signal_t handle{};
  bool active = false;
  bool destroy_queued = false;
  // int64_t async_id = 0;
};

bool SignalWrapHasRef(void* data) {
  auto* wrap = static_cast<SignalWrap*>(data);
  return wrap != nullptr &&
         EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle));
}

napi_value SignalWrapGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<SignalWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

std::mutex g_handled_signals_mutex;
std::map<int, int64_t> g_handled_signals;

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value MakeInt64(napi_env env, int64_t value) {
  napi_value out = nullptr;
  napi_create_int64(env, value, &out);
  return out;
}

void ThrowIllegalInvocation(napi_env env) {
  napi_throw_type_error(env, nullptr, "Illegal invocation");
}

bool UnwrapSignalWrap(napi_env env, napi_value self, SignalWrap** out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (self == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  SignalWrap* wrap = nullptr;
  if (napi_unwrap(env, self, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  *out = wrap;
  return true;
}

void IncreaseSignalHandlerCount(int signum) {
  std::lock_guard<std::mutex> lock(g_handled_signals_mutex);
  g_handled_signals[signum]++;
}

void DecreaseSignalHandlerCount(int signum) {
  std::lock_guard<std::mutex> lock(g_handled_signals_mutex);
  auto it = g_handled_signals.find(signum);
  if (it == g_handled_signals.end()) return;
  if (--it->second <= 0) g_handled_signals.erase(it);
}

void QueueDestroy(SignalWrap* wrap) {
  // if (wrap == nullptr || wrap->destroy_queued || wrap->async_id <= 0) return;
  wrap->destroy_queued = true;
  // EdgeAsyncWrapQueueDestroy(wrap->env, static_cast<double>(wrap->async_id));
}

void DeleteSignalWrap(void* data) {
  auto* wrap = static_cast<SignalWrap*>(data);
  if (wrap == nullptr) return;
  QueueDestroy(wrap);
  delete wrap;
}

void OnClosed(uv_handle_t* handle);

void OnSignal(uv_signal_t* handle, int signum) {
  auto* wrap = static_cast<SignalWrap*>(handle->data);
  if (wrap == nullptr) return;
  if (wrap->handle_wrap.env == nullptr) return;
  edge::HandleScope scope(wrap->handle_wrap.env);
  if (!scope.is_open()) return;
  napi_value self = EdgeHandleWrapGetRefValue(wrap->handle_wrap.env, wrap->handle_wrap.wrapper_ref);
  if (self == nullptr) return;
  napi_value onsignal = nullptr;
  if (napi_get_named_property(wrap->handle_wrap.env, self, "onsignal", &onsignal) != napi_ok ||
      onsignal == nullptr) {
    return;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(wrap->handle_wrap.env, onsignal, &type) != napi_ok || type != napi_function) return;
  napi_value arg = MakeInt32(wrap->handle_wrap.env, signum);
  napi_value argv[1] = {arg};
  napi_value ignored = nullptr;
  EdgeMakeCallback(wrap->handle_wrap.env, self, onsignal, 1, argv, &ignored);
}

void CloseSignalWrapHandle(SignalWrap* wrap) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  if (wrap->active) {
    DecreaseSignalHandlerCount(wrap->handle.signum);
    wrap->active = false;
  }
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (!uv_is_closing(handle)) {
    wrap->handle_wrap.state = kEdgeHandleClosing;
    uv_close(handle, OnClosed);
  }
}

void CloseSignalWrapForCleanup(void* data) {
  CloseSignalWrapHandle(static_cast<SignalWrap*>(data));
}

void OnClosed(uv_handle_t* handle) {
  auto* wrap = static_cast<SignalWrap*>(handle->data);
  if (wrap == nullptr) return;
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteSignalWrap);
}

void SignalFinalize(napi_env env, void* data, void* /*hint*/) {
  EdgeHandleWrapFinalizeNative(env, &static_cast<SignalWrap*>(data)->handle_wrap, data, DeleteSignalWrap);
}

napi_value SignalCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);

  auto* wrap = new SignalWrap();
  EdgeHandleWrapInit(&wrap->handle_wrap, env);
  // wrap->async_id = EdgeAsyncWrapNewAsyncId(env);

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  int rc = loop != nullptr ? uv_signal_init(loop, &wrap->handle) : UV_EINVAL;
  if (rc == 0) {
    wrap->handle_wrap.state = kEdgeHandleInitialized;
    wrap->handle.data = wrap;
    EdgeHandleWrapAttach(&wrap->handle_wrap,
                        wrap,
                        reinterpret_cast<uv_handle_t*>(&wrap->handle),
                        CloseSignalWrapForCleanup);
  }
  napi_wrap(env, self, wrap, SignalFinalize, nullptr, &wrap->handle_wrap.wrapper_ref);
  wrap->handle_wrap.active_handle_token =
      EdgeRegisterActiveHandle(
          env, self, "SIGNALWRAP", SignalWrapHasRef, SignalWrapGetActiveOwner, wrap, CloseSignalWrapForCleanup);

  // EdgeAsyncWrapEmitInit(env,
  //                        static_cast<double>(wrap->async_id),
  //                        "SIGNALWRAP",
  //                        -1,
  //                        self,
  //                        false);

  return self;
}

napi_value SignalStart(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state != kEdgeHandleInitialized) return MakeInt32(env, UV_EINVAL);
  if (argc < 1 || argv[0] == nullptr) return MakeInt32(env, UV_EINVAL);

  int32_t signum = 0;
  if (napi_get_value_int32(env, argv[0], &signum) != napi_ok) return MakeInt32(env, UV_EINVAL);

  const int rc = uv_signal_start(&wrap->handle, OnSignal, signum);
  if (rc == 0 && !wrap->active) {
    wrap->active = true;
    IncreaseSignalHandlerCount(signum);
  }
  return MakeInt32(env, rc);
}

napi_value SignalStop(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);

  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state != kEdgeHandleInitialized) return MakeInt32(env, UV_EINVAL);

  if (wrap->active) {
    wrap->active = false;
    DecreaseSignalHandlerCount(wrap->handle.signum);
  }
  const int rc = uv_signal_stop(&wrap->handle);
  return MakeInt32(env, rc);
}

napi_value SignalClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;

  if (wrap->handle_wrap.state != kEdgeHandleInitialized) {
    napi_value out = nullptr;
    napi_get_undefined(env, &out);
    return out;
  }

  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      EdgeHandleWrapSetOnCloseCallback(env, self, argv[0]);
    }
  }

  if (wrap->handle_wrap.state == kEdgeHandleInitialized) {
    CloseSignalWrapHandle(wrap);
  } else {
    QueueDestroy(wrap);
  }

  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value SignalRef(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state == kEdgeHandleInitialized) uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value SignalUnref(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state == kEdgeHandleInitialized) uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value SignalHasRef(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  SignalWrap* wrap = nullptr;
  if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
  bool has_ref = EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle));
  napi_value out = nullptr;
  napi_get_boolean(env, has_ref, &out);
  return out;
}

// napi_value SignalGetAsyncId(napi_env env, napi_callback_info info) {
//   napi_value self = nullptr;
//   size_t argc = 0;
//   napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
//   SignalWrap* wrap = nullptr;
//   if (!UnwrapSignalWrap(env, self, &wrap)) return nullptr;
//   return MakeInt64(env, wrap->async_id);
// }

}  // namespace

napi_value EdgeInstallSignalWrapBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  constexpr napi_property_attributes kMethodAttrs =
      static_cast<napi_property_attributes>(napi_writable | napi_configurable);
  napi_property_descriptor signal_props[] = {
      {"start", nullptr, SignalStart, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"stop", nullptr, SignalStop, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"close", nullptr, SignalClose, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"ref", nullptr, SignalRef, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"unref", nullptr, SignalUnref, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"hasRef", nullptr, SignalHasRef, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      // {"getAsyncId", nullptr, SignalGetAsyncId, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
  };

  napi_value signal_ctor = nullptr;
  if (napi_define_class(env,
                        "Signal",
                        NAPI_AUTO_LENGTH,
                        SignalCtor,
                        nullptr,
                        sizeof(signal_props) / sizeof(signal_props[0]),
                        signal_props,
                        &signal_ctor) != napi_ok ||
      signal_ctor == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "Signal", signal_ctor);
  return binding;
}
