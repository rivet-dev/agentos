#include "internal_binding/binding_initializers.h"

#include <algorithm>
#include <cstdint>
#include <cstdlib>

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "../edge_stream_base.h"
#include "../edge_stream_wrap.h"

namespace internal_binding {

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

struct StreamPipeBindingState {
  explicit StreamPipeBindingState(napi_env env_in) : env(env_in) {}
  ~StreamPipeBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

struct StreamPipeWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref source_ref = nullptr;
  napi_ref sink_ref = nullptr;
  EdgeStreamBase* source_base = nullptr;
  EdgeStreamBase* sink_base = nullptr;
  EdgeStreamListener readable_listener{};
  EdgeStreamListener writable_listener{};
  bool is_closed = true;
  bool is_reading = false;
  bool is_eof = false;
  bool source_destroyed = false;
  bool sink_destroyed = false;
  bool source_listener_attached = false;
  bool sink_listener_attached = false;
  bool onunpipe_scheduled = false;
  bool lifetime_refed = false;
  uint32_t pending_writes = 0;
  size_t wanted_data = 65536;
};

StreamPipeBindingState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<StreamPipeBindingState>(
      env, kEdgeEnvironmentSlotStreamPipeBindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

bool IsFunction(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

StreamPipeWrap* UnwrapPipe(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  StreamPipeWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

void FreeOwnedBuffer(napi_env /*env*/, void* data, void* /*hint*/) {
  free(data);
}

void RefPipeLifetime(StreamPipeWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr || wrap->lifetime_refed) return;
  uint32_t ignored = 0;
  if (napi_reference_ref(wrap->env, wrap->wrapper_ref, &ignored) == napi_ok) {
    wrap->lifetime_refed = true;
  }
}

void MaybeReleasePipeLifetime(StreamPipeWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr || !wrap->lifetime_refed) return;
  if (!wrap->is_closed || wrap->pending_writes != 0 || wrap->source_listener_attached ||
      wrap->sink_listener_attached || wrap->onunpipe_scheduled) {
    return;
  }
  wrap->lifetime_refed = false;
  uint32_t ignored = 0;
  (void)napi_reference_unref(wrap->env, wrap->wrapper_ref, &ignored);
}

void ClearLinks(napi_env env, napi_value self, StreamPipeWrap* wrap, bool clear_js_links) {
  if (clear_js_links && env != nullptr && self != nullptr) {
    napi_value null_value = nullptr;
    napi_get_null(env, &null_value);
    napi_value source = nullptr;
    napi_value sink = nullptr;
    (void)napi_get_named_property(env, self, "source", &source);
    (void)napi_get_named_property(env, self, "sink", &sink);
    (void)napi_set_named_property(env, self, "source", null_value);
    (void)napi_set_named_property(env, self, "sink", null_value);
    if (source != nullptr && !IsUndefined(env, source)) {
      (void)napi_set_named_property(env, source, "pipeTarget", null_value);
    }
    if (sink != nullptr && !IsUndefined(env, sink)) {
      (void)napi_set_named_property(env, sink, "pipeSource", null_value);
    }
  }
  if (wrap == nullptr) return;
  DeleteRefIfPresent(env, &wrap->source_ref);
  DeleteRefIfPresent(env, &wrap->sink_ref);
}

napi_value DeferredOnUnpipeCallback(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  void* data = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, &data);
  auto* wrap = static_cast<StreamPipeWrap*>(data);
  if (wrap == nullptr || wrap->env != env) return Undefined(env);
  wrap->onunpipe_scheduled = false;

  napi_value pipe_self = GetRefValue(env, wrap->wrapper_ref);
  if (pipe_self == nullptr) return Undefined(env);

  napi_value onunpipe = nullptr;
  if (napi_get_named_property(env, pipe_self, "onunpipe", &onunpipe) == napi_ok && IsFunction(env, onunpipe)) {
    napi_value ignored = nullptr;
    (void)napi_call_function(env, pipe_self, onunpipe, 0, nullptr, &ignored);
  }
  ClearLinks(env, pipe_self, wrap, true);
  if (wrap->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    (void)napi_reference_unref(env, wrap->wrapper_ref, &ignored);
  }
  MaybeReleasePipeLifetime(wrap);
  return Undefined(env);
}

void ScheduleOnUnpipe(napi_env env, StreamPipeWrap* wrap) {
  if (env == nullptr || wrap == nullptr || wrap->onunpipe_scheduled) return;
  bool wrapper_refed = false;
  if (wrap->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    if (napi_reference_ref(env, wrap->wrapper_ref, &ignored) == napi_ok) {
      wrapper_refed = true;
    }
  }
  napi_value global = GetGlobal(env);
  napi_value set_immediate = nullptr;
  napi_valuetype set_immediate_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(env, global, "setImmediate", &set_immediate) != napi_ok ||
      set_immediate == nullptr ||
      napi_typeof(env, set_immediate, &set_immediate_type) != napi_ok ||
      set_immediate_type != napi_function) {
    if (wrapper_refed && wrap->wrapper_ref != nullptr) {
      uint32_t ignored = 0;
      (void)napi_reference_unref(env, wrap->wrapper_ref, &ignored);
    }
    napi_value pipe_self = GetRefValue(env, wrap->wrapper_ref);
    if (pipe_self != nullptr) ClearLinks(env, pipe_self, wrap, true);
    MaybeReleasePipeLifetime(wrap);
    return;
  }

