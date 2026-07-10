#include "internal_binding/binding_initializers.h"

#include <cstdint>
#include <cstring>
#include <string>

#include <uv.h>

#include "internal_binding/helpers.h"
#include "edge_active_resource.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_runtime.h"

namespace internal_binding {

namespace {

struct FsEventWrap {
  EdgeHandleWrap handle_wrap{};
  napi_ref owner_ref = nullptr;
  uv_fs_event_t handle{};
  bool referenced = true;
  int64_t async_id = 0;
  std::string encoding = "utf8";
};

int64_t g_next_async_id = 500000;

bool ValueToUtf8(napi_env env, napi_value value, std::string* out) {
  if (env == nullptr || value == nullptr || out == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(env, value, &data, &len) != napi_ok || data == nullptr) return false;
    out->assign(static_cast<const char*>(data), len);
    return true;
  }

  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value MakeInt64(napi_env env, int64_t value) {
  napi_value out = nullptr;
  napi_create_int64(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

FsEventWrap* UnwrapFsEvent(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<FsEventWrap*>(data);
}

FsEventWrap* GetFsEventWrap(napi_env env, napi_callback_info info, napi_value* this_arg_out = nullptr) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg_out != nullptr) *this_arg_out = this_arg;
  return UnwrapFsEvent(env, this_arg);
}

bool FsEventHasRef(void* data) {
  auto* wrap = static_cast<FsEventWrap*>(data);
  return wrap != nullptr &&
         EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle));
}

napi_value FsEventGetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<FsEventWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

napi_value CreateFilenameValue(FsEventWrap* wrap, const char* filename) {
  if (wrap == nullptr || wrap->handle_wrap.env == nullptr || filename == nullptr) {
    napi_value out = nullptr;
    if (wrap != nullptr && wrap->handle_wrap.env != nullptr) {
      napi_get_null(wrap->handle_wrap.env, &out);
    }
    return out;
  }

  napi_value out = nullptr;
  if (wrap->encoding == "buffer") {
    napi_create_buffer_copy(wrap->handle_wrap.env, std::strlen(filename), filename, nullptr, &out);
    return out != nullptr ? out : Undefined(wrap->handle_wrap.env);
  }

  napi_value buffer = nullptr;
  if (napi_create_buffer_copy(wrap->handle_wrap.env,
                              std::strlen(filename),
                              filename,
                              nullptr,
                              &buffer) != napi_ok ||
      buffer == nullptr) {
    return Undefined(wrap->handle_wrap.env);
  }

  napi_value to_string = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(wrap->handle_wrap.env, buffer, "toString", &to_string) != napi_ok ||
      to_string == nullptr ||
      napi_typeof(wrap->handle_wrap.env, to_string, &type) != napi_ok ||
      type != napi_function) {
    return buffer;
  }

  napi_value encoding_value = nullptr;
  if (napi_create_string_utf8(wrap->handle_wrap.env,
                              wrap->encoding.c_str(),
                              NAPI_AUTO_LENGTH,
                              &encoding_value) != napi_ok ||
      encoding_value == nullptr ||
      napi_call_function(
          wrap->handle_wrap.env, buffer, to_string, 1, &encoding_value, &out) != napi_ok ||
      out == nullptr) {
    return buffer;
  }

  return out;
}

void DeleteFsEventWrap(void* data) {
  auto* wrap = static_cast<FsEventWrap*>(data);
  if (wrap == nullptr) return;
  EdgeHandleWrapDeleteRefIfPresent(wrap->handle_wrap.env, &wrap->owner_ref);
  delete wrap;
}

void OnClosed(uv_handle_t* handle) {
  auto* wrap = static_cast<FsEventWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr) return;
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteFsEventWrap);
}

void CloseFsEvent(FsEventWrap* wrap) {
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  wrap->handle_wrap.state = kEdgeHandleClosing;
  uv_close(reinterpret_cast<uv_handle_t*>(&wrap->handle), OnClosed);
}

