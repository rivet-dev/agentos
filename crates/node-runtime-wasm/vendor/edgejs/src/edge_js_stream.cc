#include "edge_js_stream.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

#include <uv.h>

#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_runtime.h"
#include "edge_stream_base.h"
#include "edge_stream_wrap.h"

namespace {

struct JsStreamWrap {
  napi_env env = nullptr;
  EdgeStreamBase base{};
};

JsStreamWrap* FromBase(EdgeStreamBase* base) {
  if (base == nullptr) return nullptr;
  return reinterpret_cast<JsStreamWrap*>(reinterpret_cast<char*>(base) - offsetof(JsStreamWrap, base));
}

void JsStreamDestroy(EdgeStreamBase* base) {
  delete FromBase(base);
}

const EdgeStreamBaseOps kJsStreamOps = {
    nullptr,
    nullptr,
    nullptr,
    JsStreamDestroy,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
};

bool GetThisAndWrap(napi_env env,
                    napi_callback_info info,
                    size_t* argc_out,
                    napi_value* argv,
                    napi_value* self_out,
                    JsStreamWrap** wrap_out) {
  size_t argc = argc_out != nullptr ? *argc_out : 0;
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr) return false;
  if (argc_out != nullptr) *argc_out = argc;
  if (self_out != nullptr) *self_out = self;
  if (wrap_out != nullptr) {
    *wrap_out = nullptr;
    napi_unwrap(env, self, reinterpret_cast<void**>(wrap_out));
  }
  return true;
}

bool IsFunction(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

napi_value GetNamedValue(napi_env env, napi_value obj, const char* key) {
  if (env == nullptr || obj == nullptr || key == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_named_property(env, obj, key, &out) != napi_ok) return nullptr;
  return out;
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

bool GetStringProperty(napi_env env, napi_value obj, const char* key, std::string* out) {
  if (env == nullptr || obj == nullptr || key == nullptr || out == nullptr) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, obj, key, &value) != napi_ok || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

bool ClearPendingExceptionIfCode(napi_env env, const char* code) {
  if (env == nullptr || code == nullptr) return false;
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return false;

  napi_value exception = nullptr;
  if (napi_get_and_clear_last_exception(env, &exception) != napi_ok || exception == nullptr) {
    return false;
  }

  std::string actual_code;
  if (GetStringProperty(env, exception, "code", &actual_code) && actual_code == code) {
    return true;
  }

  (void)napi_throw(env, exception);
  return false;
}

int32_t CallMethodReturningInt32(napi_env env,
                                 napi_value self,
                                 int64_t async_id,
                                 const char* method_name,
                                 size_t argc,
                                 napi_value* argv,
                                 int32_t fallback) {
  (void)async_id;
  if (env == nullptr || self == nullptr || method_name == nullptr) return fallback;
  napi_value fn = nullptr;
  if (napi_get_named_property(env, self, method_name, &fn) != napi_ok || !IsFunction(env, fn)) {
    return fallback;
  }
  napi_value result = nullptr;
  if (napi_call_function(env, self, fn, argc, argv, &result) != napi_ok || result == nullptr) {
    return fallback;
  }
  int32_t out = fallback;
  if (napi_get_value_int32(env, result, &out) != napi_ok) return fallback;
  return out;
}

napi_value BuildWriteArray(napi_env env, const std::vector<std::vector<uint8_t>>& chunks) {
  napi_value array = nullptr;
  if (napi_create_array_with_length(env, chunks.size(), &array) != napi_ok || array == nullptr) return nullptr;
  for (size_t i = 0; i < chunks.size(); ++i) {
    napi_value buffer = nullptr;
    void* out = nullptr;
    const auto& chunk = chunks[i];
    if (napi_create_buffer_copy(env, chunk.size(), chunk.data(), &out, &buffer) != napi_ok || buffer == nullptr) {
      return nullptr;
    }
    napi_set_element(env, array, static_cast<uint32_t>(i), buffer);
  }
  return array;
}

napi_value CallOnWrite(napi_env env,
                       JsStreamWrap* wrap,
                       napi_value self,
                       napi_value req_obj,
                       const std::vector<std::vector<uint8_t>>& chunks,
                       size_t total_bytes) {
  napi_value array = BuildWriteArray(env, chunks);
  if (array == nullptr) {
    int32_t* state = EdgeGetStreamBaseState(env);
    if (state != nullptr) {
      state[kEdgeBytesWritten] = 0;
      state[kEdgeLastWriteWasAsync] = 0;
    }
    return EdgeStreamBaseMakeInt32(env, UV_EPROTO);
  }

  napi_value argv[2] = {req_obj != nullptr ? req_obj : EdgeStreamBaseUndefined(env), array};
  EdgeStreamReqActivate(env, req_obj, kEdgeProviderWriteWrap, wrap->base.async_id);
  int32_t status = CallMethodReturningInt32(env, self, wrap->base.async_id, "onwrite", 2, argv, UV_EPROTO);
  if (status == UV_EPROTO && ClearPendingExceptionIfCode(env, "ERR_STREAM_DESTROYED")) {
    status = UV_EPIPE;
  }
  int32_t* state = EdgeGetStreamBaseState(env);
  if (state != nullptr) {
    state[kEdgeBytesWritten] = static_cast<int32_t>(status == 0 ? total_bytes : 0);
    state[kEdgeLastWriteWasAsync] = status == 0 ? 1 : 0;
  }
  if (status == 0) {
    wrap->base.bytes_written += total_bytes;
  } else {
    EdgeStreamReqMarkDone(env, req_obj);
  }
  return EdgeStreamBaseMakeInt32(env, status);
}

napi_value JsStreamCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (self == nullptr) return nullptr;

  auto* wrap = new JsStreamWrap();
  wrap->env = env;
  EdgeStreamBaseInit(&wrap->base, env, &kJsStreamOps, kEdgeProviderJsStream);
  napi_wrap(env, self, wrap, [](napi_env finalize_env, void* data, void*) {
    auto* js_wrap = static_cast<JsStreamWrap*>(data);
    if (js_wrap == nullptr) return;
    EdgeStreamBaseFinalize(&js_wrap->base);
  }, nullptr, &wrap->base.wrapper_ref);
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->base.wrapper_ref);
  EdgeStreamBaseSetInitialStreamProperties(&wrap->base, false, false);
  // Node's native JSStream does not expose an enumerable `reading` own property,
  // so keep it writable/configurable but hide it from default inspection.
  napi_property_descriptor reading_desc = {};
  reading_desc.utf8name = "reading";
  reading_desc.value = EdgeStreamBaseMakeBool(env, false);
  reading_desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
  (void)napi_define_properties(env, self, 1, &reading_desc);
  return self;
}