  napi_value callback = nullptr;
  if (napi_create_function(env,
                           "__edgeStreamPipeDeferredOnUnpipe",
                           NAPI_AUTO_LENGTH,
                           DeferredOnUnpipeCallback,
                           wrap,
                           &callback) != napi_ok ||
      callback == nullptr) {
    if (wrapper_refed && wrap->wrapper_ref != nullptr) {
      uint32_t ignored = 0;
      (void)napi_reference_unref(env, wrap->wrapper_ref, &ignored);
    }
    napi_value pipe_self = GetRefValue(env, wrap->wrapper_ref);
    if (pipe_self != nullptr) ClearLinks(env, pipe_self, wrap, true);
    MaybeReleasePipeLifetime(wrap);
    return;
  }

  wrap->onunpipe_scheduled = true;
  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(env, global, set_immediate, 1, argv, &ignored) != napi_ok) {
    wrap->onunpipe_scheduled = false;
    if (wrapper_refed && wrap->wrapper_ref != nullptr) {
      uint32_t ref_ignored = 0;
      (void)napi_reference_unref(env, wrap->wrapper_ref, &ref_ignored);
    }
    napi_value pipe_self = GetRefValue(env, wrap->wrapper_ref);
    if (pipe_self != nullptr) ClearLinks(env, pipe_self, wrap, true);
    MaybeReleasePipeLifetime(wrap);
  }
}

void RemoveListeners(StreamPipeWrap* wrap) {
  if (wrap == nullptr) return;
  if (wrap->source_listener_attached && wrap->source_base != nullptr) {
    (void)EdgeStreamBaseRemoveListener(wrap->source_base, &wrap->readable_listener);
    wrap->source_listener_attached = false;
  }
  if (wrap->sink_listener_attached && wrap->sink_base != nullptr && wrap->pending_writes == 0) {
    (void)EdgeStreamBaseRemoveListener(wrap->sink_base, &wrap->writable_listener);
    wrap->sink_listener_attached = false;
  }
}

void UnpipeInternal(napi_env env, napi_value self, StreamPipeWrap* wrap, bool in_deletion) {
  if (wrap == nullptr || wrap->is_closed) return;

  wrap->is_closed = true;
  wrap->is_reading = false;
  if (!in_deletion && !wrap->source_destroyed && wrap->source_base != nullptr) {
    (void)EdgeStreamBaseReadStop(wrap->source_base);
  }
  RemoveListeners(wrap);

  if (in_deletion) {
    ClearLinks(env, nullptr, wrap, false);
    return;
  }

  ScheduleOnUnpipe(env, wrap);
}