void CloseFsEventForCleanup(void* data) {
  CloseFsEvent(static_cast<FsEventWrap*>(data));
}

void FsEventFinalize(napi_env env, void* data, void* /*hint*/) {
  EdgeHandleWrapFinalizeNative(env, &static_cast<FsEventWrap*>(data)->handle_wrap, data, DeleteFsEventWrap);
}

void OnEvent(uv_fs_event_t* handle, const char* filename, int events, int status) {
  auto* wrap = static_cast<FsEventWrap*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr || wrap->handle_wrap.env == nullptr) return;
  edge::HandleScope scope(wrap->handle_wrap.env);
  if (!scope.is_open()) return;

  napi_value self = EdgeHandleWrapGetRefValue(wrap->handle_wrap.env, wrap->handle_wrap.wrapper_ref);
  if (self == nullptr) return;

  napi_value onchange = nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_get_named_property(wrap->handle_wrap.env, self, "onchange", &onchange) != napi_ok ||
      onchange == nullptr ||
      napi_typeof(wrap->handle_wrap.env, onchange, &type) != napi_ok ||
      type != napi_function) {
    return;
  }

  const char* event_name = "";
  if (status == 0) {
    if ((events & UV_RENAME) != 0) {
      event_name = "rename";
    } else if ((events & UV_CHANGE) != 0) {
      event_name = "change";
    }
  }

  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_create_int32(wrap->handle_wrap.env, status, &argv[0]);
  napi_create_string_utf8(wrap->handle_wrap.env, event_name, NAPI_AUTO_LENGTH, &argv[1]);
  argv[2] = CreateFilenameValue(wrap, filename);
  if (argv[2] == nullptr) {
    napi_get_null(wrap->handle_wrap.env, &argv[2]);
  }

  napi_value ignored = nullptr;
  EdgeMakeCallback(wrap->handle_wrap.env, self, onchange, 3, argv, &ignored);
}

napi_value FsEventCtor(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  if (this_arg == nullptr) return nullptr;

  auto* wrap = new FsEventWrap();
  EdgeHandleWrapInit(&wrap->handle_wrap, env);
  wrap->async_id = g_next_async_id++;
  if (napi_wrap(env, this_arg, wrap, FsEventFinalize, nullptr, &wrap->handle_wrap.wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  return this_arg;
}

napi_value FsEventStart(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FsEventWrap* wrap = UnwrapFsEvent(env, this_arg);
  if (wrap == nullptr) return MakeInt32(env, UV_EINVAL);
  if (wrap->handle_wrap.state == kEdgeHandleClosing || wrap->handle_wrap.state == kEdgeHandleClosed) {
    return MakeInt32(env, UV_EINVAL);
  }
  if (wrap->handle_wrap.state == kEdgeHandleInitialized) return MakeInt32(env, 0);

  std::string path;
  if (argc < 1 || !ValueToUtf8(env, argv[0], &path)) return MakeInt32(env, UV_EINVAL);

  bool persistent = true;
  if (argc >= 2 && argv[1] != nullptr) {
    napi_get_value_bool(env, argv[1], &persistent);
  }

  bool recursive = false;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_get_value_bool(env, argv[2], &recursive);
  }

  std::string encoding = "utf8";
  if (argc >= 4 && argv[3] != nullptr) {
    ValueToUtf8(env, argv[3], &encoding);
  }
  wrap->encoding = std::move(encoding);

  int flags = 0;
  if (recursive) flags |= UV_FS_EVENT_RECURSIVE;

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  int rc = loop != nullptr ? uv_fs_event_init(loop, &wrap->handle) : UV_EINVAL;
  if (rc != 0) return MakeInt32(env, rc);

  wrap->handle.data = wrap;
  EdgeHandleWrapAttach(&wrap->handle_wrap,
                      wrap,
                      reinterpret_cast<uv_handle_t*>(&wrap->handle),
                      CloseFsEventForCleanup);
  rc = uv_fs_event_start(&wrap->handle, OnEvent, path.c_str(), flags);
  if (rc != 0) {
    wrap->handle_wrap.state = kEdgeHandleClosing;
    uv_close(reinterpret_cast<uv_handle_t*>(&wrap->handle), OnClosed);
    return MakeInt32(env, rc);
  }

  wrap->handle_wrap.state = kEdgeHandleInitialized;
  // Keep-alive comes from EdgeRegisterActiveHandle's strong keepalive_ref.
  if (wrap->handle_wrap.active_handle_token == nullptr) {
    wrap->handle_wrap.active_handle_token =
        EdgeRegisterActiveHandle(
            env, this_arg, "FSEVENTWRAP", FsEventHasRef, FsEventGetActiveOwner, wrap, CloseFsEventForCleanup);
  }
  if (!persistent) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
    wrap->referenced = false;
  } else {
    wrap->referenced = true;
  }

  return MakeInt32(env, 0);
}

