#include "edge_tty_wrap.h"

#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <unordered_map>
#include <unordered_set>

#include <uv.h>

#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_stream_base.h"
#include "edge_trace.h"

namespace {

// Unique N-API type tag used to safely identify TtyWrap-backed JS objects
// before downcasting their unwrapped native pointer. See EdgePipeWrapGetStreamBase.
constexpr napi_type_tag kTtyWrapTypeTag = {
    0xb7e15162f01d3c9aULL,
    0x3f84a1b2c3d4e5f6ULL,
};

struct TtyWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
  uv_tty_t handle{};
  bool initialized = false;
  int init_err = 0;
  int32_t fd = -1;
  bool raw_mode = false;
};

struct TtyBindingState {
  explicit TtyBindingState(napi_env env_in) : env(env_in) {}
  ~TtyBindingState() {
    if (tty_ctor_ref != nullptr) napi_delete_reference(env, tty_ctor_ref);
    if (binding_ref != nullptr) napi_delete_reference(env, binding_ref);
  }

  napi_env env = nullptr;
  napi_ref tty_ctor_ref = nullptr;
  napi_ref binding_ref = nullptr;
};

TtyBindingState& EnsureBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<TtyBindingState>(
      env, kEdgeEnvironmentSlotTtyBindingState);
}

TtyWrap* FromBase(EdgeStreamBase* base) {
  if (base == nullptr) return nullptr;
  return reinterpret_cast<TtyWrap*>(reinterpret_cast<char*>(base) - offsetof(TtyWrap, base));
}

uv_handle_t* TtyGetHandle(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  if (wrap == nullptr || !wrap->initialized) return nullptr;
  return reinterpret_cast<uv_handle_t*>(&wrap->handle);
}

uv_stream_t* TtyGetStream(EdgeStreamBase* base) {
  auto* wrap = FromBase(base);
  if (wrap == nullptr || !wrap->initialized) return nullptr;
  return reinterpret_cast<uv_stream_t*>(&wrap->handle);
}

void TtyDestroy(EdgeStreamBase* base) {
  delete FromBase(base);
}

void TtyAfterClose(uv_handle_t* handle) {
  auto* wrap = handle != nullptr ? static_cast<TtyWrap*>(handle->data) : nullptr;
  if (wrap == nullptr) return;
  EdgeStreamBaseOnClosed(&wrap->base);
}

const EdgeStreamBaseOps kTtyOps = {
    TtyGetHandle,
    TtyGetStream,
    TtyAfterClose,
    TtyDestroy,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
};

napi_value GetThis(napi_env env,
                   napi_callback_info info,
                   size_t* argc_out,
                   napi_value* argv,
                   TtyWrap** wrap_out) {
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

void SetInitErrorContext(napi_env env, napi_value maybe_ctx, int err) {
  if (env == nullptr || maybe_ctx == nullptr || err == 0) return;
  napi_valuetype ctx_type = napi_undefined;
  if (napi_typeof(env, maybe_ctx, &ctx_type) != napi_ok || ctx_type != napi_object) return;

  napi_value errno_v = nullptr;
  napi_value code_v = nullptr;
  napi_value message_v = nullptr;
  napi_value syscall_v = nullptr;
  napi_create_int32(env, err, &errno_v);
  napi_create_string_utf8(env,
                          uv_err_name(err) != nullptr ? uv_err_name(err) : "UV_ERROR",
                          NAPI_AUTO_LENGTH,
                          &code_v);
  napi_create_string_utf8(env,
                          uv_strerror(err) != nullptr ? uv_strerror(err) : "unknown error",
                          NAPI_AUTO_LENGTH,
                          &message_v);
  napi_create_string_utf8(env, "uv_tty_init", NAPI_AUTO_LENGTH, &syscall_v);

  if (errno_v != nullptr) napi_set_named_property(env, maybe_ctx, "errno", errno_v);
  if (code_v != nullptr) napi_set_named_property(env, maybe_ctx, "code", code_v);
  if (message_v != nullptr) napi_set_named_property(env, maybe_ctx, "message", message_v);
  if (syscall_v != nullptr) napi_set_named_property(env, maybe_ctx, "syscall", syscall_v);
}

void OnAlloc(uv_handle_t* handle, size_t suggested_size, uv_buf_t* buf) {
  auto* wrap = handle != nullptr ? static_cast<TtyWrap*>(handle->data) : nullptr;
  EdgeStreamBaseOnUvAlloc(wrap != nullptr ? &wrap->base : nullptr, suggested_size, buf);
}

void OnRead(uv_stream_t* stream, ssize_t nread, const uv_buf_t* buf) {
  auto* wrap = stream != nullptr ? static_cast<TtyWrap*>(stream->data) : nullptr;
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY onread fd=%d nread=%zd len=%zu\n",
                 wrap != nullptr ? wrap->fd : -1,
                 nread,
                 buf != nullptr ? buf->len : 0);
  }
  EdgeStreamBaseOnUvRead(wrap != nullptr ? &wrap->base : nullptr, nread, buf);
}

