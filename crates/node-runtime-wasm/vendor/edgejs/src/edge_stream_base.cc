#include "edge_stream_base.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

#include "binding_registry/binding_registry.h"
#include "internal_binding/helpers.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_module_loader.h"
#include "edge_pipe_wrap.h"
#include "edge_runtime.h"
#include "edge_js_stream.h"
#include "edge_stream_wrap.h"
#include "edge_tcp_wrap.h"
#include "edge_trace.h"
#include "edge_tty_wrap.h"

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

bool TraceNetEnabled() {
  return EDGE_TRACE_ENABLED("EDGE_TRACE_NET");
}

void ClearPendingException(napi_env env) {
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  napi_get_and_clear_last_exception(env, &ignored);
}

struct StreamSymbolCache {
  explicit StreamSymbolCache(napi_env env_in) : env(env_in) {}
  ~StreamSymbolCache() {
    DeleteRefIfPresent(env, &symbols_ref);
    DeleteRefIfPresent(env, &owner_symbol_ref);
    DeleteRefIfPresent(env, &handle_onclose_symbol_ref);
  }

  napi_env env = nullptr;
  napi_ref symbols_ref = nullptr;
  napi_ref owner_symbol_ref = nullptr;
  napi_ref handle_onclose_symbol_ref = nullptr;
};

napi_value GetOwnerSymbol(napi_env env);
napi_value GetRefValue(napi_env env, napi_ref ref);

struct LibuvWriteReq;
struct LibuvShutdownReq;

bool StreamBaseHasRefForTracking(void* data) {
  return EdgeStreamBaseHasRef(static_cast<EdgeStreamBase*>(data));
}

void CloseStreamBaseForCleanup(void* data) {
  auto* base = static_cast<EdgeStreamBase*>(data);
  if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr || base->ops->on_close == nullptr) {
    return;
  }
  uv_handle_t* handle = base->ops->get_handle(base);
  if (handle == nullptr || base->closed || base->closing || uv_is_closing(handle) != 0) return;
  base->closing = true;
  uv_close(handle, base->ops->on_close);
}

napi_value StreamBaseGetActiveOwner(napi_env env, void* data) {
  auto* base = static_cast<EdgeStreamBase*>(data);
  if (base == nullptr) return nullptr;
  napi_value wrapper = EdgeStreamBaseGetWrapper(base);
  if (wrapper == nullptr) return nullptr;

  napi_value owner_symbol = GetOwnerSymbol(env);
  if (owner_symbol == nullptr) return nullptr;

  napi_value owner = nullptr;
  if (napi_get_property(env, wrapper, owner_symbol, &owner) != napi_ok || owner == nullptr) return nullptr;
  napi_valuetype owner_type = napi_undefined;
  if (napi_typeof(env, owner, &owner_type) != napi_ok) return nullptr;
  if (owner_type == napi_undefined || owner_type == napi_null) return nullptr;
  return owner;
}

napi_value LibuvWriteReqGetOwner(napi_env env, void* data);
void CancelLibuvWriteReq(void* data);
napi_value LibuvShutdownReqGetOwner(napi_env env, void* data);
void CancelLibuvShutdownReq(void* data);

const char* ActiveResourceNameForProvider(int32_t provider_type) {
  switch (provider_type) {
    case kEdgeProviderTcpWrap:
      return "TCPSocketWrap";
    case kEdgeProviderTcpServerWrap:
      return "TCPServerWrap";
    case kEdgeProviderPipeWrap:
      return "PipeWrap";
    case kEdgeProviderPipeServerWrap:
      return "PipeServerWrap";
    case kEdgeProviderJsStream:
      return "JSSTREAM";
    case kEdgeProviderTlsWrap:
      return "TLSWrap";
    default:
      return "STREAM";
  }
}

struct LibuvWriteReq {
  uv_write_t req{};
  napi_env env = nullptr;
  EdgeStreamBase* base = nullptr;
  napi_ref req_obj_ref = nullptr;
  void* active_request_token = nullptr;
  napi_ref send_handle_ref = nullptr;
  uv_buf_t* bufs = nullptr;
  uv_buf_t* bufs_storage = nullptr;
  napi_ref* bufs_refs = nullptr;
  char** bufs_allocs = nullptr;
  uint32_t nbufs = 0;
  uint32_t nbufs_storage = 0;
};

struct LibuvShutdownReq {
  uv_shutdown_t req{};
  napi_env env = nullptr;
  EdgeStreamBase* base = nullptr;
  napi_ref req_obj_ref = nullptr;
  void* active_request_token = nullptr;
};

napi_value LibuvWriteReqGetOwner(napi_env env, void* data) {
  auto* req = static_cast<LibuvWriteReq*>(data);
  return req != nullptr ? GetRefValue(env, req->req_obj_ref) : nullptr;
}

void CancelLibuvWriteReq(void* data) {
  auto* req = static_cast<LibuvWriteReq*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

napi_value LibuvShutdownReqGetOwner(napi_env env, void* data) {
  auto* req = static_cast<LibuvShutdownReq*>(data);
  return req != nullptr ? GetRefValue(env, req->req_obj_ref) : nullptr;
}

void CancelLibuvShutdownReq(void* data) {
  auto* req = static_cast<LibuvShutdownReq*>(data);
  if (req == nullptr) return;
  (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->req));
}

struct StreamBaseEnvState {
  explicit StreamBaseEnvState(napi_env /*env_in*/) {}

  EdgeStreamBase* head = nullptr;
  bool cleanup_started = false;
};

std::mutex g_stream_base_env_states_mutex;

StreamSymbolCache& GetSymbolCache(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<StreamSymbolCache>(
      env, kEdgeEnvironmentSlotStreamSymbolCache);
}

StreamBaseEnvState* GetStreamBaseState(napi_env env) {
  return EdgeEnvironmentGetSlotData<StreamBaseEnvState>(
      env, kEdgeEnvironmentSlotStreamBaseEnvState);
}

StreamBaseEnvState& EnsureStreamBaseState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<StreamBaseEnvState>(
      env, kEdgeEnvironmentSlotStreamBaseEnvState);
}

void UnlinkStreamBaseLocked(StreamBaseEnvState* state, EdgeStreamBase* base) {
  if (state == nullptr || base == nullptr || !base->attached) return;
  if (base->prev != nullptr) {
    base->prev->next = base->next;
  } else if (state->head == base) {
    state->head = base->next;
  }
  if (base->next != nullptr) {
    base->next->prev = base->prev;
  }
  base->prev = nullptr;
  base->next = nullptr;
  base->attached = false;
}

void StreamBaseDetach(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr || !base->attached) return;
  std::lock_guard<std::mutex> lock(g_stream_base_env_states_mutex);
  auto* state = GetStreamBaseState(base->env);
  if (state == nullptr) {
    base->prev = nullptr;
    base->next = nullptr;
    base->attached = false;
    return;
  }
  UnlinkStreamBaseLocked(state, base);
}

void RunStreamBaseEnvCleanup(napi_env env);

void StreamBaseMaybeAttach(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr || base->attached) return;
  if (base->ops == nullptr || base->ops->get_handle == nullptr || base->ops->on_close == nullptr) return;
  uv_handle_t* handle = base->ops->get_handle(base);
  if (handle == nullptr) return;
  std::lock_guard<std::mutex> lock(g_stream_base_env_states_mutex);
  auto& state = EnsureStreamBaseState(base->env);
  if (state.cleanup_started) return;
  base->prev = nullptr;
  base->next = state.head;
  if (state.head != nullptr) {
    state.head->prev = base;
  }
  state.head = base;
  base->attached = true;
}

void RunStreamBaseEnvCleanup(napi_env env) {
  if (env == nullptr) return;
  std::vector<EdgeStreamBase*> bases_to_close;
  {
    std::lock_guard<std::mutex> lock(g_stream_base_env_states_mutex);
    auto* state = GetStreamBaseState(env);
    if (state == nullptr) return;
    state->cleanup_started = true;
    for (EdgeStreamBase* base = state->head; base != nullptr; base = base->next) {
      bases_to_close.push_back(base);
    }
  }

  for (EdgeStreamBase* base : bases_to_close) {
    if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr ||
        base->ops->on_close == nullptr || base->closed || base->closing) {
      continue;
    }
    uv_handle_t* handle = base->ops->get_handle(base);
    if (handle == nullptr || uv_is_closing(handle) != 0) continue;
    base->closing = true;
    uv_close(handle, base->ops->on_close);
  }

  uv_loop_t* loop = EdgeGetExistingEnvLoop(env);
  while (loop != nullptr) {
    bool empty = false;
    {
      std::lock_guard<std::mutex> lock(g_stream_base_env_states_mutex);
      auto* state = GetStreamBaseState(env);
      empty = state == nullptr || state->head == nullptr;
    }
    if (empty) break;
    (void)uv_run(loop, UV_RUN_ONCE);
  }
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

napi_value ResolveInternalBinding(napi_env env, const char* name) {
  if (env == nullptr || name == nullptr) return nullptr;
  return edge::binding_registry::Get(env, name);
}