napi_value JsStreamReadStart(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, nullptr, nullptr, &self, &wrap) || self == nullptr) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }
  int32_t status = CallMethodReturningInt32(env, self, wrap->base.async_id, "onreadstart", 0, nullptr, UV_EPROTO);
  if (status == UV_EALREADY) status = 0;
  EdgeStreamBaseSetReading(&wrap->base, status == 0);
  return EdgeStreamBaseMakeInt32(env, status);
}

napi_value JsStreamReadStop(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, nullptr, nullptr, &self, &wrap) || self == nullptr) {
    return EdgeStreamBaseMakeInt32(env, UV_EBADF);
  }
  int32_t status = CallMethodReturningInt32(env, self, wrap->base.async_id, "onreadstop", 0, nullptr, UV_EPROTO);
  if (status == UV_EALREADY) status = 0;
  EdgeStreamBaseSetReading(&wrap->base, false);
  return EdgeStreamBaseMakeInt32(env, status);
}

napi_value JsStreamShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, &self, &wrap) || self == nullptr || argc < 1) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  napi_value cb_argv[1] = {argv[0]};
  EdgeStreamReqActivate(env, argv[0], kEdgeProviderShutdownWrap, wrap->base.async_id);
  int32_t status = CallMethodReturningInt32(env, self, wrap->base.async_id, "onshutdown", 1, cb_argv, UV_EPROTO);
  if (status != 0) {
    EdgeStreamReqMarkDone(env, argv[0]);
  }
  return EdgeStreamBaseMakeInt32(env, status);
}

napi_value JsStreamWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, &self, &wrap) || self == nullptr || wrap == nullptr || argc < 2) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  EdgeStreamBaseExtractByteSpan(env, argv[1], &data, &len, &refable, &temp_utf8);
  std::vector<std::vector<uint8_t>> chunks;
  chunks.emplace_back(data, data + len);
  return CallOnWrite(env, wrap, self, argv[0], chunks, len);
}