bool MakeBufferFromOwnedBytes(napi_env env, char* data, size_t len, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (env == nullptr || data == nullptr) return false;

  if (napi_create_external_buffer(env, len, data, FreeOwnedBuffer, nullptr, out) == napi_ok && *out != nullptr) {
    return true;
  }

  void* copy = nullptr;
  if (napi_create_buffer_copy(env, len, data, &copy, out) == napi_ok && *out != nullptr) {
    free(data);
    return true;
  }

  free(data);
  return false;
}

void ShutdownSinkAndUnpipe(StreamPipeWrap* wrap, napi_value self) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  if (wrap->sink_base != nullptr) {
    napi_value req_obj = EdgeCreateStreamReqObject(wrap->env);
    if (req_obj != nullptr) {
      const int rc = EdgeStreamBaseShutdownDirect(wrap->sink_base, req_obj);
      if (rc != 0 && rc != 1 && rc != UV_ENOTCONN) {
        EdgeStreamBaseInvokeReqOnComplete(wrap->env, req_obj, rc, nullptr, 0);
      }
    }
  }
  UnpipeInternal(wrap->env, self, wrap, false);
}

bool ReadableOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf);

void StartReading(StreamPipeWrap* wrap, size_t suggested_size) {
  if (wrap == nullptr || wrap->is_closed || wrap->source_base == nullptr || wrap->source_destroyed ||
      wrap->sink_destroyed || wrap->pending_writes != 0 || wrap->is_reading) {
    return;
  }

  wrap->wanted_data = suggested_size;
  wrap->is_reading = true;
  const int rc = EdgeStreamBaseReadStart(wrap->source_base);
  if (rc == 0) return;

  wrap->is_reading = false;
  ReadableOnRead(&wrap->readable_listener, rc == 1 ? UV_EOF : rc, nullptr);
}

uv_buf_t ReadableAlloc(size_t suggested_size) {
  char* storage = static_cast<char*>(malloc(suggested_size));
  if (storage == nullptr && suggested_size > 0) return uv_buf_init(nullptr, 0);
  return uv_buf_init(storage, static_cast<unsigned int>(suggested_size));
}

bool ReadableOnAlloc(EdgeStreamListener* listener, size_t suggested_size, uv_buf_t* out) {
  if (listener == nullptr || out == nullptr) return false;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr) return false;
  const size_t wanted = wrap->wanted_data == 0 ? suggested_size : std::min(suggested_size, wrap->wanted_data);
  *out = ReadableAlloc(wanted == 0 ? suggested_size : wanted);
  return out->base != nullptr || out->len == 0;
}

bool WritableOnAfterWrite(EdgeStreamListener* listener, napi_value req_obj, int status);
bool WritableOnWantsWrite(EdgeStreamListener* listener, size_t suggested_size);

bool ProcessData(StreamPipeWrap* wrap, ssize_t nread, const uv_buf_t* buf) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->sink_base == nullptr || buf == nullptr || buf->base == nullptr ||
      nread <= 0) {
    return false;
  }

  napi_value payload = nullptr;
  if (!MakeBufferFromOwnedBytes(wrap->env, buf->base, static_cast<size_t>(nread), &payload)) {
    return false;
  }

  napi_value req_obj = EdgeCreateStreamReqObject(wrap->env);
  if (req_obj == nullptr) return false;

  bool async = false;
  wrap->pending_writes++;
  const int rc = EdgeStreamBaseWriteBufferDirect(wrap->sink_base, req_obj, payload, &async);
  if (!async) {
    return WritableOnAfterWrite(&wrap->writable_listener, req_obj, rc);
  }

  wrap->is_reading = false;
  if (wrap->source_base != nullptr) {
    (void)EdgeStreamBaseReadStop(wrap->source_base);
  }
  return rc == 0;
}