napi_value GetSymbolsBinding(napi_env env) {
  StreamSymbolCache& cache = GetSymbolCache(env);
  napi_value binding = GetRefValue(env, cache.symbols_ref);
  if (binding != nullptr) return binding;

  binding = ResolveInternalBinding(env, "symbols");
  if (binding == nullptr) return nullptr;

  if (cache.symbols_ref != nullptr) {
    napi_delete_reference(env, cache.symbols_ref);
    cache.symbols_ref = nullptr;
  }
  napi_create_reference(env, binding, 1, &cache.symbols_ref);
  return binding;
}

napi_value GetNamedCachedSymbol(napi_env env, const char* key, napi_ref* slot) {
  if (slot == nullptr) return nullptr;
  napi_value symbol = GetRefValue(env, *slot);
  if (symbol != nullptr) return symbol;

  napi_value symbols = GetSymbolsBinding(env);
  if (symbols == nullptr) return nullptr;

  if (napi_get_named_property(env, symbols, key, &symbol) != napi_ok || symbol == nullptr) {
    return nullptr;
  }

  if (*slot != nullptr) {
    napi_delete_reference(env, *slot);
    *slot = nullptr;
  }
  napi_create_reference(env, symbol, 1, slot);
  return symbol;
}

napi_value GetOwnerSymbol(napi_env env) {
  StreamSymbolCache& cache = GetSymbolCache(env);
  return GetNamedCachedSymbol(env, "owner_symbol", &cache.owner_symbol_ref);
}

napi_value GetHandleOnCloseSymbol(napi_env env) {
  StreamSymbolCache& cache = GetSymbolCache(env);
  return GetNamedCachedSymbol(env, "handle_onclose", &cache.handle_onclose_symbol_ref);
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void FreeExternalArrayBuffer(napi_env /*env*/, void* data, void* /*hint*/) {
  free(data);
}

void SetStreamState(napi_env env, int index, int32_t value) {
  int32_t* state = EdgeGetStreamBaseState(env);
  if (state == nullptr) return;
  state[index] = value;
}

bool IsFunction(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

int CallStreamIntMethod(EdgeStreamBase* base,
                        const char* name,
                        size_t argc,
                        napi_value* argv) {
  if (base == nullptr || base->env == nullptr || name == nullptr) return UV_EBADF;
  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return UV_EBADF;

  napi_value method = nullptr;
  if (napi_get_named_property(base->env, self, name, &method) != napi_ok || !IsFunction(base->env, method)) {
    return UV_EBADF;
  }

  napi_value result = nullptr;
  if (napi_call_function(base->env, self, method, argc, argv, &result) != napi_ok || result == nullptr) {
    return UV_EPROTO;
  }

  int32_t status = UV_EPROTO;
  if (napi_get_value_int32(base->env, result, &status) != napi_ok) return UV_EPROTO;
  return status;
}

void DefineValueProperty(napi_env env,
                         napi_value object,
                         const char* name,
                         napi_value value,
                         napi_property_attributes attrs) {
  if (env == nullptr || object == nullptr || name == nullptr || value == nullptr) return;
  napi_property_descriptor desc = {
      name,
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      value,
      attrs,
      nullptr,
  };
  napi_define_properties(env, object, 1, &desc);
}

// Mirror Node's LibuvStreamWrap::DoTryWrite() contract by slicing consumed
// buffers in place and returning 0 for EAGAIN/ENOSYS.
int EdgeLibuvStreamDoTryWrite(uv_stream_t* stream, uv_buf_t** bufs, size_t* count) {
  if (stream == nullptr || bufs == nullptr || count == nullptr || *bufs == nullptr) return UV_EINVAL;

  int err = uv_try_write(stream, *bufs, *count);
  if (err == UV_ENOSYS || err == UV_EAGAIN) return 0;
  if (err < 0) return err;

  size_t written = static_cast<size_t>(err);
  uv_buf_t* vbufs = *bufs;
  size_t vcount = *count;

  for (; vcount > 0; vbufs++, vcount--) {
    if (vbufs[0].len > written) {
      vbufs[0].base += written;
      vbufs[0].len -= written;
      written = 0;
      break;
    }
    written -= vbufs[0].len;
  }

  *bufs = vbufs;
  *count = vcount;
  return 0;
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

void SetPropertyIfPresent(napi_env env, napi_value obj, napi_value key, napi_value value) {
  if (env == nullptr || obj == nullptr || key == nullptr || value == nullptr) return;
  napi_set_property(env, obj, key, value);
}

napi_value GetNamedPropertyValue(napi_env env, napi_value obj, const char* key) {
  if (env == nullptr || obj == nullptr || key == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_named_property(env, obj, key, &value) != napi_ok) return nullptr;
  return value;
}

bool UpdateUserReadBuffer(EdgeStreamBase* base, napi_value value) {
  if (base == nullptr || base->env == nullptr || value == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(base->env, value, &is_buffer) == napi_ok && is_buffer) {
    void* data = nullptr;
    size_t len = 0;
    if (napi_get_buffer_info(base->env, value, &data, &len) != napi_ok ||
        data == nullptr ||
        len == 0) {
      return false;
    }
    DeleteRefIfPresent(base->env, &base->user_read_buffer_ref);
    if (napi_create_reference(base->env, value, 1, &base->user_read_buffer_ref) != napi_ok ||
        base->user_read_buffer_ref == nullptr) {
      return false;
    }
    base->user_buffer_base = static_cast<char*>(data);
    base->user_buffer_len = len;
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(base->env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(base->env,
                                 value,
                                 &ta_type,
                                 &length,
                                 &data,
                                 &arraybuffer,
                                 &byte_offset) != napi_ok ||
        data == nullptr ||
        length == 0) {
      return false;
    }
    DeleteRefIfPresent(base->env, &base->user_read_buffer_ref);
    if (napi_create_reference(base->env, value, 1, &base->user_read_buffer_ref) != napi_ok ||
        base->user_read_buffer_ref == nullptr) {
      return false;
    }
    base->user_buffer_base = static_cast<char*>(data);
    base->user_buffer_len = length * EdgeTypedArrayElementSize(ta_type);
    return true;
  }

  return false;
}

bool CallJsOnRead(EdgeStreamBase* base,
                  ssize_t nread,
                  napi_value arraybuffer,
                  size_t offset,
                  napi_value* result) {
  if (base == nullptr || base->env == nullptr) return false;

  SetStreamState(base->env, kEdgeReadBytesOrError, static_cast<int32_t>(nread));
  SetStreamState(base->env, kEdgeArrayBufferOffset, static_cast<int32_t>(offset));

  napi_value callback = GetRefValue(base->env, base->onread_ref);
  if (!IsFunction(base->env, callback)) {
    if (TraceNetEnabled()) {
      napi_valuetype callback_type = napi_undefined;
      if (callback != nullptr) (void)napi_typeof(base->env, callback, &callback_type);
      std::fprintf(stderr,
                   "EDGE_TRACE_NET stream missing_onread provider=%d async_id=%llu nread=%zd callback=%p type=%d\n",
                   base->provider_type,
                   static_cast<unsigned long long>(base->async_id),
                   nread,
                   static_cast<void*>(callback),
                   static_cast<int>(callback_type));
    }
    if (EDGE_TRACE_ENABLED("EDGE_TRACE_TTY")) {
      std::fprintf(stderr,
                   "EDGE_TRACE_TTY missing onread async_id=%llu nread=%zd\n",
                   static_cast<unsigned long long>(base->async_id),
                   nread);
    }
    return false;
  }

  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return false;

  napi_value argv[1] = {arraybuffer != nullptr ? arraybuffer : EdgeStreamBaseUndefined(base->env)};
  napi_value ignored = nullptr;
  napi_value* out = result != nullptr ? result : &ignored;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream call_onread provider=%d async_id=%llu nread=%zd has_ab=%d offset=%zu\n",
                 base->provider_type,
                 static_cast<unsigned long long>(base->async_id),
                 nread,
                 arraybuffer != nullptr,
                 offset);
  }
  if (EdgeAsyncWrapMakeCallback(
          base->env, base->async_id, self, self, callback, 1, argv, out, kEdgeMakeCallbackNone) != napi_ok) {
    *out = nullptr;
  }
  if (TraceNetEnabled()) {
    bool pending = false;
    (void)napi_is_exception_pending(base->env, &pending);
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream call_onread_done provider=%d async_id=%llu pending_exception=%d result=%p\n",
                 base->provider_type,
                 static_cast<unsigned long long>(base->async_id),
                 pending,
                 static_cast<void*>(out != nullptr ? *out : nullptr));
  }
  (void)EdgeHandlePendingExceptionNow(base->env, nullptr);
  return true;
}