napi_value JsStreamWriteString(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, &self, &wrap) || self == nullptr || wrap == nullptr || argc < 2) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  std::string text = ValueToUtf8(env, argv[1]);
  std::vector<std::vector<uint8_t>> chunks;
  chunks.emplace_back(reinterpret_cast<const uint8_t*>(text.data()),
                      reinterpret_cast<const uint8_t*>(text.data()) + text.size());
  return CallOnWrite(env, wrap, self, argv[0], chunks, text.size());
}

napi_value JsStreamWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  JsStreamWrap* wrap = nullptr;
  napi_value self = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, &self, &wrap) || self == nullptr || wrap == nullptr || argc < 2) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }

  bool all_buffers = false;
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_bool(env, argv[2], &all_buffers);

  uint32_t raw_len = 0;
  if (napi_get_array_length(env, argv[1], &raw_len) != napi_ok) return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  const uint32_t count = all_buffers ? raw_len : (raw_len / 2);
  if (count == 0) {
    int32_t* state = EdgeGetStreamBaseState(env);
    if (state != nullptr) {
      state[kEdgeBytesWritten] = 0;
      state[kEdgeLastWriteWasAsync] = 0;
    }
    return EdgeStreamBaseMakeInt32(env, 0);
  }

  std::vector<std::vector<uint8_t>> chunks;
  chunks.reserve(count);
  size_t total = 0;
  for (uint32_t i = 0; i < count; ++i) {
    napi_value chunk = nullptr;
    napi_get_element(env, argv[1], all_buffers ? i : (i * 2), &chunk);
    const uint8_t* data = nullptr;
    size_t len = 0;
    bool refable = false;
    std::string temp_utf8;
    EdgeStreamBaseExtractByteSpan(env, chunk, &data, &len, &refable, &temp_utf8);
    chunks.emplace_back(data, data + len);
    total += len;
  }
  return CallOnWrite(env, wrap, self, argv[0], chunks, total);
}

napi_value JsStreamUseUserBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  JsStreamWrap* wrap = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, nullptr, &wrap) || wrap == nullptr || argc < 1) {
    return EdgeStreamBaseMakeInt32(env, UV_EINVAL);
  }
  return EdgeStreamBaseUseUserBuffer(&wrap->base, argv[0]);
}

napi_value JsStreamReadBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  JsStreamWrap* wrap = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, nullptr, &wrap) || wrap == nullptr || argc < 1) {
    return EdgeStreamBaseUndefined(env);
  }

  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  EdgeStreamBaseExtractByteSpan(env, argv[0], &data, &len, &refable, &temp_utf8);

  while (len != 0) {
    uv_buf_t buf = uv_buf_init(nullptr, 0);
    if (!EdgeStreamEmitAlloc(&wrap->base.listener_state, len, &buf) || buf.base == nullptr || buf.len == 0) {
      break;
    }
    size_t available = len;
    if (buf.len < available) available = buf.len;
    memcpy(buf.base, data, available);
    data += available;
    len -= available;
    wrap->base.bytes_read += available;
    if (!EdgeStreamEmitRead(&wrap->base.listener_state, static_cast<ssize_t>(available), &buf) && buf.base != nullptr) {
      free(buf.base);
    }
  }
  return EdgeStreamBaseUndefined(env);
}

napi_value JsStreamEmitEOF(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  if (!GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap) || wrap == nullptr) {
    return EdgeStreamBaseUndefined(env);
  }
  EdgeStreamEmitRead(&wrap->base.listener_state, UV_EOF, nullptr);
  return EdgeStreamBaseUndefined(env);
}

napi_value JsStreamFinishWrite(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  JsStreamWrap* wrap = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, nullptr, &wrap) || wrap == nullptr || argc < 2) {
    return EdgeStreamBaseUndefined(env);
  }
  int32_t status = 0;
  if (napi_get_value_int32(env, argv[1], &status) != napi_ok) status = UV_EINVAL;
  EdgeStreamBaseEmitAfterWrite(&wrap->base, argv[0], status);
  return EdgeStreamBaseUndefined(env);
}

napi_value JsStreamFinishShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  JsStreamWrap* wrap = nullptr;
  if (!GetThisAndWrap(env, info, &argc, argv, nullptr, &wrap) || wrap == nullptr || argc < 2) {
    return EdgeStreamBaseUndefined(env);
  }
  if (!EdgeEnvironmentCanCallIntoJs(env)) {
    return EdgeStreamBaseUndefined(env);
  }
  if (wrap->base.finalized || wrap->base.destroyed) {
    return EdgeStreamBaseUndefined(env);
  }
  int32_t status = 0;
  if (napi_get_value_int32(env, argv[1], &status) != napi_ok) status = UV_EINVAL;
  EdgeStreamBaseEmitAfterShutdown(&wrap->base, env, argv[0], status);
  return EdgeStreamBaseUndefined(env);
}