napi_value FsEventClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FsEventWrap* wrap = UnwrapFsEvent(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  if (wrap->handle_wrap.state == kEdgeHandleInitialized && argc >= 1 && argv[0] != nullptr) {
    EdgeHandleWrapSetOnCloseCallback(env, this_arg, argv[0]);
  }
  CloseFsEvent(wrap);
  return Undefined(env);
}

napi_value FsEventRef(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return Undefined(env);
  if (!wrap->referenced) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
    wrap->referenced = true;
  }
  return Undefined(env);
}

napi_value FsEventUnref(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return Undefined(env);
  if (wrap->referenced) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->handle));
    wrap->referenced = false;
  }
  return Undefined(env);
}

napi_value FsEventGetAsyncId(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  return MakeInt64(env, wrap != nullptr ? wrap->async_id : -1);
}

napi_value FsEventGetInitialized(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  napi_value out = nullptr;
  napi_get_boolean(env, wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value FsEventGetOwner(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  napi_value owner = wrap == nullptr ? nullptr : EdgeHandleWrapGetRefValue(env, wrap->owner_ref);
  return owner != nullptr ? owner : Undefined(env);
}

napi_value FsEventSetOwner(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  FsEventWrap* wrap = UnwrapFsEvent(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  EdgeHandleWrapDeleteRefIfPresent(env, &wrap->owner_ref);
  if (argc >= 1 && argv[0] != nullptr && !IsUndefined(env, argv[0])) {
    napi_create_reference(env, argv[0], 1, &wrap->owner_ref);
  }
  return Undefined(env);
}

napi_value FsEventHasRef(napi_env env, napi_callback_info info) {
  FsEventWrap* wrap = GetFsEventWrap(env, info);
  napi_value out = nullptr;
  napi_get_boolean(
      env,
      wrap != nullptr &&
          EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->handle)),
      &out);
  return out != nullptr ? out : Undefined(env);
}

}  // namespace

napi_value InitFsEventWrap(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  napi_property_descriptor methods[] = {
      {"start", nullptr, FsEventStart, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"close", nullptr, FsEventClose, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"ref", nullptr, FsEventRef, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"unref", nullptr, FsEventUnref, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"hasRef", nullptr, FsEventHasRef, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getAsyncId", nullptr, FsEventGetAsyncId, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"initialized", nullptr, nullptr, FsEventGetInitialized, nullptr, nullptr, napi_default, nullptr},
      {"owner",
       nullptr,
       nullptr,
       FsEventGetOwner,
       FsEventSetOwner,
       nullptr,
       static_cast<napi_property_attributes>(napi_configurable),
       nullptr},
  };

  napi_value cls = nullptr;
  if (napi_define_class(env,
                        "FSEvent",
                        NAPI_AUTO_LENGTH,
                        FsEventCtor,
                        nullptr,
                        sizeof(methods) / sizeof(methods[0]),
                        methods,
                        &cls) == napi_ok &&
      cls != nullptr) {
    napi_set_named_property(env, out, "FSEvent", cls);
  }

  return out;
}

}  // namespace internal_binding