bool DefaultOnAlloc(EdgeStreamListener* listener, size_t suggested_size, uv_buf_t* out) {
  if (listener == nullptr || out == nullptr) return false;
  char* base = static_cast<char*>(malloc(suggested_size));
  if (base == nullptr && suggested_size > 0) return false;
  *out = uv_buf_init(base, static_cast<unsigned int>(suggested_size));
  if (TraceNetEnabled()) {
    auto* stream_base = static_cast<EdgeStreamBase*>(listener->data);
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream alloc provider=%d async_id=%llu suggested=%zu buf=%p\n",
                 stream_base != nullptr ? stream_base->provider_type : -1,
                 stream_base != nullptr ? static_cast<unsigned long long>(stream_base->async_id) : 0,
                 suggested_size,
                 static_cast<void*>(base));
  }
  return true;
}

bool DefaultOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf) {
  if (listener == nullptr) return false;
  auto* base = static_cast<EdgeStreamBase*>(listener->data);
  if (base == nullptr) return false;

  const char* data = (buf != nullptr) ? buf->base : nullptr;
  size_t length = (buf != nullptr) ? buf->len : 0;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream read provider=%d async_id=%llu nread=%zd buf_len=%zu buf=%p\n",
                 base->provider_type,
                 static_cast<unsigned long long>(base->async_id),
                 nread,
                 length,
                 static_cast<const void*>(data));
  }

  if (nread <= 0) {
    if (nread < 0) {
      (void)CallJsOnRead(base, nread, nullptr, 0, nullptr);
    }
    if (data != nullptr) free(const_cast<char*>(data));
    return true;
  }

  napi_value ab = nullptr;
  if (static_cast<size_t>(nread) == length) {
    if (napi_create_external_arraybuffer(base->env,
                                         const_cast<char*>(data),
                                         static_cast<size_t>(nread),
                                         FreeExternalArrayBuffer,
                                         nullptr,
                                         &ab) != napi_ok ||
        ab == nullptr) {
      ab = nullptr;
    }
  }

  if (ab == nullptr) {
    void* out = nullptr;
    if (napi_create_arraybuffer(base->env, static_cast<size_t>(nread), &out, &ab) != napi_ok ||
        out == nullptr ||
        ab == nullptr) {
      if (data != nullptr) free(const_cast<char*>(data));
      return true;
    }
    if (nread > 0 && data != nullptr) memcpy(out, data, static_cast<size_t>(nread));
    if (data != nullptr) free(const_cast<char*>(data));
  }

  (void)CallJsOnRead(base, nread, ab, 0, nullptr);
  return true;
}

bool UserBufferOnAlloc(EdgeStreamListener* listener, size_t /*suggested_size*/, uv_buf_t* out) {
  if (listener == nullptr || out == nullptr) return false;
  auto* base = static_cast<EdgeStreamBase*>(listener->data);
  if (base == nullptr || base->user_buffer_base == nullptr || base->user_buffer_len == 0) {
    return false;
  }
  *out = uv_buf_init(base->user_buffer_base, static_cast<unsigned int>(base->user_buffer_len));
  return true;
}

bool UserBufferOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf) {
  if (listener == nullptr) return false;
  auto* base = static_cast<EdgeStreamBase*>(listener->data);
  if (base == nullptr) return false;

  if (nread < 0 && (buf == nullptr || buf->base == nullptr)) {
    (void)CallJsOnRead(base, nread, nullptr, 0, nullptr);
    return true;
  }

  napi_value next_buffer = nullptr;
  (void)CallJsOnRead(base, nread, nullptr, 0, &next_buffer);
  if (next_buffer != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(base->env, next_buffer, &type) == napi_ok &&
        type != napi_undefined &&
        type != napi_null) {
      (void)UpdateUserReadBuffer(base, next_buffer);
    }
  }
  return true;
}

bool DefaultOnAfterReqFinished(EdgeStreamBase* base,
                               napi_value req_obj,
                               int status) {
  if (base == nullptr || base->env == nullptr || req_obj == nullptr) return true;
  if (base->finalized || base->destroyed || EdgeStreamBaseEnvCleanupStarted(base->env)) {
    return true;
  }
  napi_value stream_obj = EdgeStreamBaseGetWrapper(base);
  napi_value argv[3] = {
      EdgeStreamBaseMakeInt32(base->env, status),
      stream_obj != nullptr ? stream_obj : EdgeStreamBaseUndefined(base->env),
      status < 0 ? GetNamedPropertyValue(base->env, req_obj, "error") : EdgeStreamBaseUndefined(base->env),
  };
  EdgeStreamBaseInvokeReqOnComplete(base->env, req_obj, status, argv, 3);
  return true;
}

bool DefaultOnAfterWrite(EdgeStreamListener* listener,
                         napi_value req_obj,
                         int status) {
  if (listener == nullptr) return false;
  return DefaultOnAfterReqFinished(static_cast<EdgeStreamBase*>(listener->data), req_obj, status);
}

bool DefaultOnAfterShutdown(EdgeStreamListener* listener,
                            napi_value req_obj,
                            int status) {
  if (listener == nullptr) return false;
  return DefaultOnAfterReqFinished(static_cast<EdgeStreamBase*>(listener->data), req_obj, status);
}

void DeleteOnReadRefs(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return;
  DeleteRefIfPresent(base->env, &base->onread_ref);
  DeleteRefIfPresent(base->env, &base->user_read_buffer_ref);
  base->user_buffer_base = nullptr;
  base->user_buffer_len = 0;
}

void MaybeCallHandleOnClose(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr || base->finalized ||
      EdgeStreamBaseEnvCleanupStarted(base->env)) {
    return;
  }
  edge::HandleScope scope(base->env);
  if (!scope.is_open()) return;
  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return;
  napi_value symbol = GetHandleOnCloseSymbol(base->env);
  if (symbol == nullptr) return;

  bool has_callback = false;
  if (napi_has_property(base->env, self, symbol, &has_callback) != napi_ok || !has_callback) {
    return;
  }

  napi_value callback = nullptr;
  if (napi_get_property(base->env, self, symbol, &callback) != napi_ok || !IsFunction(base->env, callback)) {
    return;
  }

  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(
      base->env, base->async_id, self, self, callback, 0, nullptr, &ignored, kEdgeMakeCallbackNone);
  napi_value undefined = EdgeStreamBaseUndefined(base->env);
  SetPropertyIfPresent(base->env, self, symbol, undefined);
}

void DestroyBase(EdgeStreamBase* base) {
  if (base == nullptr ||
      base->destroyed ||
      base->ops == nullptr ||
      base->ops->destroy_self == nullptr) {
    return;
  }
  base->destroyed = true;
  base->ops->destroy_self(base);
}

void FreeWriteReq(LibuvWriteReq* wr) {
  if (wr == nullptr) return;
  if (wr->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(wr->env, wr->active_request_token);
    wr->active_request_token = nullptr;
  }
  if (wr->bufs_refs != nullptr) {
    for (uint32_t i = 0; i < wr->nbufs_storage; ++i) {
      DeleteRefIfPresent(wr->env, &wr->bufs_refs[i]);
    }
    delete[] wr->bufs_refs;
    wr->bufs_refs = nullptr;
  }
  if (wr->bufs_allocs != nullptr) {
    for (uint32_t i = 0; i < wr->nbufs_storage; ++i) {
      free(wr->bufs_allocs[i]);
    }
    delete[] wr->bufs_allocs;
    wr->bufs_allocs = nullptr;
  }
  delete[] wr->bufs_storage;
  wr->bufs_storage = nullptr;
  DeleteRefIfPresent(wr->env, &wr->req_obj_ref);
  DeleteRefIfPresent(wr->env, &wr->send_handle_ref);
  delete wr;
}

void OnWriteDone(uv_write_t* req, int status) {
  auto* wr = static_cast<LibuvWriteReq*>(req->data);
  if (wr == nullptr) return;
  if (wr->env == nullptr) {
    FreeWriteReq(wr);
    return;
  }
  edge::HandleScope scope(wr->env);
  if (!scope.is_open()) {
    FreeWriteReq(wr);
    return;
  }
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream write_done provider=%d async_id=%llu status=%d(%s)\n",
                 wr->base != nullptr ? wr->base->provider_type : -1,
                 wr->base != nullptr ? static_cast<unsigned long long>(wr->base->async_id) : 0,
                 status,
                 status == 0 ? "OK" : uv_err_name(status));
  }
  napi_value req_obj = GetRefValue(wr->env, wr->req_obj_ref);
  EdgeStreamBaseEmitAfterWrite(wr->base, req_obj, status);
  FreeWriteReq(wr);
}

void OnShutdownDone(uv_shutdown_t* req, int status) {
  auto* sr = static_cast<LibuvShutdownReq*>(req->data);
  if (sr == nullptr) return;
  if (sr->env == nullptr) {
    delete sr;
    return;
  }
  edge::HandleScope scope(sr->env);
  if (!scope.is_open()) {
    DeleteRefIfPresent(sr->env, &sr->req_obj_ref);
    delete sr;
    return;
  }
  if (sr->active_request_token != nullptr) {
    EdgeUnregisterActiveRequestToken(sr->env, sr->active_request_token);
    sr->active_request_token = nullptr;
  }
  napi_value req_obj = GetRefValue(sr->env, sr->req_obj_ref);
  EdgeStreamBaseEmitAfterShutdown(sr->base, req_obj, status);
  DeleteRefIfPresent(sr->env, &sr->req_obj_ref);
  delete sr;
}

}  // namespace