bool ReadableOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf) {
  if (listener == nullptr) return false;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr || wrap->env == nullptr) return false;

  if (nread < 0) {
    wrap->is_reading = false;
    wrap->is_eof = true;
    if (buf != nullptr && buf->base != nullptr) free(buf->base);
    if (listener->previous != nullptr && listener->previous->on_read != nullptr) {
      uv_buf_t empty = uv_buf_init(nullptr, 0);
      (void)listener->previous->on_read(listener->previous, nread, &empty);
    }
    if (wrap->pending_writes == 0) {
      napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
      ShutdownSinkAndUnpipe(wrap, self);
    }
    return true;
  }

  return ProcessData(wrap, nread, buf);
}

bool WritableOnAfterWrite(EdgeStreamListener* listener, napi_value req_obj, int status) {
  if (listener == nullptr) return false;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr || wrap->env == nullptr) return false;

  if (wrap->pending_writes > 0) wrap->pending_writes--;

  if (wrap->is_closed) {
    RemoveListeners(wrap);
    MaybeReleasePipeLifetime(wrap);
    return true;
  }

  if (wrap->is_eof) {
    napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
    ShutdownSinkAndUnpipe(wrap, self);
    return true;
  }

  if (status != 0) {
    (void)EdgeStreamPassAfterWrite(listener, req_obj, status);
    napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
    UnpipeInternal(wrap->env, self, wrap, false);
    return true;
  }

  StartReading(wrap, 65536);
  return true;
}

bool WritableOnAfterShutdown(EdgeStreamListener* listener, napi_value req_obj, int status) {
  if (listener == nullptr) return false;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr) return false;
  (void)EdgeStreamPassAfterShutdown(listener, req_obj, status);
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  UnpipeInternal(wrap->env, self, wrap, false);
  return true;
}

bool WritableOnWantsWrite(EdgeStreamListener* listener, size_t suggested_size) {
  if (listener == nullptr) return false;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr) return false;
  wrap->wanted_data = suggested_size;
  StartReading(wrap, suggested_size);
  return true;
}

void ReadableOnClose(EdgeStreamListener* listener) {
  if (listener == nullptr) return;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr) return;
  wrap->source_destroyed = true;
  if (!wrap->is_eof) {
    (void)ReadableOnRead(listener, UV_EPIPE, nullptr);
  }
}

void WritableOnClose(EdgeStreamListener* listener) {
  if (listener == nullptr) return;
  auto* wrap = static_cast<StreamPipeWrap*>(listener->data);
  if (wrap == nullptr) return;
  wrap->sink_destroyed = true;
  wrap->is_eof = true;
  wrap->pending_writes = 0;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  UnpipeInternal(wrap->env, self, wrap, false);
}

void StreamPipeFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<StreamPipeWrap*>(data);
  if (wrap == nullptr) return;
  UnpipeInternal(env, nullptr, wrap, true);
  wrap->lifetime_refed = false;
  DeleteRefIfPresent(env, &wrap->wrapper_ref);
  delete wrap;
}