napi_value TtyCtor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  if (self == nullptr) return nullptr;

  int32_t fd = -1;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_int32(env, argv[0], &fd);
  }

  auto* wrap = new TtyWrap();
  wrap->env = env;
  wrap->fd = fd;
  EdgeStreamBaseInit(&wrap->base, env, &kTtyOps, kEdgeProviderTtyWrap);

  if (fd >= 0) {
    uv_loop_t* loop = EdgeGetEnvLoop(env);
    wrap->init_err = loop != nullptr ? uv_tty_init(loop, &wrap->handle, fd, fd == 0 ? 1 : 0) : UV_EINVAL;
  } else {
    wrap->init_err = UV_EINVAL;
  }
  wrap->initialized = (wrap->init_err == 0);
  if (wrap->initialized) {
    wrap->handle.data = wrap;
  }

  napi_wrap(env,
            self,
            wrap,
            [](napi_env finalize_env, void* data, void*) {
              auto* tty_wrap = static_cast<TtyWrap*>(data);
              if (tty_wrap == nullptr) return;
              EdgeStreamBaseFinalize(&tty_wrap->base);
            },
            nullptr,
            &wrap->base.wrapper_ref);
  (void)napi_type_tag_object(env, self, &kTtyWrapTypeTag);

  if (wrap->initialized) {
    EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->base.wrapper_ref);
  }
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, true, false);
  SetInitErrorContext(env, argc >= 2 ? argv[1] : nullptr, wrap->init_err);
  return self;
}

napi_value TtyIsTTY(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t fd = -1;
  if (argc < 1 || argv[0] == nullptr || napi_get_value_int32(env, argv[0], &fd) != napi_ok || fd < 0) {
    return EdgeStreamBaseMakeBool(env, false);
  }
  return EdgeStreamBaseMakeBool(env, uv_guess_handle(fd) == UV_TTY);
}

napi_value TtyGetWindowSize(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }

  int width = 0;
  int height = 0;
  const int rc = uv_tty_get_winsize(&wrap->handle, &width, &height);
  if (rc == 0 && argc >= 1 && argv[0] != nullptr) {
    napi_set_element(env, argv[0], 0, EdgeStreamBaseMakeInt32(env, width));
    napi_set_element(env, argv[0], 1, EdgeStreamBaseMakeInt32(env, height));
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TtySetRawMode(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }

  bool flag = false;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_bool(env, argv[0], &flag);
  }
#ifdef UV_TTY_MODE_RAW_VT
  const uv_tty_mode_t mode = flag ? UV_TTY_MODE_RAW_VT : UV_TTY_MODE_NORMAL;
#else
  const uv_tty_mode_t mode = flag ? UV_TTY_MODE_RAW : UV_TTY_MODE_NORMAL;
#endif
  const int rc = uv_tty_set_mode(&wrap->handle, mode);
  if (rc == 0) {
    wrap->raw_mode = flag;
  }
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY setRawMode fd=%d flag=%s rc=%d\n",
                 wrap->fd,
                 flag ? "true" : "false",
                 rc);
  }
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TtySetBlocking(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }

  bool enable = true;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_bool(env, argv[0], &enable);
  }
  return EdgeStreamBaseMakeInt32(
      env,
      uv_stream_set_blocking(reinterpret_cast<uv_stream_t*>(&wrap->handle), enable ? 1 : 0));
}