void EdgeStreamBaseInit(EdgeStreamBase* base,
                       napi_env env,
                       const EdgeStreamBaseOps* ops,
                       int32_t provider_type) {
  if (base == nullptr) return;
  base->env = env;
  base->ops = ops;
  base->provider_type = provider_type;
  base->async_id = EdgeAsyncWrapNextId(env);
  base->attached = false;
  base->prev = nullptr;
  base->next = nullptr;

  base->default_listener.env = env;
  base->default_listener.on_alloc = DefaultOnAlloc;
  base->default_listener.on_read = DefaultOnRead;
  base->default_listener.on_after_write = DefaultOnAfterWrite;
  base->default_listener.on_after_shutdown = DefaultOnAfterShutdown;
  base->default_listener.data = base;

  base->user_buffer_listener.env = env;
  base->user_buffer_listener.on_alloc = UserBufferOnAlloc;
  base->user_buffer_listener.on_read = UserBufferOnRead;
  base->user_buffer_listener.data = base;

  EdgeInitStreamListenerState(&base->listener_state, &base->default_listener);
}

void EdgeStreamBaseSetWrapperRef(EdgeStreamBase* base, napi_ref wrapper_ref) {
  if (base == nullptr) return;
  base->wrapper_ref = wrapper_ref;
  if (base->env == nullptr || wrapper_ref == nullptr) return;
  StreamBaseMaybeAttach(base);
  napi_value owner = EdgeStreamBaseGetWrapper(base);
  if (owner == nullptr) return;
  if (base->active_handle_token == nullptr && base->provider_type != kEdgeProviderJsStream) {
    base->active_handle_token = EdgeRegisterActiveHandle(base->env,
                                                        owner,
                                                        ActiveResourceNameForProvider(base->provider_type),
                                                        StreamBaseHasRefForTracking,
                                                        StreamBaseGetActiveOwner,
                                                        base,
                                                        CloseStreamBaseForCleanup);
  }
  if (!base->async_init_emitted && base->async_id > 0) {
    EdgeAsyncWrapEmitInit(
        base->env, base->async_id, base->provider_type, EdgeAsyncWrapExecutionAsyncId(base->env), owner);
    base->async_init_emitted = true;
  }
}

napi_value EdgeStreamBaseGetWrapper(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return nullptr;
  return GetRefValue(base->env, base->wrapper_ref);
}

void EdgeStreamBaseSetInitialStreamProperties(EdgeStreamBase* base,
                                             bool set_owner_symbol,
                                             bool set_onconnection) {
  if (base == nullptr || base->env == nullptr) return;
  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return;
  const auto writable_js_property = static_cast<napi_property_attributes>(
      napi_writable | napi_enumerable | napi_configurable);

  DefineValueProperty(base->env,
                      self,
                      "isStreamBase",
                      EdgeStreamBaseMakeBool(base->env, true),
                      napi_default);
  DefineValueProperty(base->env,
                      self,
                      "reading",
                      EdgeStreamBaseMakeBool(base->env, false),
                      writable_js_property);

  if (set_onconnection) {
    DefineValueProperty(base->env,
                        self,
                        "onconnection",
                        EdgeStreamBaseUndefined(base->env),
                        writable_js_property);
  }

  if (set_owner_symbol) {
    napi_value owner_symbol = GetOwnerSymbol(base->env);
    if (owner_symbol != nullptr) {
      napi_value null_value = nullptr;
      napi_get_null(base->env, &null_value);
      SetPropertyIfPresent(base->env, self, owner_symbol, null_value);
    }
  }
}

void EdgeStreamBaseFinalize(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return;
  base->finalized = true;
  DeleteOnReadRefs(base);
  DeleteRefIfPresent(base->env, &base->wrapper_ref);

  uv_handle_t* handle = (base->ops != nullptr && base->ops->get_handle != nullptr)
                            ? base->ops->get_handle(base)
                            : nullptr;
  if (base->close_callback_active) {
    base->delete_on_close = true;
    return;
  }

  if (handle == nullptr) {
    StreamBaseDetach(base);
    if (base->active_handle_token != nullptr) {
      EdgeUnregisterActiveHandle(base->env, base->active_handle_token);
      base->active_handle_token = nullptr;
    }
    DestroyBase(base);
    return;
  }

  if (!base->closed) {
    base->delete_on_close = true;
    if (!base->closing && !uv_is_closing(handle) && base->ops != nullptr && base->ops->on_close != nullptr) {
      base->closing = true;
      uv_close(handle, base->ops->on_close);
    }
    return;
  }

  StreamBaseDetach(base);
  DestroyBase(base);
}

void EdgeStreamBaseOnClosed(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return;
  base->closing = false;
  base->closed = true;
  base->close_callback_active = true;
  StreamBaseDetach(base);

  MaybeCallHandleOnClose(base);

  if (!base->destroy_notified) {
    base->destroy_notified = true;
    EdgeAsyncWrapQueueDestroyId(base->env, base->async_id);
    base->async_id = -1;
    EdgeStreamNotifyClosed(&base->listener_state);
  }

  if (base->active_handle_token != nullptr) {
    EdgeUnregisterActiveHandle(base->env, base->active_handle_token);
    base->active_handle_token = nullptr;
  }

  if (base->delete_on_close || base->finalized) {
    DeleteOnReadRefs(base);
    base->close_callback_active = false;
    DestroyBase(base);
    return;
  }

  base->close_callback_active = false;
}

bool EdgeStreamBasePushListener(EdgeStreamBase* base, EdgeStreamListener* listener) {
  if (base == nullptr || listener == nullptr) return false;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream listener_push base=%p old_current=%p listener=%p on_alloc=%p on_read=%p previous=%p\n",
                 static_cast<void*>(base),
                 static_cast<void*>(base->listener_state.current),
                 static_cast<void*>(listener),
                 reinterpret_cast<void*>(listener->on_alloc),
                 reinterpret_cast<void*>(listener->on_read),
                 static_cast<void*>(listener->previous));
  }
  EdgePushStreamListener(&base->listener_state, listener);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream listener_push_done base=%p current=%p listener_previous=%p\n",
                 static_cast<void*>(base),
                 static_cast<void*>(base->listener_state.current),
                 static_cast<void*>(listener->previous));
  }
  return true;
}

bool EdgeStreamBaseRemoveListener(EdgeStreamBase* base, EdgeStreamListener* listener) {
  if (base == nullptr || listener == nullptr) return false;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream listener_remove base=%p current=%p listener=%p previous=%p\n",
                 static_cast<void*>(base),
                 static_cast<void*>(base->listener_state.current),
                 static_cast<void*>(listener),
                 static_cast<void*>(listener->previous));
  }
  bool removed = EdgeRemoveStreamListener(&base->listener_state, listener);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream listener_remove_done base=%p removed=%d current=%p listener_previous=%p\n",
                 static_cast<void*>(base),
                 removed,
                 static_cast<void*>(base->listener_state.current),
                 static_cast<void*>(listener->previous));
  }
  return removed;
}

bool EdgeStreamBaseOnUvAlloc(EdgeStreamBase* base, size_t suggested_size, uv_buf_t* out) {
  if (base == nullptr || out == nullptr) return false;
  if (base->env == nullptr) return false;
  edge::HandleScope scope(base->env);
  if (!scope.is_open()) return false;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream uv_alloc base=%p current=%p suggested=%zu\n",
                 static_cast<void*>(base),
                 static_cast<void*>(base->listener_state.current),
                 suggested_size);
  }
  if (EdgeStreamEmitAlloc(&base->listener_state, suggested_size, out)) return true;
  char* raw = static_cast<char*>(malloc(suggested_size));
  *out = uv_buf_init(raw, static_cast<unsigned int>(suggested_size));
  return true;
}

void EdgeStreamBaseOnUvRead(EdgeStreamBase* base, ssize_t nread, const uv_buf_t* buf) {
  if (base == nullptr) {
    if (buf != nullptr && buf->base != nullptr) free(buf->base);
    return;
  }
  if (base->env == nullptr) {
    if (buf != nullptr && buf->base != nullptr) free(buf->base);
    return;
  }
  edge::HandleScope scope(base->env);
  if (!scope.is_open()) {
    if (buf != nullptr && buf->base != nullptr) free(buf->base);
    return;
  }
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream uv_read base=%p current=%p nread=%zd buf=%p len=%zu\n",
                 static_cast<void*>(base),
                 static_cast<void*>(base->listener_state.current),
                 nread,
                 buf != nullptr ? static_cast<void*>(buf->base) : nullptr,
                 buf != nullptr ? static_cast<size_t>(buf->len) : 0);
  }

  if (nread == UV_EOF) {
    base->eof_emitted = true;
  }

  if (base->ops != nullptr && base->ops->accept_pending_handle != nullptr) {
    napi_value self = EdgeStreamBaseGetWrapper(base);
    napi_value pending_handle = base->ops->accept_pending_handle(base);
    if (self != nullptr && pending_handle != nullptr && !internal_binding::IsUndefined(base->env, pending_handle)) {
      napi_set_named_property(base->env, self, "pendingHandle", pending_handle);
    }
  }

  if (nread > 0) {
    base->bytes_read += static_cast<uint64_t>(nread);
  }

  if (!EdgeStreamEmitRead(&base->listener_state, nread, buf) && buf != nullptr && buf->base != nullptr) {
    free(buf->base);
  }
}