napi_value StreamPipeCtor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  auto* wrap = new StreamPipeWrap();
  wrap->env = env;
  wrap->readable_listener.on_alloc = ReadableOnAlloc;
  wrap->readable_listener.on_read = ReadableOnRead;
  wrap->readable_listener.on_close = ReadableOnClose;
  wrap->readable_listener.data = wrap;
  wrap->writable_listener.on_after_write = WritableOnAfterWrite;
  wrap->writable_listener.on_after_shutdown = WritableOnAfterShutdown;
  wrap->writable_listener.on_wants_write = WritableOnWantsWrite;
  wrap->writable_listener.on_close = WritableOnClose;
  wrap->writable_listener.data = wrap;

  if (napi_wrap(env, self, wrap, StreamPipeFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }

  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  napi_value source = argc >= 1 && argv[0] != nullptr ? argv[0] : null_value;
  napi_value sink = argc >= 2 && argv[1] != nullptr ? argv[1] : null_value;
  if (source != nullptr && !IsUndefined(env, source)) {
    (void)napi_create_reference(env, source, 1, &wrap->source_ref);
    wrap->source_base = EdgeStreamBaseFromValue(env, source);
  }
  if (sink != nullptr && !IsUndefined(env, sink)) {
    (void)napi_create_reference(env, sink, 1, &wrap->sink_ref);
    wrap->sink_base = EdgeStreamBaseFromValue(env, sink);
  }

  if (wrap->source_base != nullptr) {
    (void)EdgeStreamBasePushListener(wrap->source_base, &wrap->readable_listener);
    wrap->source_listener_attached = true;
  }
  if (wrap->sink_base != nullptr) {
    (void)EdgeStreamBasePushListener(wrap->sink_base, &wrap->writable_listener);
    wrap->sink_listener_attached = true;
  }

  (void)napi_set_named_property(env, self, "source", source != nullptr ? source : null_value);
  (void)napi_set_named_property(env, self, "sink", sink != nullptr ? sink : null_value);
  if (source != nullptr && !IsUndefined(env, source)) {
    (void)napi_set_named_property(env, source, "pipeTarget", self);
  }
  if (sink != nullptr && !IsUndefined(env, sink)) {
    (void)napi_set_named_property(env, sink, "pipeSource", self);
  }
  (void)napi_set_named_property(env, self, "onunpipe", Undefined(env));
  return self;
}

napi_value StreamPipeStart(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamPipeWrap* wrap = UnwrapPipe(env, self);
  if (wrap == nullptr) return Undefined(env);

  RefPipeLifetime(wrap);
  wrap->is_closed = false;
  if (wrap->source_base == nullptr || wrap->sink_base == nullptr) {
    UnpipeInternal(env, self, wrap, false);
    return Undefined(env);
  }

  (void)WritableOnWantsWrite(&wrap->writable_listener, 65536);
  return Undefined(env);
}

napi_value StreamPipeUnpipe(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamPipeWrap* wrap = UnwrapPipe(env, self);
  if (wrap != nullptr) {
    UnpipeInternal(env, self, wrap, false);
  }
  return Undefined(env);
}

napi_value StreamPipeIsClosed(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamPipeWrap* wrap = UnwrapPipe(env, self);
  napi_value out = nullptr;
  napi_get_boolean(env, wrap != nullptr ? wrap->is_closed : true, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamPipePendingWrites(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamPipeWrap* wrap = UnwrapPipe(env, self);
  napi_value out = nullptr;
  napi_create_uint32(env, wrap != nullptr ? wrap->pending_writes : 0, &out);
  return out != nullptr ? out : Undefined(env);
}

}  // namespace

napi_value InitStreamPipe(napi_env env) {
  StreamPipeBindingState& state = EnsureState(env);
  napi_value cached = GetRefValue(env, state.binding_ref);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  napi_property_descriptor props[] = {
      {"unpipe", nullptr, StreamPipeUnpipe, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"start", nullptr, StreamPipeStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"isClosed", nullptr, StreamPipeIsClosed, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"pendingWrites", nullptr, StreamPipePendingWrites, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };
  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "StreamPipe",
                        NAPI_AUTO_LENGTH,
                        StreamPipeCtor,
                        nullptr,
                        sizeof(props) / sizeof(props[0]),
                        props,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return Undefined(env);
  }

  napi_set_named_property(env, binding, "StreamPipe", ctor);
  DeleteRefIfPresent(env, &state.binding_ref);
  (void)napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}

}  // namespace internal_binding