napi_value TtyReadStart(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr || !wrap->initialized) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }

  int rc = uv_read_start(reinterpret_cast<uv_stream_t*>(&wrap->handle), OnAlloc, OnRead);
  if (rc == UV_EALREADY) rc = 0;
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY readStart fd=%d rc=%d has_ref=%d loop_alive=%d\n",
                 wrap->fd,
                 rc,
                 uv_has_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle)),
                 uv_loop_alive(wrap->handle.loop));
  }
  EdgeStreamBaseSetReading(&wrap->base, rc == 0);
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TtyReadStop(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap == nullptr || !wrap->initialized) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }

  if (wrap->fd == 0 && wrap->raw_mode) {
    if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
      std::fprintf(stderr, "EDGE_TRACE_TTY readStop ignored fd=0 raw_mode=true\n");
    }
    return EdgeStreamBaseMakeInt32(env, 0);
  }

  int rc = uv_read_stop(reinterpret_cast<uv_stream_t*>(&wrap->handle));
  if (rc == UV_EALREADY) rc = 0;
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_TTY readStop fd=%d rc=%d has_ref=%d loop_alive=%d\n",
                 wrap->fd,
                 rc,
                 uv_has_ref(reinterpret_cast<uv_handle_t*>(&wrap->handle)),
                 uv_loop_alive(wrap->handle.loop));
  }
  EdgeStreamBaseSetReading(&wrap->base, false);
  return EdgeStreamBaseMakeInt32(env, rc);
}

napi_value TtyWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized || argc < 2) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  return EdgeLibuvStreamWriteBuffer(&wrap->base, argv[0], argv[1], nullptr, nullptr);
}

napi_value TtyWriteString(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, &data);
  TtyWrap* wrap = nullptr;
  if (self != nullptr) {
    napi_unwrap(env, self, reinterpret_cast<void**>(&wrap));
  }
  if (wrap == nullptr || !wrap->initialized || argc < 2) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  return EdgeLibuvStreamWriteString(&wrap->base,
                                   argv[0],
                                   argv[1],
                                   static_cast<const char*>(data),
                                   nullptr,
                                   nullptr);
}

napi_value TtyWriteV(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized || argc < 2) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);

  bool all_buffers = false;
  if (argc > 2 && argv[2] != nullptr) {
    napi_get_value_bool(env, argv[2], &all_buffers);
  }
  return EdgeLibuvStreamWriteV(&wrap->base, argv[0], argv[1], all_buffers, nullptr, nullptr);
}

napi_value TtyShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || !wrap->initialized || argc < 1 || argv[0] == nullptr) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  return EdgeLibuvStreamShutdown(&wrap->base, argv[0]);
}

napi_value TtyClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr) return EdgeStreamBaseUndefined(env);
  napi_value close_callback = (argc > 0) ? argv[0] : nullptr;
  return EdgeStreamBaseClose(&wrap->base, close_callback);
}