void EdgeStreamBaseEmitAfterWrite(EdgeStreamBase* base, napi_value req_obj, int status) {
  if (base == nullptr) return;
  if (!EdgeStreamEmitAfterWrite(&base->listener_state, req_obj, status) && req_obj != nullptr) {
    EdgeStreamBaseInvokeReqOnComplete(base->env, req_obj, status, nullptr, 0);
  }
}

void EdgeStreamBaseEmitAfterShutdown(EdgeStreamBase* base, napi_value req_obj, int status) {
  EdgeStreamBaseEmitAfterShutdown(base, nullptr, req_obj, status);
}

void EdgeStreamBaseEmitAfterShutdown(EdgeStreamBase* base,
                                    napi_env callback_env,
                                    napi_value req_obj,
                                    int status) {
  if (base == nullptr) return;
  napi_env env = callback_env != nullptr ? callback_env : base->env;
  if (!EdgeStreamEmitAfterShutdown(&base->listener_state, req_obj, status, env) && req_obj != nullptr) {
    EdgeStreamBaseInvokeReqOnComplete(env != nullptr ? env : base->env, req_obj, status, nullptr, 0);
  }
}

void EdgeStreamBaseEmitWantsWrite(EdgeStreamBase* base, size_t suggested_size) {
  if (base == nullptr) return;
  (void)EdgeStreamEmitWantsWrite(&base->listener_state, suggested_size);
}

bool EdgeStreamBaseEmitReadBuffer(EdgeStreamBase* base, const uint8_t* data, size_t len) {
  if (base == nullptr || data == nullptr || len == 0) return false;
  char* copy = static_cast<char*>(malloc(len));
  if (copy == nullptr) return false;
  memcpy(copy, data, len);
  uv_buf_t buf = uv_buf_init(copy, static_cast<unsigned int>(len));
  if (EdgeStreamEmitRead(&base->listener_state, static_cast<ssize_t>(len), &buf)) {
    return true;
  }
  free(copy);
  return false;
}

bool EdgeStreamBaseEmitEOF(EdgeStreamBase* base) {
  if (base == nullptr) return false;
  return EdgeStreamEmitRead(&base->listener_state, UV_EOF, nullptr);
}

void EdgeStreamBaseSetReading(EdgeStreamBase* base, bool reading) {
  if (base == nullptr || base->env == nullptr) return;
  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return;
  napi_set_named_property(base->env, self, "reading", EdgeStreamBaseMakeBool(base->env, reading));
}

napi_value EdgeStreamBaseClose(EdgeStreamBase* base, napi_value close_callback) {
  if (base == nullptr || base->env == nullptr) return EdgeStreamBaseUndefined(base != nullptr ? base->env : nullptr);
  uv_handle_t* handle = (base->ops != nullptr && base->ops->get_handle != nullptr)
                            ? base->ops->get_handle(base)
                            : nullptr;
  if (handle == nullptr || base->closed || base->closing || uv_is_closing(handle)) {
    return EdgeStreamBaseUndefined(base->env);
  }

  EdgeStreamBaseSetCloseCallback(base, close_callback);
  if (base->ops != nullptr && base->ops->get_stream != nullptr) {
    if (uv_stream_t* stream = base->ops->get_stream(base)) {
      (void)uv_read_stop(stream);
    }
  }
  EdgeStreamBaseSetReading(base, false);

  base->closing = true;
  if (base->ops != nullptr && base->ops->on_close != nullptr) {
    uv_close(handle, base->ops->on_close);
  }
  return EdgeStreamBaseUndefined(base->env);
}

void EdgeStreamBaseSetCloseCallback(EdgeStreamBase* base, napi_value close_callback) {
  if (base == nullptr || base->env == nullptr || close_callback == nullptr ||
      !IsFunction(base->env, close_callback)) {
    return;
  }
  napi_value self = EdgeStreamBaseGetWrapper(base);
  napi_value symbol = GetHandleOnCloseSymbol(base->env);
  if (self != nullptr && symbol != nullptr) {
    SetPropertyIfPresent(base->env, self, symbol, close_callback);
  }
}

bool EdgeStreamBaseHasRef(EdgeStreamBase* base) {
  if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr) return false;
  uv_handle_t* handle = base->ops->get_handle(base);
  return handle != nullptr && !base->closed && uv_has_ref(handle) != 0;
}

void EdgeStreamBaseRef(EdgeStreamBase* base) {
  if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr) return;
  uv_handle_t* handle = base->ops->get_handle(base);
  if (handle != nullptr && !base->closed) uv_ref(handle);
}

void EdgeStreamBaseUnref(EdgeStreamBase* base) {
  if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr) return;
  uv_handle_t* handle = base->ops->get_handle(base);
  if (handle != nullptr && !base->closed) uv_unref(handle);
}

bool EdgeStreamBaseEnvCleanupStarted(napi_env env) {
  return EdgeEnvironmentCleanupStarted(env);
}

void EdgeStreamBaseRunEnvCleanup(napi_env env) {
  (void)env;
}

napi_value EdgeStreamBaseGetOnRead(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return nullptr;
  napi_value callback = GetRefValue(base->env, base->onread_ref);
  return callback != nullptr ? callback : EdgeStreamBaseUndefined(base->env);
}

napi_value EdgeStreamBaseSetOnRead(EdgeStreamBase* base, napi_value value) {
  if (base == nullptr || base->env == nullptr) return nullptr;

  napi_valuetype type = napi_undefined;
  if (value == nullptr || napi_typeof(base->env, value, &type) != napi_ok ||
      type == napi_undefined || type == napi_null) {
    DeleteRefIfPresent(base->env, &base->onread_ref);
    return EdgeStreamBaseUndefined(base->env);
  }

  if (type != napi_function) {
    return EdgeStreamBaseUndefined(base->env);
  }

  DeleteRefIfPresent(base->env, &base->onread_ref);
  napi_create_reference(base->env, value, 1, &base->onread_ref);
  return EdgeStreamBaseUndefined(base->env);
}

napi_value EdgeStreamBaseUseUserBuffer(EdgeStreamBase* base, napi_value value) {
  if (base == nullptr || base->env == nullptr || value == nullptr) {
    return EdgeStreamBaseMakeInt32(base != nullptr ? base->env : nullptr, UV_EINVAL);
  }

  if (!UpdateUserReadBuffer(base, value)) return EdgeStreamBaseMakeInt32(base->env, UV_EINVAL);
  if (!base->user_buffer_listener_active) {
    EdgePushStreamListener(&base->listener_state, &base->user_buffer_listener);
    base->user_buffer_listener_active = true;
  }
  return EdgeStreamBaseMakeInt32(base->env, 0);
}

napi_value EdgeStreamBaseGetBytesRead(EdgeStreamBase* base) {
  return EdgeStreamBaseMakeDouble(base != nullptr ? base->env : nullptr,
                                 static_cast<double>(base != nullptr ? base->bytes_read : 0));
}

napi_value EdgeStreamBaseGetBytesWritten(EdgeStreamBase* base) {
  return EdgeStreamBaseMakeDouble(base != nullptr ? base->env : nullptr,
                                 static_cast<double>(base != nullptr ? base->bytes_written : 0));
}

napi_value EdgeStreamBaseGetFd(EdgeStreamBase* base) {
  if (base == nullptr || base->ops == nullptr || base->ops->get_handle == nullptr) {
    return EdgeStreamBaseMakeInt32(base != nullptr ? base->env : nullptr, -1);
  }
  uv_handle_t* handle = base->ops->get_handle(base);
  uv_os_fd_t fd = -1;
  if (handle == nullptr || uv_fileno(handle, &fd) != 0) fd = -1;
  return EdgeStreamBaseMakeInt32(base->env, static_cast<int32_t>(fd));
}

napi_value EdgeStreamBaseGetExternal(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return nullptr;
  napi_value external = nullptr;
  napi_create_external(base->env, base, nullptr, nullptr, &external);
  return external;
}

napi_value EdgeStreamBaseGetAsyncId(EdgeStreamBase* base) {
  return EdgeStreamBaseMakeInt64(base != nullptr ? base->env : nullptr,
                                base != nullptr ? base->async_id : -1);
}