napi_value JsStreamGetOnRead(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetOnRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamSetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, &argc, argv, nullptr, &wrap);
  return EdgeStreamBaseSetOnRead(wrap != nullptr ? &wrap->base : nullptr, argc > 0 ? argv[0] : nullptr);
}

napi_value JsStreamGetAsyncId(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetAsyncId(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamAsyncReset(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseAsyncReset(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamGetProviderType(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetProviderType(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamBytesReadGetter(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesRead(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamBytesWrittenGetter(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetBytesWritten(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamFdGetter(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetFd(wrap != nullptr ? &wrap->base : nullptr);
}

napi_value JsStreamExternalStreamGetter(napi_env env, napi_callback_info info) {
  JsStreamWrap* wrap = nullptr;
  GetThisAndWrap(env, info, nullptr, nullptr, nullptr, &wrap);
  return EdgeStreamBaseGetExternal(wrap != nullptr ? &wrap->base : nullptr);
}

}  // namespace

napi_value EdgeInstallJsStreamBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  constexpr napi_property_attributes kMethodAttrs =
      static_cast<napi_property_attributes>(napi_writable | napi_configurable);

  napi_property_descriptor js_stream_props[] = {
      {"readStart", nullptr, JsStreamReadStart, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"readStop", nullptr, JsStreamReadStop, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"shutdown", nullptr, JsStreamShutdown, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"useUserBuffer", nullptr, JsStreamUseUserBuffer, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writev", nullptr, JsStreamWritev, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeBuffer", nullptr, JsStreamWriteBuffer, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeAsciiString", nullptr, JsStreamWriteString, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeUtf8String", nullptr, JsStreamWriteString, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeUcs2String", nullptr, JsStreamWriteString, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"writeLatin1String", nullptr, JsStreamWriteString, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"finishWrite", nullptr, JsStreamFinishWrite, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"finishShutdown", nullptr, JsStreamFinishShutdown, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"readBuffer", nullptr, JsStreamReadBuffer, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"emitEOF", nullptr, JsStreamEmitEOF, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"getAsyncId", nullptr, JsStreamGetAsyncId, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"getProviderType", nullptr, JsStreamGetProviderType, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"asyncReset", nullptr, JsStreamAsyncReset, nullptr, nullptr, nullptr, kMethodAttrs, nullptr},
      {"onread", nullptr, nullptr, JsStreamGetOnRead, JsStreamSetOnRead, nullptr, napi_default, nullptr},
      {"bytesRead", nullptr, nullptr, JsStreamBytesReadGetter, nullptr, nullptr, napi_default, nullptr},
      {"bytesWritten", nullptr, nullptr, JsStreamBytesWrittenGetter, nullptr, nullptr, napi_default, nullptr},
      {"fd", nullptr, nullptr, JsStreamFdGetter, nullptr, nullptr, napi_default, nullptr},
      {"_externalStream", nullptr, nullptr, JsStreamExternalStreamGetter, nullptr, nullptr, napi_default, nullptr},
  };

  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "JSStream",
                        NAPI_AUTO_LENGTH,
                        JsStreamCtor,
                        nullptr,
                        sizeof(js_stream_props) / sizeof(js_stream_props[0]),
                        js_stream_props,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "JSStream", ctor);
  return binding;
}

int EdgeJsStreamWriteBuffer(EdgeStreamBase* base,
                           napi_value req_obj,
                           napi_value payload,
                           bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  auto* wrap = FromBase(base);
  if (wrap == nullptr || base == nullptr || base->env == nullptr) return UV_EBADF;

  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return UV_EBADF;

  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  EdgeStreamBaseExtractByteSpan(base->env, payload, &data, &len, &refable, &temp_utf8);

  std::vector<std::vector<uint8_t>> chunks;
  chunks.emplace_back(data, data + len);

  napi_value status_value = CallOnWrite(base->env, wrap, self, req_obj, chunks, len);
  int32_t status = UV_EPROTO;
  if (status_value == nullptr || napi_get_value_int32(base->env, status_value, &status) != napi_ok) {
    return UV_EPROTO;
  }

  if (async_out != nullptr && status == 0) {
    *async_out = true;
  }

  return status;
}