napi_value TtyRef(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseRef(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value TtyUnref(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  if (wrap != nullptr) EdgeStreamBaseUnref(&wrap->base);
  return EdgeStreamBaseUndefined(env);
}

napi_value TtyHasRef(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseMakeBool(env, wrap != nullptr && EdgeStreamBaseHasRef(&wrap->base));
}

napi_value TtyGetAsyncId(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetAsyncId(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyGetProviderType(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetProviderType(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyAsyncReset(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseAsyncReset(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  if (wrap == nullptr || argc < 1) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  return EdgeStreamBaseUseUserBuffer(&wrap->base, argv[0]);
}

napi_value TtyGetOnRead(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetOnRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtySetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  TtyWrap* wrap = nullptr;
  GetThis(env, info, &argc, argv, &wrap);
  return EdgeStreamBaseSetOnRead(wrap != nullptr ? &wrap->base : nullptr, argc > 0 ? argv[0] : nullptr);
}

napi_value TtyBytesReadGetter(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyBytesWrittenGetter(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesWritten(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyFdGetter(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseMakeInt32(env, wrap != nullptr ? wrap->fd : -1);
}

napi_value TtyExternalStreamGetter(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetExternal(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value TtyWriteQueueSizeGetter(napi_env env, napi_callback_info info) {
  TtyWrap* wrap = nullptr;
  GetThis(env, info, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetWriteQueueSize(wrap != nullptr ? &wrap->base : nullptr);
}

}  // namespace

uv_stream_t* EdgeTtyWrapGetStream(napi_env env, napi_value value) {
  EdgeStreamBase* base = EdgeTtyWrapGetStreamBase(env, value);
  return base != nullptr ? reinterpret_cast<uv_stream_t*>(&FromBase(base)->handle) : nullptr;
}

EdgeStreamBase* EdgeTtyWrapGetStreamBase(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok ||
      (type != napi_object && type != napi_function && type != napi_external)) {
    return nullptr;
  }
  // See EdgePipeWrapGetStreamBase: verify the type tag before downcasting the
  // unwrapped native pointer to avoid an out-of-bounds read on other wrap types.
  bool is_tty = false;
  if (napi_check_object_type_tag(env, value, &kTtyWrapTypeTag, &is_tty) != napi_ok || !is_tty) {
    return nullptr;
  }
  TtyWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) return nullptr;
  if (!wrap->initialized) return nullptr;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->handle);
  if (handle->data != wrap || handle->type != UV_TTY) return nullptr;
  return &wrap->base;
}

napi_value EdgeInstallTtyWrapBinding(napi_env env) {
  TtyBindingState& state = EnsureBindingState(env);
  napi_value cached = nullptr;
  if (state.binding_ref != nullptr) {
    napi_get_reference_value(env, state.binding_ref, &cached);
  }
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  constexpr napi_property_attributes kMethodAttrs =
      static_cast<napi_property_attributes>(napi_writable | napi_configurable);
  napi_property_descriptor tty_props[] = {
      {"getWindowSize", nullptr, TtyGetWindowSize, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"setRawMode", nullptr, TtySetRawMode, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"setBlocking", nullptr, TtySetBlocking, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"readStart", nullptr, TtyReadStart, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"readStop", nullptr, TtyReadStop, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeBuffer", nullptr, TtyWriteBuffer, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writev", nullptr, TtyWriteV, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"shutdown", nullptr, TtyShutdown, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeLatin1String", nullptr, TtyWriteString, nullptr, nullptr, nullptr, kMethodAttrs,
       const_cast<char*>("latin1")},
      {"writeUtf8String", nullptr, TtyWriteString, nullptr, nullptr, nullptr, kMethodAttrs,
       const_cast<char*>("utf8")},
      {"writeAsciiString", nullptr, TtyWriteString, nullptr, nullptr, nullptr, kMethodAttrs,
       const_cast<char*>("ascii")},
      {"writeUcs2String", nullptr, TtyWriteString, nullptr, nullptr, nullptr, kMethodAttrs,
       const_cast<char*>("ucs2")},
      {"close", nullptr, TtyClose, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"ref", nullptr, TtyRef, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"unref", nullptr, TtyUnref, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"hasRef", nullptr, TtyHasRef, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"getAsyncId", nullptr, TtyGetAsyncId, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"getProviderType", nullptr, TtyGetProviderType, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"asyncReset", nullptr, TtyAsyncReset, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"useUserBuffer", nullptr, TtyUseUserBuffer, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"onread", nullptr, nullptr, TtyGetOnRead, TtySetOnRead, nullptr, napi_default, nullptr},
      {"bytesRead", nullptr, nullptr, TtyBytesReadGetter, nullptr, nullptr, napi_default, nullptr},
      {"bytesWritten", nullptr, nullptr, TtyBytesWrittenGetter, nullptr, nullptr, napi_default, nullptr},
      {"fd", nullptr, nullptr, TtyFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"_externalStream", nullptr, nullptr, TtyExternalStreamGetter, nullptr, nullptr, napi_default, nullptr},
      {"writeQueueSize", nullptr, nullptr, TtyWriteQueueSizeGetter, nullptr, nullptr, napi_default, nullptr},
  };

  napi_value tty_ctor = nullptr;
  if (napi_define_class(env,
                        "TTY",
                        NAPI_AUTO_LENGTH,
                        TtyCtor,
                        nullptr,
                        sizeof(tty_props) / sizeof(tty_props[0]),
                        tty_props,
                        &tty_ctor) != napi_ok ||
      tty_ctor == nullptr) {
    return nullptr;
  }
  if (state.tty_ctor_ref != nullptr) napi_delete_reference(env, state.tty_ctor_ref);
  napi_create_reference(env, tty_ctor, 1, &state.tty_ctor_ref);

  napi_set_named_property(env, binding, "TTY", tty_ctor);
  napi_property_descriptor is_tty_desc = {
      "isTTY", nullptr, TtyIsTTY, nullptr, nullptr, nullptr, napi_default, nullptr,
  };
  napi_define_properties(env, binding, 1, &is_tty_desc);

  if (state.binding_ref != nullptr) napi_delete_reference(env, state.binding_ref);
  napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}