napi_value EdgeStreamBaseGetProviderType(EdgeStreamBase* base) {
  return EdgeStreamBaseMakeInt32(base != nullptr ? base->env : nullptr,
                                base != nullptr ? base->provider_type : kEdgeProviderNone);
}

napi_value EdgeStreamBaseAsyncReset(EdgeStreamBase* base) {
  if (base == nullptr || base->env == nullptr) return nullptr;
  EdgeAsyncWrapReset(base->env, &base->async_id);
  base->async_init_emitted = false;
  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self != nullptr && base->async_id > 0) {
    EdgeAsyncWrapEmitInit(
        base->env, base->async_id, base->provider_type, EdgeAsyncWrapExecutionAsyncId(base->env), self);
    base->async_init_emitted = true;
  }
  return EdgeStreamBaseUndefined(base->env);
}

napi_value EdgeStreamBaseHasRefValue(EdgeStreamBase* base) {
  return EdgeStreamBaseMakeBool(base != nullptr ? base->env : nullptr, EdgeStreamBaseHasRef(base));
}

napi_value EdgeStreamBaseGetWriteQueueSize(EdgeStreamBase* base) {
  if (base == nullptr || base->ops == nullptr || base->ops->get_stream == nullptr) {
    return EdgeStreamBaseMakeInt32(base != nullptr ? base->env : nullptr, 0);
  }
  uv_stream_t* stream = base->ops->get_stream(base);
  const uint32_t size = stream != nullptr ? stream->write_queue_size : 0;
  napi_value out = nullptr;
  napi_create_uint32(base->env, size, &out);
  return out;
}

uv_stream_t* EdgeStreamBaseGetLibuvStream(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  if (uv_stream_t* stream = EdgePipeWrapGetStream(env, value)) return stream;
  if (uv_stream_t* stream = EdgeTcpWrapGetStream(env, value)) return stream;
  if (uv_stream_t* stream = EdgeTtyWrapGetStream(env, value)) return stream;
  return nullptr;
}

EdgeStreamBase* EdgeStreamBaseFromValue(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;

  if (TraceNetEnabled()) {
    std::fprintf(stderr, "EDGE_TRACE_NET stream from_value value=%p begin\n", static_cast<void*>(value));
  }

  if (EdgeStreamBase* base = EdgePipeWrapGetStreamBase(env, value)) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr, "EDGE_TRACE_NET stream from_value pipe base=%p\n", static_cast<void*>(base));
    }
    return base;
  }
  if (EdgeStreamBase* base = EdgeTcpWrapGetStreamBase(env, value)) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr, "EDGE_TRACE_NET stream from_value tcp base=%p\n", static_cast<void*>(base));
    }
    return base;
  }
  if (EdgeStreamBase* base = EdgeTtyWrapGetStreamBase(env, value)) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr, "EDGE_TRACE_NET stream from_value tty base=%p\n", static_cast<void*>(base));
    }
    return base;
  }

  napi_valuetype type = napi_undefined;
  napi_status type_status = napi_typeof(env, value, &type);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream from_value type_status=%d type=%d\n",
                 static_cast<int>(type_status),
                 static_cast<int>(type));
  }
  if (type_status != napi_ok) return nullptr;
  if (type == napi_external) {
    void* data = nullptr;
    if (napi_get_value_external(env, value, &data) == napi_ok) {
      if (TraceNetEnabled()) {
        std::fprintf(stderr, "EDGE_TRACE_NET stream from_value direct_external data=%p\n", data);
      }
      return static_cast<EdgeStreamBase*>(data);
    }
  }

  if (type != napi_object && type != napi_function) {
    return nullptr;
  }

  napi_value external = nullptr;
  if (napi_get_named_property(env, value, "_externalStream", &external) != napi_ok || external == nullptr) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr, "EDGE_TRACE_NET stream from_value no_external_property\n");
    }
    return nullptr;
  }
  void* data = nullptr;
  if (napi_get_value_external(env, external, &data) != napi_ok) return nullptr;
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream from_value external value=%p data=%p\n",
                 static_cast<void*>(external),
                 data);
  }
  return static_cast<EdgeStreamBase*>(data);
}

napi_value EdgeStreamBaseMakeInt32(napi_env env, int32_t value) {
  if (env == nullptr) return nullptr;
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value EdgeStreamBaseMakeInt64(napi_env env, int64_t value) {
  if (env == nullptr) return nullptr;
  napi_value out = nullptr;
  napi_create_int64(env, value, &out);
  return out;
}

napi_value EdgeStreamBaseMakeDouble(napi_env env, double value) {
  if (env == nullptr) return nullptr;
  napi_value out = nullptr;
  napi_create_double(env, value, &out);
  return out;
}

napi_value EdgeStreamBaseMakeBool(napi_env env, bool value) {
  if (env == nullptr) return nullptr;
  napi_value out = nullptr;
  napi_get_boolean(env, value, &out);
  return out;
}

napi_value EdgeStreamBaseUndefined(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

void EdgeStreamBaseSetReqError(napi_env env, napi_value req_obj, int status) {
  if (env == nullptr || req_obj == nullptr || status >= 0) return;
  bool has_error = false;
  if (napi_has_named_property(env, req_obj, "error", &has_error) == napi_ok && has_error) {
    napi_value existing = nullptr;
    napi_valuetype type = napi_undefined;
    if (napi_get_named_property(env, req_obj, "error", &existing) == napi_ok &&
        existing != nullptr &&
        napi_typeof(env, existing, &type) == napi_ok &&
        type != napi_undefined) {
      return;
    }
  }
  napi_value undefined = nullptr;
  if (napi_get_undefined(env, &undefined) == napi_ok && undefined != nullptr) {
    napi_set_named_property(env, req_obj, "error", undefined);
  }
}

void EdgeStreamBaseInvokeReqOnComplete(napi_env env,
                                      napi_value req_obj,
                                      int status,
                                      napi_value* argv,
                                      size_t argc) {
  if (env == nullptr || req_obj == nullptr) return;

  EdgeStreamBaseSetReqError(env, req_obj, status);
  if (auto* environment = EdgeEnvironmentGet(env);
      environment != nullptr && !environment->can_call_into_js()) {
    EdgeStreamReqMarkDone(env, req_obj);
    return;
  }
  napi_value oncomplete = nullptr;
  if (napi_get_named_property(env, req_obj, "oncomplete", &oncomplete) != napi_ok ||
      !IsFunction(env, oncomplete)) {
    EdgeStreamReqMarkDone(env, req_obj);
    return;
  }

  napi_value local_argv[1] = {EdgeStreamBaseMakeInt32(env, status)};
  if (argv == nullptr || argc == 0) {
    argv = local_argv;
    argc = 1;
  }

  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(env,
                           EdgeStreamReqGetAsyncId(env, req_obj),
                           req_obj,
                           req_obj,
                           oncomplete,
                           argc,
                           argv,
                           &ignored,
                           kEdgeMakeCallbackNone);
  EdgeStreamReqMarkDone(env, req_obj);
}

int EdgeStreamBaseReadStart(EdgeStreamBase* base) {
  if (base == nullptr) return UV_EBADF;
  if (base->ops != nullptr && base->ops->read_start != nullptr) {
    return base->ops->read_start(base);
  }
  return CallStreamIntMethod(base, "readStart", 0, nullptr);
}

int EdgeStreamBaseReadStop(EdgeStreamBase* base) {
  if (base == nullptr) return UV_EBADF;
  if (base->ops != nullptr && base->ops->read_stop != nullptr) {
    return base->ops->read_stop(base);
  }
  return CallStreamIntMethod(base, "readStop", 0, nullptr);
}

int EdgeStreamBaseShutdownDirect(EdgeStreamBase* base, napi_value req_obj) {
  if (base == nullptr) return UV_EBADF;
  if (base->ops != nullptr && base->ops->shutdown_direct != nullptr) {
    return base->ops->shutdown_direct(base, req_obj);
  }
  napi_value argv[1] = {req_obj};
  return CallStreamIntMethod(base, "shutdown", 1, argv);
}

int EdgeStreamBaseWriteBufferDirect(EdgeStreamBase* base,
                                   napi_value req_obj,
                                   napi_value payload,
                                   bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (base == nullptr || base->env == nullptr) return UV_EBADF;
  if (base->ops != nullptr && base->ops->write_buffer_direct != nullptr) {
    return base->ops->write_buffer_direct(base, req_obj, payload, async_out);
  }

  if (base->provider_type == kEdgeProviderJsStream) {
    return EdgeJsStreamWriteBuffer(base, req_obj, payload, async_out);
  }

  napi_value status_value = EdgeLibuvStreamWriteBuffer(base, req_obj, payload, nullptr, nullptr);
  int32_t status = UV_EINVAL;
  if (status_value == nullptr || napi_get_value_int32(base->env, status_value, &status) != napi_ok) {
    return UV_EINVAL;
  }

  if (async_out != nullptr && status == 0) {
    int32_t* state = EdgeGetStreamBaseState(base->env);
    *async_out = state != nullptr && state[kEdgeLastWriteWasAsync] != 0;
  }

  return status;
}

int EdgeStreamBaseWritevDirect(EdgeStreamBase* base,
                              napi_value req_obj,
                              napi_value chunks,
                              bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (base == nullptr || base->env == nullptr || chunks == nullptr) return UV_EBADF;

  napi_value self = EdgeStreamBaseGetWrapper(base);
  if (self == nullptr) return UV_EBADF;

  napi_value writev = nullptr;
  if (napi_get_named_property(base->env, self, "writev", &writev) != napi_ok || !IsFunction(base->env, writev)) {
    return UV_EBADF;
  }

  napi_value argv[3] = {req_obj, chunks, EdgeStreamBaseMakeBool(base->env, true)};
  napi_value result = nullptr;
  if (napi_call_function(base->env, self, writev, 3, argv, &result) != napi_ok || result == nullptr) {
    return UV_EPROTO;
  }

  int32_t status = UV_EPROTO;
  if (napi_get_value_int32(base->env, result, &status) != napi_ok) {
    return UV_EPROTO;
  }

  if (async_out != nullptr && status == 0) {
    int32_t* state = EdgeGetStreamBaseState(base->env);
    *async_out = state != nullptr && state[kEdgeLastWriteWasAsync] != 0;
  }

  return status;
}

size_t EdgeTypedArrayElementSize(napi_typedarray_type type) {
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

bool EdgeStreamBaseExtractByteSpan(napi_env env,
                                  napi_value value,
                                  const uint8_t** data,
                                  size_t* len,
                                  bool* refable,
                                  std::string* temp_utf8) {
  if (data == nullptr || len == nullptr || refable == nullptr || temp_utf8 == nullptr) return false;
  *data = nullptr;
  *len = 0;
  *refable = false;
  temp_utf8->clear();

  if (env == nullptr || value == nullptr) return true;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    size_t length = 0;
    if (napi_get_buffer_info(env, value, &raw, &length) == napi_ok && raw != nullptr) {
      *data = static_cast<const uint8_t*>(raw);
      *len = length;
      *refable = true;
      return true;
    }
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type ta_type = napi_uint8_array;
    size_t length = 0;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_get_typedarray_info(env,
                                 value,
                                 &ta_type,
                                 &length,
                                 &raw,
                                 &arraybuffer,
                                 &byte_offset) == napi_ok &&
        raw != nullptr) {
      *data = static_cast<const uint8_t*>(raw);
      *len = length * EdgeTypedArrayElementSize(ta_type);
      *refable = true;
      return true;
    }
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t length = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &length) == napi_ok && raw != nullptr) {
      *data = static_cast<const uint8_t*>(raw);
      *len = length;
      *refable = true;
      return true;
    }
  }

  *temp_utf8 = ValueToUtf8(env, value);
  *data = reinterpret_cast<const uint8_t*>(temp_utf8->data());
  *len = temp_utf8->size();
  return true;
}

bool IsUint8ArrayPayload(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) != napi_ok || !is_typedarray) {
    return false;
  }

  napi_typedarray_type ta_type = napi_uint8_array;
  size_t length = 0;
  void* raw = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  return napi_get_typedarray_info(env,
                                  value,
                                  &ta_type,
                                  &length,
                                  &raw,
                                  &arraybuffer,
                                  &byte_offset) == napi_ok &&
         ta_type == napi_uint8_array;
}

napi_value EdgeStreamBufferFromWithEncoding(napi_env env,
                                          napi_value value,
                                          napi_value encoding) {
  if (env == nullptr || value == nullptr) return value;

  napi_value global = internal_binding::GetGlobal(env);
  if (global == nullptr) return value;

  napi_value buffer_ctor = nullptr;
  napi_value from_fn = nullptr;
  if (napi_get_named_property(env, global, "Buffer", &buffer_ctor) != napi_ok ||
      buffer_ctor == nullptr ||
      napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok ||
      !IsFunction(env, from_fn)) {
    return value;
  }

  napi_value argv[2] = {value, nullptr};
  size_t argc = 1;
  if (encoding != nullptr && !internal_binding::IsUndefined(env, encoding)) {
    argv[1] = encoding;
    argc = 2;
  }

  napi_value out = nullptr;
  if (napi_call_function(env, buffer_ctor, from_fn, argc, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingException(env);
    return value;
  }
  return out;
}

napi_value EdgeLibuvStreamWriteBuffer(EdgeStreamBase* base,
                                     napi_value req_obj,
                                     napi_value payload,
                                     uv_stream_t* send_handle,
                                     napi_value send_handle_obj) {
  if (base == nullptr || base->env == nullptr || base->ops == nullptr || base->ops->get_stream == nullptr) {
    return nullptr;
  }

  if (!IsUint8ArrayPayload(base->env, payload)) {
    napi_throw_type_error(base->env, "ERR_INVALID_ARG_TYPE", "Second argument must be a buffer");
    return EdgeStreamBaseMakeInt32(base->env, 0);
  }

  const uint8_t* data = nullptr;
  size_t len = 0;
  bool refable = false;
  std::string temp_utf8;
  EdgeStreamBaseExtractByteSpan(base->env, payload, &data, &len, &refable, &temp_utf8);

  uv_stream_t* stream = base->ops->get_stream(base);
  base->bytes_written += len;
  uv_buf_t write_buf{};
  uv_buf_t* write_bufs = &write_buf;
  size_t write_count = 1;

  if (send_handle == nullptr && len > 0 && data != nullptr) {
    write_buf =
        uv_buf_init(const_cast<char*>(reinterpret_cast<const char*>(data)), static_cast<unsigned int>(len));
    const int try_rc = EdgeLibuvStreamDoTryWrite(stream, &write_bufs, &write_count);
    if (TraceNetEnabled()) {
      std::fprintf(stderr,
                   "EDGE_TRACE_NET stream try_write provider=%d async_id=%llu len=%zu rc=%d(%s) remaining=%zu\n",
                   base->provider_type,
                   static_cast<unsigned long long>(base->async_id),
                   len,
                   try_rc,
                   try_rc == 0 ? "OK" : uv_err_name(try_rc),
                   write_count > 0 ? static_cast<size_t>(write_bufs[0].len) : 0);
    }
    if (try_rc != 0 || write_count == 0) {
      SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(len));
      SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
      if (try_rc != 0) EdgeStreamBaseSetReqError(base->env, req_obj, try_rc);
      return EdgeStreamBaseMakeInt32(base->env, try_rc);
    }
  }

  const char* remaining_base =
      (send_handle == nullptr && len > 0 && data != nullptr) ? write_bufs[0].base
                                                              : const_cast<char*>(reinterpret_cast<const char*>(data));
  const size_t remaining =
      (send_handle == nullptr && len > 0 && data != nullptr) ? write_bufs[0].len : len;

  EdgeStreamReqActivate(base->env, req_obj, kEdgeProviderWriteWrap, base->async_id);

  auto* wr = new LibuvWriteReq();
  wr->env = base->env;
  wr->base = base;
  napi_create_reference(base->env, req_obj, 1, &wr->req_obj_ref);
  wr->active_request_token =
      EdgeRegisterActiveRequest(base->env,
                                req_obj,
                                "WriteWrap",
                                wr,
                                CancelLibuvWriteReq,
                                LibuvWriteReqGetOwner);
  wr->nbufs = 1;
  wr->nbufs_storage = 1;
  wr->bufs_storage = new uv_buf_t[1];
  wr->bufs_refs = new napi_ref[1]();
  wr->bufs_allocs = new char*[1]();
  wr->bufs = wr->bufs_storage;

  if (refable && payload != nullptr && remaining_base != nullptr &&
      napi_create_reference(base->env, payload, 1, &wr->bufs_refs[0]) == napi_ok &&
      wr->bufs_refs[0] != nullptr) {
    wr->bufs_storage[0] = uv_buf_init(const_cast<char*>(remaining_base), static_cast<unsigned int>(remaining));
  } else {
    char* copy = static_cast<char*>(malloc(remaining));
    if (copy == nullptr && remaining > 0) {
      SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(len));
      SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
      EdgeStreamBaseSetReqError(base->env, req_obj, UV_ENOMEM);
      FreeWriteReq(wr);
      return EdgeStreamBaseMakeInt32(base->env, UV_ENOMEM);
    }
    if (remaining > 0 && copy != nullptr && remaining_base != nullptr) {
      memcpy(copy, remaining_base, remaining);
    }
    wr->bufs_allocs[0] = copy;
    wr->bufs_storage[0] = uv_buf_init(copy, static_cast<unsigned int>(remaining));
  }

  if (send_handle != nullptr && send_handle_obj != nullptr) {
    napi_create_reference(base->env, send_handle_obj, 1, &wr->send_handle_ref);
  }

  wr->req.data = wr;
  SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(len));
  SetStreamState(base->env, kEdgeLastWriteWasAsync, 1);

  int rc = 0;
  if (send_handle != nullptr) {
    rc = uv_write2(&wr->req, stream, wr->bufs, wr->nbufs, send_handle, OnWriteDone);
  } else {
    rc = uv_write(&wr->req, stream, wr->bufs, wr->nbufs, OnWriteDone);
  }
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET stream write provider=%d async_id=%llu len=%zu remaining=%zu rc=%d(%s)\n",
                 base->provider_type,
                 static_cast<unsigned long long>(base->async_id),
                 len,
                 remaining,
                 rc,
                 rc == 0 ? "OK" : uv_err_name(rc));
  }
  if (rc != 0) {
    SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
    EdgeStreamBaseSetReqError(base->env, req_obj, rc);
    EdgeStreamReqMarkDone(base->env, req_obj);
    FreeWriteReq(wr);
  }
  return EdgeStreamBaseMakeInt32(base->env, rc);
}

napi_value EdgeLibuvStreamWriteString(EdgeStreamBase* base,
                                     napi_value req_obj,
                                     napi_value payload,
                                     const char* encoding_name,
                                     uv_stream_t* send_handle,
                                     napi_value send_handle_obj) {
  if (base == nullptr || base->env == nullptr) return nullptr;

  napi_value encoded = payload;
  if (encoding_name != nullptr && payload != nullptr) {
    napi_value encoding = nullptr;
    if (napi_create_string_utf8(base->env, encoding_name, NAPI_AUTO_LENGTH, &encoding) == napi_ok &&
        encoding != nullptr) {
      encoded = EdgeStreamBufferFromWithEncoding(base->env, payload, encoding);
    }
  }

  return EdgeLibuvStreamWriteBuffer(base, req_obj, encoded, send_handle, send_handle_obj);
}

napi_value EdgeLibuvStreamWriteV(EdgeStreamBase* base,
                                napi_value req_obj,
                                napi_value chunks,
                                bool all_buffers,
                                uv_stream_t* send_handle,
                                napi_value send_handle_obj) {
  if (base == nullptr || base->env == nullptr || base->ops == nullptr || base->ops->get_stream == nullptr) {
    return nullptr;
  }

  uint32_t raw_len = 0;
  if (napi_get_array_length(base->env, chunks, &raw_len) != napi_ok) {
    return EdgeStreamBaseMakeInt32(base->env, UV_EINVAL);
  }
  const uint32_t nbufs = all_buffers ? raw_len : (raw_len / 2);
  if (nbufs == 0) {
    SetStreamState(base->env, kEdgeBytesWritten, 0);
    SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
    return EdgeStreamBaseMakeInt32(base->env, 0);
  }

  auto* wr = new LibuvWriteReq();
  wr->env = base->env;
  wr->base = base;
  napi_create_reference(base->env, req_obj, 1, &wr->req_obj_ref);
  wr->active_request_token =
      EdgeRegisterActiveRequest(base->env,
                                req_obj,
                                "WriteWrap",
                                wr,
                                CancelLibuvWriteReq,
                                LibuvWriteReqGetOwner);
  wr->nbufs = nbufs;
  wr->nbufs_storage = nbufs;
  wr->bufs_storage = new uv_buf_t[nbufs];
  wr->bufs_refs = new napi_ref[nbufs]();
  wr->bufs_allocs = new char*[nbufs]();
  wr->bufs = wr->bufs_storage;

  size_t total = 0;
  for (uint32_t i = 0; i < nbufs; ++i) {
    napi_value chunk = nullptr;
    napi_get_element(base->env, chunks, all_buffers ? i : (i * 2), &chunk);

    if (!all_buffers) {
      bool is_buffer = false;
      bool is_typedarray = false;
      napi_is_buffer(base->env, chunk, &is_buffer);
      if (!is_buffer) napi_is_typedarray(base->env, chunk, &is_typedarray);
      if (!is_buffer && !is_typedarray) {
        napi_value encoding = nullptr;
        napi_get_element(base->env, chunks, i * 2 + 1, &encoding);
        chunk = EdgeStreamBufferFromWithEncoding(base->env, chunk, encoding);
      }
    }

    const uint8_t* data = nullptr;
    size_t len = 0;
    bool refable = false;
    std::string temp_utf8;
    EdgeStreamBaseExtractByteSpan(base->env, chunk, &data, &len, &refable, &temp_utf8);

    if (refable &&
        chunk != nullptr &&
        data != nullptr &&
        napi_create_reference(base->env, chunk, 1, &wr->bufs_refs[i]) == napi_ok &&
        wr->bufs_refs[i] != nullptr) {
      wr->bufs_storage[i] = uv_buf_init(const_cast<char*>(reinterpret_cast<const char*>(data)),
                                        static_cast<unsigned int>(len));
    } else {
      char* copy = static_cast<char*>(malloc(len));
      if (copy == nullptr && len > 0) {
        SetStreamState(base->env, kEdgeBytesWritten, 0);
        SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
        EdgeStreamBaseSetReqError(base->env, req_obj, UV_ENOMEM);
        FreeWriteReq(wr);
        return EdgeStreamBaseMakeInt32(base->env, UV_ENOMEM);
      }
      if (len > 0 && copy != nullptr && data != nullptr) memcpy(copy, data, len);
      wr->bufs_allocs[i] = copy;
      wr->bufs_storage[i] = uv_buf_init(copy, static_cast<unsigned int>(len));
    }
    total += len;
  }

  base->bytes_written += total;
  uv_stream_t* stream = base->ops->get_stream(base);
  if (send_handle == nullptr && total > 0) {
    uv_buf_t* try_bufs = wr->bufs;
    size_t try_count = wr->nbufs;
    const int try_rc = EdgeLibuvStreamDoTryWrite(stream, &try_bufs, &try_count);
    if (try_rc != 0 || try_count == 0) {
      SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(total));
      SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
      if (try_rc != 0) EdgeStreamBaseSetReqError(base->env, req_obj, try_rc);
      FreeWriteReq(wr);
      return EdgeStreamBaseMakeInt32(base->env, try_rc);
    }
    wr->bufs = try_bufs;
    wr->nbufs = static_cast<uint32_t>(try_count);
  }

  if (wr->nbufs == 0) {
    SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(total));
    SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
    FreeWriteReq(wr);
    return EdgeStreamBaseMakeInt32(base->env, 0);
  }

  EdgeStreamReqActivate(base->env, req_obj, kEdgeProviderWriteWrap, base->async_id);

  if (send_handle != nullptr && send_handle_obj != nullptr) {
    napi_create_reference(base->env, send_handle_obj, 1, &wr->send_handle_ref);
  }

  wr->req.data = wr;
  SetStreamState(base->env, kEdgeBytesWritten, static_cast<int32_t>(total));
  SetStreamState(base->env, kEdgeLastWriteWasAsync, 1);

  int rc = 0;
  if (send_handle != nullptr) {
    rc = uv_write2(&wr->req, stream, wr->bufs, wr->nbufs, send_handle, OnWriteDone);
  } else {
    rc = uv_write(&wr->req, stream, wr->bufs, wr->nbufs, OnWriteDone);
  }
  if (rc != 0) {
    SetStreamState(base->env, kEdgeLastWriteWasAsync, 0);
    EdgeStreamBaseSetReqError(base->env, req_obj, rc);
    EdgeStreamReqMarkDone(base->env, req_obj);
    FreeWriteReq(wr);
  }
  return EdgeStreamBaseMakeInt32(base->env, rc);
}

napi_value EdgeLibuvStreamShutdown(EdgeStreamBase* base, napi_value req_obj) {
  if (base == nullptr || base->env == nullptr || base->ops == nullptr || base->ops->get_stream == nullptr) {
    return nullptr;
  }

  auto* sr = new LibuvShutdownReq();
  sr->env = base->env;
  sr->base = base;
  EdgeStreamReqActivate(base->env, req_obj, kEdgeProviderShutdownWrap, base->async_id);
  napi_create_reference(base->env, req_obj, 1, &sr->req_obj_ref);
  sr->active_request_token =
      EdgeRegisterActiveRequest(base->env,
                                req_obj,
                                "ShutdownWrap",
                                sr,
                                CancelLibuvShutdownReq,
                                LibuvShutdownReqGetOwner);
  sr->req.data = sr;

  int rc = uv_shutdown(&sr->req, base->ops->get_stream(base), OnShutdownDone);
  if (rc != 0) {
    EdgeStreamBaseSetReqError(base->env, req_obj, rc);
    EdgeStreamReqMarkDone(base->env, req_obj);
    // uv_shutdown failed synchronously, so OnShutdownDone will never run. Undo
    // the active-request registration before freeing sr; otherwise the freed
    // request stays in the Environment's active list and CancelActiveRequests()
    // calls uv_cancel() on it during cleanup (use-after-free).
    if (sr->active_request_token != nullptr) {
      EdgeUnregisterActiveRequestToken(base->env, sr->active_request_token);
      sr->active_request_token = nullptr;
    }
    DeleteRefIfPresent(base->env, &sr->req_obj_ref);
    delete sr;
  }
  return EdgeStreamBaseMakeInt32(base->env, rc);
}
