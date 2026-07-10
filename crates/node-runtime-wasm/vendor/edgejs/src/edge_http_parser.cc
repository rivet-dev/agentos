#include "edge_http_parser.h"

#include <algorithm>
#include <cstdio>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <set>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

#include <uv.h>

#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_handle_scope.h"
#include "edge_runtime.h"
#include "edge_pipe_wrap.h"
#include "edge_stream_base.h"
#include "edge_tcp_wrap.h"
#include "edge_trace.h"
#include "edge_stream_listener.h"

extern "C" {
#include "llhttp.h"
}

namespace {

constexpr uint32_t kOnMessageBegin = 0;
constexpr uint32_t kOnHeaders = 1;
constexpr uint32_t kOnHeadersComplete = 2;
constexpr uint32_t kOnBody = 3;
constexpr uint32_t kOnMessageComplete = 4;
constexpr uint32_t kOnExecute = 5;
constexpr uint32_t kOnTimeout = 6;
constexpr size_t kMaxHeaderFieldsCount = 32;
constexpr size_t kAllocBufferSize = 64 * 1024;

constexpr uint32_t kLenientNone = 0;
constexpr uint32_t kLenientHeaders = 1 << 0;
constexpr uint32_t kLenientChunkedLength = 1 << 1;
constexpr uint32_t kLenientKeepAlive = 1 << 2;
constexpr uint32_t kLenientTransferEncoding = 1 << 3;
constexpr uint32_t kLenientVersion = 1 << 4;
constexpr uint32_t kLenientDataAfterClose = 1 << 5;
constexpr uint32_t kLenientOptionalLFAfterCR = 1 << 6;
constexpr uint32_t kLenientOptionalCRLFAfterChunk = 1 << 7;
constexpr uint32_t kLenientOptionalCRBeforeLF = 1 << 8;
constexpr uint32_t kLenientSpacesAfterChunkSize = 1 << 9;
constexpr uint32_t kLenientAll = kLenientHeaders | kLenientChunkedLength | kLenientKeepAlive |
    kLenientTransferEncoding | kLenientVersion | kLenientDataAfterClose |
    kLenientOptionalLFAfterCR | kLenientOptionalCRLFAfterChunk |
    kLenientOptionalCRBeforeLF | kLenientSpacesAfterChunkSize;

struct Parser;

bool TraceNetEnabled() {
  return EDGE_TRACE_ENABLED("EDGE_TRACE_NET");
}

struct ConnectionsList {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  bool finalized = false;
  std::set<Parser*> all;
  std::set<Parser*> active;
};

struct Parser {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  EdgeStreamBase* consumed_stream = nullptr;
  EdgeStreamListener consumed_listener{};
  llhttp_t parser{};
  llhttp_settings_t settings{};
  std::vector<std::string> fields;
  std::vector<std::string> values;
  std::string url;
  std::string status_message;
  bool last_was_value = false;
  bool have_flushed = false;
  bool got_exception = false;
  bool pending_pause = false;
  bool headers_completed = false;
  bool trim_header_value_prefix = false;
  bool propagate_callback_exceptions = false;
  uint64_t header_nread = 0;
  uint64_t chunk_extensions_nread = 0;
  uint64_t max_http_header_size = 8 * 1024;
  uint64_t last_message_start = 0;
  const char* current_buffer_data = nullptr;
  size_t current_buffer_len = 0;
  ConnectionsList* list = nullptr;
  int64_t async_id = 0;
  int32_t provider_type = kEdgeProviderNone;
};

struct ParserReadBufferState {
  explicit ParserReadBufferState(napi_env) {}
  ~ParserReadBufferState() {
    free(data);
    data = nullptr;
    in_use = false;
  }

  char* data = nullptr;
  bool in_use = false;
};

ParserReadBufferState& GetParserReadBufferState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<ParserReadBufferState>(
      env, kEdgeEnvironmentSlotParserReadBufferState);
}

template <typename T>
T* Unwrap(napi_env env, napi_callback_info info, napi_value* this_arg = nullptr) {
  size_t argc = 0;
  napi_value self = nullptr;
  void* data = nullptr;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, &data) != napi_ok || self == nullptr) return nullptr;
  T* out = nullptr;
  if (napi_unwrap(env, self, reinterpret_cast<void**>(&out)) != napi_ok) return nullptr;
  if (this_arg != nullptr) *this_arg = self;
  return out;
}

napi_value CreateUint8ArrayCopy(napi_env env, const char* data, size_t length);
napi_value CreateBufferCopy(napi_env env, const char* data, size_t length);
napi_value GetWrappedObject(napi_env env, napi_ref ref);
int FlushHeadersToJs(Parser* p);
void ClearConsumedStreamBinding(Parser* p);
void ParserDetachFromConnectionsList(Parser* p);
void ParserQueueDestroy(Parser* p);
bool DispatchConsumedParserRead(Parser* p,
                                napi_value parser_obj,
                                const char* data,
                                size_t len);
bool ParserShouldIgnoreConsumedRead(Parser* p, napi_value parser_obj);

bool AcquireParserReadBuffer(napi_env env, size_t suggested_size, uv_buf_t* out) {
  if (env == nullptr || out == nullptr) return false;
  ParserReadBufferState& state = GetParserReadBufferState(env);
  if (state.data == nullptr) {
    state.data = static_cast<char*>(malloc(kAllocBufferSize));
    state.in_use = false;
  }
  if (state.data != nullptr && !state.in_use) {
    state.in_use = true;
    *out = uv_buf_init(state.data, static_cast<unsigned int>(kAllocBufferSize));
    return true;
  }
  char* fallback = static_cast<char*>(malloc(suggested_size));
  if (fallback == nullptr && suggested_size > 0) return false;
  *out = uv_buf_init(fallback, static_cast<unsigned int>(suggested_size));
  return true;
}

void ReleaseParserReadBuffer(napi_env env, const char* base) {
  if (env == nullptr || base == nullptr) return;
  auto* state =
      EdgeEnvironmentGetSlotData<ParserReadBufferState>(env, kEdgeEnvironmentSlotParserReadBufferState);
  if (state != nullptr && state->in_use && state->data == base) {
    state->in_use = false;
    return;
  }
  free(const_cast<char*>(base));
}

bool ParserConsumedListenerOnAlloc(EdgeStreamListener* listener,
                                   size_t suggested_size,
                                   uv_buf_t* out) {
  if (listener == nullptr || out == nullptr) return false;
  auto* p = static_cast<Parser*>(listener->data);
  if (p == nullptr) return false;
  bool ok = AcquireParserReadBuffer(p->env, suggested_size, out);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET http_parser consumed_alloc parser=%p suggested=%zu ok=%d buf=%p len=%zu\n",
                 static_cast<void*>(p),
                 suggested_size,
                 ok,
                 ok ? static_cast<void*>(out->base) : nullptr,
                 ok ? static_cast<size_t>(out->len) : 0);
  }
  return ok;
}

bool ParserConsumedListenerOnRead(EdgeStreamListener* listener,
                                  ssize_t nread,
                                  const uv_buf_t* buf) {
  if (listener == nullptr) return false;
  auto* p = static_cast<Parser*>(listener->data);
  if (p == nullptr) return false;

  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET http_parser consumed_read parser=%p nread=%zd buf=%p len=%zu consumed_stream=%p\n",
                 static_cast<void*>(p),
                 nread,
                 buf != nullptr ? static_cast<void*>(buf->base) : nullptr,
                 buf != nullptr ? static_cast<size_t>(buf->len) : 0,
                 static_cast<void*>(p->consumed_stream));
  }

  auto release = [&]() {
    if (buf != nullptr && buf->base != nullptr) {
      ReleaseParserReadBuffer(p->env, buf->base);
    }
  };

  if (nread < 0) {
    release();
    return false;
  }
  if (nread == 0 || buf == nullptr || buf->base == nullptr) {
    release();
    return true;
  }

  napi_value parser_obj = GetWrappedObject(p->env, p->wrapper_ref);
  if (parser_obj != nullptr) {
    if (ParserShouldIgnoreConsumedRead(p, parser_obj)) {
      if (TraceNetEnabled()) {
        std::fprintf(stderr,
                     "EDGE_TRACE_NET http_parser consumed_ignore parser=%p\n",
                     static_cast<void*>(p));
      }
      release();
      ClearConsumedStreamBinding(p);
      return true;
    }
    bool dispatched = DispatchConsumedParserRead(
        p,
        parser_obj,
        buf->base,
        static_cast<size_t>(nread));
    if (TraceNetEnabled()) {
      bool pending = false;
      (void)napi_is_exception_pending(p->env, &pending);
      std::fprintf(stderr,
                   "EDGE_TRACE_NET http_parser consumed_dispatched parser=%p dispatched=%d pending_exception=%d\n",
                   static_cast<void*>(p),
                   dispatched,
                   pending);
    }
    (void)EdgeHandlePendingExceptionNow(p->env, nullptr);
  }
  release();
  return true;
}

void ParserConsumedListenerOnClose(EdgeStreamListener* listener) {
  if (listener == nullptr) return;
  auto* p = static_cast<Parser*>(listener->data);
  if (p == nullptr) return;
  p->consumed_stream = nullptr;
  listener->previous = nullptr;
}

void ClearConsumedStreamBinding(Parser* p) {
  if (p == nullptr || p->consumed_stream == nullptr) return;
  (void)EdgeStreamBaseRemoveListener(p->consumed_stream, &p->consumed_listener);
  p->consumed_stream = nullptr;
  p->consumed_listener.previous = nullptr;
}

void ParserDetachFromConnectionsList(Parser* p) {
  if (p == nullptr || p->list == nullptr || p->list->finalized) return;
  p->list->all.erase(p);
  p->list->active.erase(p);
  p->list = nullptr;
}

void ParserQueueDestroy(Parser* p) {
  if (p == nullptr || p->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(p->env, p->async_id);
  p->async_id = 0;
  p->provider_type = kEdgeProviderNone;
}

napi_value MakeError(napi_env env,
                     const char* message,
                     const char* code,
                     const char* reason,
                     uint32_t bytes_parsed,
                     const char* raw_packet,
                     size_t raw_packet_len) {
  (void)raw_packet;
  (void)raw_packet_len;
  napi_value msg = nullptr;
  napi_value code_v = nullptr;
  napi_value err = nullptr;
  napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &msg);
  napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_v);
  napi_create_error(env, code_v, msg, &err);
  if (err != nullptr) {
    napi_value reason_v = nullptr;
    napi_create_string_utf8(env, reason ? reason : "", NAPI_AUTO_LENGTH, &reason_v);
    if (reason_v != nullptr) napi_set_named_property(env, err, "reason", reason_v);
    napi_set_named_property(env, err, "code", code_v);
    napi_value bytes_v = nullptr;
    napi_create_uint32(env, bytes_parsed, &bytes_v);
    if (bytes_v != nullptr) napi_set_named_property(env, err, "bytesParsed", bytes_v);
  }
  return err;
}

napi_value GetWrappedObject(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value obj = nullptr;
  if (napi_get_reference_value(env, ref, &obj) != napi_ok) return nullptr;
  return obj;
}

napi_value CreateUint8ArrayCopy(napi_env env, const char* data, size_t length) {
  napi_value ab = nullptr;
  void* out = nullptr;
  if (napi_create_arraybuffer(env, length, &out, &ab) != napi_ok || ab == nullptr) return nullptr;
  if (out != nullptr && data != nullptr && length > 0) {
    std::memcpy(out, data, length);
  }
  napi_value view = nullptr;
  if (napi_create_typedarray(env, napi_uint8_array, length, ab, 0, &view) != napi_ok) return nullptr;
  return view;
}

napi_value CreateBufferCopy(napi_env env, const char* data, size_t length) {
  napi_value view = CreateUint8ArrayCopy(env, data, length);
  if (view == nullptr) return nullptr;
  napi_value global = nullptr;
  napi_value buffer_ctor = nullptr;
  napi_value from_fn = nullptr;
  napi_value out = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return view;
  if (napi_get_named_property(env, global, "Buffer", &buffer_ctor) != napi_ok || buffer_ctor == nullptr) return view;
  if (napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok || from_fn == nullptr) return view;
  napi_value argv[1] = {view};
  if (napi_call_function(env, buffer_ctor, from_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value ignored = nullptr;
      napi_get_and_clear_last_exception(env, &ignored);
    }
    return view;
  }
  return out;
}

bool ParserShouldIgnoreConsumedRead(Parser* p, napi_value parser_obj) {
  if (p == nullptr || parser_obj == nullptr) return false;

  napi_value socket = nullptr;
  napi_valuetype socket_type = napi_undefined;
  if (napi_get_named_property(p->env, parser_obj, "socket", &socket) != napi_ok ||
      socket == nullptr ||
      napi_typeof(p->env, socket, &socket_type) != napi_ok ||
      socket_type != napi_object) {
    return false;
  }

  napi_value destroyed_value = nullptr;
  bool destroyed = false;
  if (napi_get_named_property(p->env, socket, "destroyed", &destroyed_value) == napi_ok &&
      destroyed_value != nullptr &&
      napi_get_value_bool(p->env, destroyed_value, &destroyed) == napi_ok &&
      destroyed) {
    return true;
  }

  napi_value socket_parser = nullptr;
  napi_valuetype socket_parser_type = napi_undefined;
  if (napi_get_named_property(p->env, socket, "parser", &socket_parser) == napi_ok &&
      socket_parser != nullptr &&
      napi_typeof(p->env, socket_parser, &socket_parser_type) == napi_ok &&
      (socket_parser_type == napi_object || socket_parser_type == napi_function)) {
    bool same_parser = false;
    if (napi_strict_equals(p->env, socket_parser, parser_obj, &same_parser) == napi_ok &&
        !same_parser) {
      return true;
    }
  }

  napi_value handle = nullptr;
  napi_valuetype handle_type = napi_undefined;
  if (napi_get_named_property(p->env, socket, "_handle", &handle) == napi_ok &&
      napi_typeof(p->env, handle, &handle_type) == napi_ok &&
      (handle_type == napi_null || handle_type == napi_undefined)) {
    return true;
  }

  return false;
}

int CallIndexedNoArgs(Parser* p, uint32_t index, bool skip_task_queues = false) {
  napi_value self = GetWrappedObject(p->env, p->wrapper_ref);
  if (self == nullptr) return 0;
  napi_value cb = nullptr;
  if (napi_get_element(p->env, self, index, &cb) != napi_ok || cb == nullptr) return 0;
  napi_valuetype t = napi_undefined;
  napi_typeof(p->env, cb, &t);
  if (t != napi_function) return 0;
  napi_value result = nullptr;
  bool has_pending = false;
  napi_status status = napi_ok;
  if (p->propagate_callback_exceptions) {
    status = napi_call_function(p->env, self, cb, 0, nullptr, &result);
  } else {
    const int callback_flags =
        skip_task_queues ? kEdgeMakeCallbackSkipTaskQueues : kEdgeMakeCallbackNone;
    status = EdgeMakeCallbackWithFlags(p->env, self, cb, 0, nullptr, &result, callback_flags);
  }
  if (status != napi_ok ||
      (napi_is_exception_pending(p->env, &has_pending) == napi_ok && has_pending)) {
    p->got_exception = true;
    llhttp_set_error_reason(&p->parser, "HPE_JS_EXCEPTION:JS Exception");
    return HPE_USER;
  }
  return 0;
}

int ParserOnMessageBegin(llhttp_t* llp) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (p->list != nullptr && !p->list->finalized) {
    p->list->all.erase(p);
    p->list->active.erase(p);
  }
  p->fields.clear();
  p->values.clear();
  p->url.clear();
  p->status_message.clear();
  p->last_was_value = false;
  p->headers_completed = false;
  p->trim_header_value_prefix = false;
  p->chunk_extensions_nread = 0;
  p->have_flushed = false;
  p->last_message_start = uv_hrtime();
  if (p->list != nullptr && !p->list->finalized) {
    p->list->all.insert(p);
    p->list->active.insert(p);
  }
  return CallIndexedNoArgs(p, kOnMessageBegin, true);
}

int TrackHeader(Parser* p, size_t len) {
  p->header_nread += len;
  if (p->header_nread >= p->max_http_header_size) {
    llhttp_set_error_reason(&p->parser, "HPE_HEADER_OVERFLOW:Header overflow");
    return HPE_USER;
  }
  return 0;
}

int ParserOnUrl(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (int rv = TrackHeader(p, length); rv != 0) return rv;
  p->url.append(at, length);
  return 0;
}

int ParserOnStatus(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (int rv = TrackHeader(p, length); rv != 0) return rv;
  p->status_message.append(at, length);
  return 0;
}

int ParserOnHeaderField(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (int rv = TrackHeader(p, length); rv != 0) return rv;
  if (p->fields.empty() || p->last_was_value) {
    if (p->fields.size() >= kMaxHeaderFieldsCount) {
      const int flush_rv = FlushHeadersToJs(p);
      if (flush_rv != 0) return flush_rv;
    }
    p->fields.emplace_back();
    p->last_was_value = false;
  }
  p->fields.back().append(at, length);
  p->trim_header_value_prefix = false;
  return 0;
}

int ParserOnHeaderValue(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (p->values.size() < p->fields.size()) {
    p->values.emplace_back();
    p->trim_header_value_prefix = true;
  }
  p->last_was_value = true;
  if (p->trim_header_value_prefix) {
    while (length > 0 && (*at == ' ' || *at == '\t')) {
      at++;
      length--;
    }
    if (length == 0) return 0;
    p->trim_header_value_prefix = false;
  }
  if (int rv = TrackHeader(p, length); rv != 0) return rv;
  p->values.back().append(at, length);
  return 0;
}

napi_value BuildHeadersArray(Parser* p) {
  napi_value arr = nullptr;
  napi_create_array_with_length(p->env, p->values.size() * 2, &arr);
  if (arr == nullptr) return nullptr;
  for (size_t i = 0; i < p->values.size(); i++) {
    napi_value k = nullptr;
    napi_value v = nullptr;
    const std::string& key = i < p->fields.size() ? p->fields[i] : std::string();
    const std::string& raw_value = p->values[i];
    size_t value_start = 0;
    size_t value_end = raw_value.size();
    while (value_start < value_end &&
           (raw_value[value_start] == ' ' || raw_value[value_start] == '\t')) {
      value_start++;
    }
    while (value_end > value_start &&
           (raw_value[value_end - 1] == ' ' || raw_value[value_end - 1] == '\t')) {
      value_end--;
    }
    napi_create_string_utf8(p->env, key.c_str(), key.size(), &k);
    // Node decodes header values as Latin-1 bytes, not UTF-8.
    napi_create_string_latin1(p->env,
                              raw_value.c_str() + value_start,
                              value_end - value_start,
                              &v);
    if (k != nullptr) napi_set_element(p->env, arr, static_cast<uint32_t>(i * 2), k);
    if (v != nullptr) napi_set_element(p->env, arr, static_cast<uint32_t>(i * 2 + 1), v);
  }
  return arr;
}

int FlushHeadersToJs(Parser* p) {
  napi_value self = GetWrappedObject(p->env, p->wrapper_ref);
  if (self == nullptr) return 0;
  napi_value cb = nullptr;
  if (napi_get_element(p->env, self, kOnHeaders, &cb) != napi_ok || cb == nullptr) return 0;
  napi_valuetype t = napi_undefined;
  napi_typeof(p->env, cb, &t);
  if (t != napi_function) return 0;

  napi_value headers = BuildHeadersArray(p);
  napi_value url_v = nullptr;
  napi_create_string_utf8(p->env, p->url.c_str(), p->url.size(), &url_v);
  napi_value argv[2] = {headers, url_v};
  napi_value ignored = nullptr;
  bool has_pending = false;
  napi_status status = p->propagate_callback_exceptions
                           ? napi_call_function(p->env, self, cb, 2, argv, &ignored)
                           : EdgeMakeCallback(p->env, self, cb, 2, argv, &ignored);
  if (status != napi_ok ||
      (napi_is_exception_pending(p->env, &has_pending) == napi_ok && has_pending)) {
    p->got_exception = true;
    llhttp_set_error_reason(&p->parser, "HPE_JS_EXCEPTION:JS Exception");
    return HPE_USER;
  }

  p->have_flushed = true;
  p->fields.clear();
  p->values.clear();
  p->url.clear();
  return 0;
}

int ParserOnHeadersComplete(llhttp_t* llp) {
  Parser* p = static_cast<Parser*>(llp->data);
  p->headers_completed = true;
  p->header_nread = 0;

  napi_value self = GetWrappedObject(p->env, p->wrapper_ref);
  if (self == nullptr) return 0;
  napi_value cb = nullptr;
  if (napi_get_element(p->env, self, kOnHeadersComplete, &cb) != napi_ok || cb == nullptr) return 0;
  napi_valuetype t = napi_undefined;
  napi_typeof(p->env, cb, &t);
  if (t != napi_function) return 0;

  napi_value argv[9] = {nullptr};
  napi_get_undefined(p->env, &argv[0]);
  for (int i = 1; i < 9; i++) argv[i] = argv[0];

  napi_create_uint32(p->env, llhttp_get_http_major(llp), &argv[0]);
  napi_create_uint32(p->env, llhttp_get_http_minor(llp), &argv[1]);
  if (p->have_flushed) {
    if (FlushHeadersToJs(p) != 0) return HPE_USER;
  } else {
    argv[2] = BuildHeadersArray(p);
    if (llhttp_get_type(llp) == HTTP_REQUEST) {
      napi_create_string_utf8(p->env, p->url.c_str(), p->url.size(), &argv[4]);
    }
  }
  if (llhttp_get_type(llp) == HTTP_REQUEST) {
    napi_create_uint32(p->env, llhttp_get_method(llp), &argv[3]);
  } else {
    napi_create_int32(p->env, llhttp_get_status_code(llp), &argv[5]);
    napi_create_string_utf8(p->env, p->status_message.c_str(), p->status_message.size(), &argv[6]);
  }
  p->fields.clear();
  p->values.clear();
  p->url.clear();
  p->status_message.clear();
  napi_get_boolean(p->env, llhttp_get_upgrade(llp) != 0, &argv[7]);
  napi_get_boolean(p->env, llhttp_should_keep_alive(llp) != 0, &argv[8]);

  napi_value result = nullptr;
  bool has_pending = false;
  napi_status status = napi_ok;
  if (p->propagate_callback_exceptions) {
    status = napi_call_function(p->env, self, cb, 9, argv, &result);
  } else {
    status = EdgeMakeCallbackWithFlags(p->env,
                                       self,
                                       cb,
                                       9,
                                       argv,
                                       &result,
                                       kEdgeMakeCallbackSkipTaskQueues);
  }
  if (status != napi_ok ||
      (napi_is_exception_pending(p->env, &has_pending) == napi_ok && has_pending)) {
    p->got_exception = true;
    llhttp_set_error_reason(&p->parser, "HPE_JS_EXCEPTION:JS Exception");
    return HPE_USER;
  }
  int32_t rv = 0;
  if (result != nullptr) {
    napi_valuetype rt = napi_undefined;
    napi_typeof(p->env, result, &rt);
    if (rt == napi_number) napi_get_value_int32(p->env, result, &rv);
  }
  return rv;
}

int ParserOnBody(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (length == 0) return 0;
  napi_value self = GetWrappedObject(p->env, p->wrapper_ref);
  if (self == nullptr) return 0;

  napi_value cb = nullptr;
  if (napi_get_element(p->env, self, kOnBody, &cb) != napi_ok || cb == nullptr) return 0;
  napi_valuetype t = napi_undefined;
  napi_typeof(p->env, cb, &t);
  if (t != napi_function) return 0;
  napi_value buf = CreateBufferCopy(p->env, at, length);
  napi_value argv[1] = {buf};
  napi_value ignored = nullptr;
  bool has_pending = false;
  napi_status status = p->propagate_callback_exceptions
                           ? napi_call_function(p->env, self, cb, 1, argv, &ignored)
                           : EdgeMakeCallback(p->env, self, cb, 1, argv, &ignored);
  if (status != napi_ok ||
      (napi_is_exception_pending(p->env, &has_pending) == napi_ok && has_pending)) {
    p->got_exception = true;
    llhttp_set_error_reason(&p->parser, "HPE_JS_EXCEPTION:JS Exception");
    return HPE_USER;
  }
  return 0;
}

int ParserOnMessageComplete(llhttp_t* llp) {
  Parser* p = static_cast<Parser*>(llp->data);
  if (p->list != nullptr && !p->list->finalized) {
    p->list->all.erase(p);
    p->list->active.erase(p);
    p->last_message_start = 0;
    p->list->all.insert(p);
  }
  if (!p->fields.empty()) {
    // Trailing headers are delivered through kOnHeaders and consumed in
    // _http_common parserOnMessageComplete via parser._headers.
    if (FlushHeadersToJs(p) != 0) return HPE_USER;
  }
  return CallIndexedNoArgs(p, kOnMessageComplete, true);
}

int ParserOnChunkExtension(llhttp_t* llp, const char* at, size_t length) {
  Parser* p = static_cast<Parser*>(llp->data);
  p->chunk_extensions_nread += length;
  if (p->chunk_extensions_nread > 16384) {
    llhttp_set_error_reason(&p->parser, "HPE_CHUNK_EXTENSIONS_OVERFLOW:Chunk extensions overflow");
    return HPE_USER;
  }
  return 0;
}

int ParserOnChunkHeader(llhttp_t* llp) {
  Parser* p = static_cast<Parser*>(llp->data);
  p->header_nread = 0;
  p->chunk_extensions_nread = 0;
  return 0;
}

int ParserOnChunkComplete(llhttp_t* llp) {
  Parser* p = static_cast<Parser*>(llp->data);
  p->header_nread = 0;
  return 0;
}

void ParserFinalize(napi_env env, void* data, void* hint) {
  Parser* p = static_cast<Parser*>(data);
  if (p == nullptr) return;
  ClearConsumedStreamBinding(p);
  ParserDetachFromConnectionsList(p);
  ParserQueueDestroy(p);
  if (p->wrapper_ref != nullptr) napi_delete_reference(env, p->wrapper_ref);
  delete p;
}

void ConnectionsListFinalize(napi_env env, void* data, void* hint) {
  ConnectionsList* list = static_cast<ConnectionsList*>(data);
  list->finalized = true;
  list->all.clear();
  list->active.clear();
  if (list->wrapper_ref != nullptr) {
    napi_delete_reference(env, list->wrapper_ref);
    list->wrapper_ref = nullptr;
  }
  // Keep backing storage alive until process exit. During env teardown, destroy
  // hooks may still call JS methods on this object after finalizers run.
}

napi_value ParserCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* p = new Parser();
  p->env = env;
  p->consumed_listener.env = env;
  p->consumed_listener.on_alloc = ParserConsumedListenerOnAlloc;
  p->consumed_listener.on_read = ParserConsumedListenerOnRead;
  p->consumed_listener.on_close = ParserConsumedListenerOnClose;
  p->consumed_listener.data = p;
  llhttp_settings_init(&p->settings);
  p->settings.on_message_begin = ParserOnMessageBegin;
  p->settings.on_url = ParserOnUrl;
  p->settings.on_status = ParserOnStatus;
  p->settings.on_header_field = ParserOnHeaderField;
  p->settings.on_header_value = ParserOnHeaderValue;
  p->settings.on_chunk_extension_name = ParserOnChunkExtension;
  p->settings.on_chunk_extension_value = ParserOnChunkExtension;
  p->settings.on_headers_complete = ParserOnHeadersComplete;
  p->settings.on_body = ParserOnBody;
  p->settings.on_message_complete = ParserOnMessageComplete;
  p->settings.on_chunk_header = ParserOnChunkHeader;
  p->settings.on_chunk_complete = ParserOnChunkComplete;
  llhttp_init(&p->parser, HTTP_REQUEST, &p->settings);
  p->parser.data = p;
  napi_wrap(env, self, p, ParserFinalize, nullptr, &p->wrapper_ref);
  return self;
}

napi_value ConnectionsListCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* list = new ConnectionsList();
  list->env = env;
  napi_wrap(env, self, list, ConnectionsListFinalize, nullptr, &list->wrapper_ref);
  return self;
}

napi_value ParserInitialize(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr};
  napi_value self = nullptr;
  Parser* p = Unwrap<Parser>(env, info, &self);
  if (p == nullptr) return nullptr;
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t type = HTTP_REQUEST;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_int32(env, argv[0], &type);
  uint64_t max_header = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    double d = 0;
    if (napi_get_value_double(env, argv[2], &d) == napi_ok && d > 0) max_header = static_cast<uint64_t>(d);
  }
  if (max_header == 0) {
    bool found = false;
    if (!EdgeReadExecArgvUint64Option("--max-http-header-size=", &max_header, &found) || !found) {
      max_header = 16 * 1024;
    }
  }
  int32_t lenient = 0;
  if (argc >= 4 && argv[3] != nullptr) napi_get_value_int32(env, argv[3], &lenient);
  llhttp_init(&p->parser, static_cast<llhttp_type_t>(type), &p->settings);
  p->parser.data = p;
  p->max_http_header_size = max_header;
  p->header_nread = 0;
  p->chunk_extensions_nread = 0;
  p->fields.clear();
  p->values.clear();
  p->url.clear();
  p->status_message.clear();
  p->last_was_value = false;
  p->headers_completed = false;
  p->trim_header_value_prefix = false;
  p->have_flushed = false;
  p->got_exception = false;
  p->propagate_callback_exceptions = false;
  ClearConsumedStreamBinding(p);
  ParserDetachFromConnectionsList(p);
  if (argc >= 5 && argv[4] != nullptr) {
    bool is_null = false;
    napi_valuetype value_type = napi_undefined;
    if (napi_typeof(env, argv[4], &value_type) == napi_ok) {
      is_null = (value_type == napi_null || value_type == napi_undefined);
    }
    if (!is_null) {
      ConnectionsList* list = nullptr;
      if (napi_unwrap(env, argv[4], reinterpret_cast<void**>(&list)) == napi_ok &&
          list != nullptr &&
          !list->finalized) {
        p->list = list;
        p->last_message_start = uv_hrtime();
        list->all.insert(p);
        list->active.insert(p);
      }
    }
  }
  p->provider_type = (type == HTTP_REQUEST) ? kEdgeProviderHttpIncomingMessage
                                            : kEdgeProviderHttpClientRequest;
  if (p->async_id > 0) {
    EdgeAsyncWrapReset(env, &p->async_id);
  } else {
    p->async_id = EdgeAsyncWrapNextId(env);
  }
  napi_value resource = self;
  if (argc >= 2 && argv[1] != nullptr) {
    napi_valuetype resource_type = napi_undefined;
    if (napi_typeof(env, argv[1], &resource_type) == napi_ok &&
        (resource_type == napi_object || resource_type == napi_function)) {
      resource = argv[1];
    }
  }
  EdgeAsyncWrapEmitInit(
      env, p->async_id, p->provider_type, EdgeAsyncWrapExecutionAsyncId(env), resource);
  if (lenient & static_cast<int32_t>(kLenientHeaders)) llhttp_set_lenient_headers(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientChunkedLength)) llhttp_set_lenient_chunked_length(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientKeepAlive)) llhttp_set_lenient_keep_alive(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientTransferEncoding)) llhttp_set_lenient_transfer_encoding(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientVersion)) llhttp_set_lenient_version(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientDataAfterClose)) llhttp_set_lenient_data_after_close(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientOptionalLFAfterCR)) llhttp_set_lenient_optional_lf_after_cr(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientOptionalCRLFAfterChunk)) llhttp_set_lenient_optional_crlf_after_chunk(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientOptionalCRBeforeLF)) llhttp_set_lenient_optional_cr_before_lf(&p->parser, 1);
  if (lenient & static_cast<int32_t>(kLenientSpacesAfterChunkSize)) llhttp_set_lenient_spaces_after_chunk_size(&p->parser, 1);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserExecuteCommon(Parser* p, const char* data, size_t len) {
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET http_parser execute parser=%p len=%zu finish=%d\n",
                 static_cast<void*>(p),
                 len,
                 data == nullptr);
  }
  edge::EscapableHandleScope scope(p != nullptr ? p->env : nullptr);
  if (!scope.is_open()) return nullptr;

  p->current_buffer_data = data;
  p->current_buffer_len = len;
  p->got_exception = false;
  llhttp_errno_t err = data == nullptr ? llhttp_finish(&p->parser) : llhttp_execute(&p->parser, data, len);
  size_t nread = len;
  if (err != HPE_OK && data != nullptr) {
    const char* pos = llhttp_get_error_pos(&p->parser);
    if (pos != nullptr && pos >= data) nread = static_cast<size_t>(pos - data);
    if (err == HPE_PAUSED_UPGRADE) {
      err = HPE_OK;
      llhttp_resume_after_upgrade(&p->parser);
    }
  }
  if (p->pending_pause) {
    p->pending_pause = false;
    llhttp_pause(&p->parser);
  }
  p->current_buffer_data = nullptr;
  p->current_buffer_len = 0;
  if (p->got_exception) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr,
                   "EDGE_TRACE_NET http_parser execute_exception parser=%p\n",
                   static_cast<void*>(p));
    }
    napi_value pending = nullptr;
    if (napi_get_and_clear_last_exception(p->env, &pending) == napi_ok && pending != nullptr) {
      napi_throw(p->env, pending);
    }
    return nullptr;
  }
  if (llhttp_get_upgrade(&p->parser) == 0 && err != HPE_OK) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr,
                   "EDGE_TRACE_NET http_parser execute_error parser=%p err=%s nread=%zu\n",
                   static_cast<void*>(p),
                   llhttp_errno_name(err) != nullptr ? llhttp_errno_name(err) : "<unknown>",
                   nread);
    }
    const char* raw_reason = llhttp_get_error_reason(&p->parser);
    std::string reason = (raw_reason != nullptr) ? raw_reason : "Unknown error";
    std::string code = llhttp_errno_name(err) != nullptr ? llhttp_errno_name(err) : "HPE_UNKNOWN";
    if (err == HPE_USER && raw_reason != nullptr && std::strncmp(raw_reason, "HPE_", 4) == 0) {
      const char* colon = std::strchr(raw_reason, ':');
      if (colon != nullptr && colon > raw_reason) {
        code.assign(raw_reason, static_cast<size_t>(colon - raw_reason));
        reason.assign(colon + 1);
      }
    }
    std::string message = "Parse Error";
    if (!reason.empty()) {
      message += ": ";
      message += reason;
    }
    napi_value error = MakeError(p->env,
                                 message.c_str(),
                                 code.c_str(),
                                 reason.c_str(),
                                 static_cast<uint32_t>(nread),
                                 data,
                                 len);
    return scope.Escape(error);
  }
  if (data == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(p->env, &undefined);
    return scope.Escape(undefined);
  }
  napi_value nread_v = nullptr;
  napi_create_uint32(p->env, static_cast<uint32_t>(nread), &nread_v);
  if (TraceNetEnabled()) {
    std::fprintf(stderr,
                 "EDGE_TRACE_NET http_parser execute_done parser=%p nread=%zu headers_completed=%d\n",
                 static_cast<void*>(p),
                 nread,
                 p->headers_completed);
  }
  return scope.Escape(nread_v);
}

bool DispatchConsumedParserRead(Parser* p,
                                napi_value parser_obj,
                                const char* data,
                                size_t len) {
  if (p == nullptr || parser_obj == nullptr || data == nullptr || len == 0) return false;

  napi_value ret = ParserExecuteCommon(p, data, len);
  if (ret == nullptr) return true;

  napi_value onexecute = nullptr;
  if (napi_get_element(p->env, parser_obj, kOnExecute, &onexecute) != napi_ok ||
      onexecute == nullptr) {
    return true;
  }
  napi_valuetype onexecute_type = napi_undefined;
  if (napi_typeof(p->env, onexecute, &onexecute_type) != napi_ok ||
      onexecute_type != napi_function) {
    if (TraceNetEnabled()) {
      std::fprintf(stderr,
                   "EDGE_TRACE_NET http_parser no_onexecute parser=%p type=%d\n",
                   static_cast<void*>(p),
                   static_cast<int>(onexecute_type));
    }
    return true;
  }

  // Mirror Node's Parser::OnStreamRead semantics: keep current buffer visible
  // while invoking kOnExecute so getCurrentBuffer() returns the active chunk.
  p->current_buffer_data = data;
  p->current_buffer_len = len;
  napi_value argv[1] = {ret};
  napi_value ignored = nullptr;
  EdgeMakeCallback(p->env, parser_obj, onexecute, 1, argv, &ignored);
  if (TraceNetEnabled()) {
    bool pending = false;
    (void)napi_is_exception_pending(p->env, &pending);
    std::fprintf(stderr,
                 "EDGE_TRACE_NET http_parser onexecute_done parser=%p pending_exception=%d\n",
                 static_cast<void*>(p),
                 pending);
  }
  p->current_buffer_data = nullptr;
  p->current_buffer_len = 0;
  return true;
}

napi_value ParserExecute(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) {
    napi_value msg = nullptr;
    napi_create_string_utf8(env, "Invalid this for HTTPParser.execute", NAPI_AUTO_LENGTH, &msg);
    napi_value err = nullptr;
    napi_create_type_error(env, nullptr, msg, &err);
    if (err != nullptr) napi_throw(env, err);
    return nullptr;
  }
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) {
    napi_value zero = nullptr;
    napi_create_uint32(env, 0, &zero);
    return zero;
  }
  bool is_typed_array = false;
  napi_is_typedarray(env, argv[0], &is_typed_array);
  if (!is_typed_array) {
    bool is_buffer = false;
    napi_is_buffer(env, argv[0], &is_buffer);
    if (is_buffer) {
      void* data = nullptr;
      size_t total_len = 0;
      napi_get_buffer_info(env, argv[0], &data, &total_len);
      uint32_t start = 0;
      uint32_t len = static_cast<uint32_t>(total_len);
      if (argc >= 2 && argv[1] != nullptr) napi_get_value_uint32(env, argv[1], &start);
      if (argc >= 3 && argv[2] != nullptr) napi_get_value_uint32(env, argv[2], &len);
      if (start > total_len) start = static_cast<uint32_t>(total_len);
      size_t available = total_len - start;
      if (len > available) len = static_cast<uint32_t>(available);
      const char* ptr = static_cast<const char*>(data) + start;
      const bool restore = p->propagate_callback_exceptions;
      p->propagate_callback_exceptions = true;
      napi_value result = ParserExecuteCommon(p, ptr, len);
      p->propagate_callback_exceptions = restore;
      return result;
    }
    napi_value zero = nullptr;
    napi_create_uint32(env, 0, &zero);
    return zero;
  }
  napi_typedarray_type type;
  size_t length = 0;
  void* data = nullptr;
  napi_value ab = nullptr;
  size_t offset = 0;
  napi_get_typedarray_info(env, argv[0], &type, &length, &data, &ab, &offset);
  uint32_t start = 0;
  uint32_t len = static_cast<uint32_t>(length);
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_uint32(env, argv[1], &start);
  if (argc >= 3 && argv[2] != nullptr) napi_get_value_uint32(env, argv[2], &len);
  if (start > length) start = static_cast<uint32_t>(length);
  size_t available = length - start;
  if (len > available) len = static_cast<uint32_t>(available);
  const char* ptr = static_cast<const char*>(data) + start;
  const bool restore = p->propagate_callback_exceptions;
  p->propagate_callback_exceptions = true;
  napi_value result = ParserExecuteCommon(p, ptr, len);
  p->propagate_callback_exceptions = restore;
  return result;
}

napi_value ParserFinish(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  const bool restore = p->propagate_callback_exceptions;
  p->propagate_callback_exceptions = true;
  napi_value result = ParserExecuteCommon(p, nullptr, 0);
  p->propagate_callback_exceptions = restore;
  return result;
}

napi_value ParserPause(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  llhttp_pause(&p->parser);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserResume(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  llhttp_resume(&p->parser);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserConsume(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  Parser* p = Unwrap<Parser>(env, info, &self);
  if (p == nullptr) return nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) == napi_ok) {
    ClearConsumedStreamBinding(p);
    if (argc >= 1 && argv[0] != nullptr && self != nullptr) {
      EdgeStreamBase* stream = EdgeStreamBaseFromValue(env, argv[0]);
      if (stream == nullptr) {
        std::fputs("HTTPParser.consume() failed: invalid stream handle\n", stderr);
        std::fflush(stderr);
        std::abort();
      }
      if (EdgeStreamBasePushListener(stream, &p->consumed_listener)) {
        p->consumed_stream = stream;
        if (TraceNetEnabled()) {
          std::fprintf(stderr,
                       "EDGE_TRACE_NET http_parser consume parser=%p stream=%p listener=%p ok=1\n",
                       static_cast<void*>(p),
                       static_cast<void*>(stream),
                       static_cast<void*>(&p->consumed_listener));
        }
      } else if (TraceNetEnabled()) {
        std::fprintf(stderr,
                     "EDGE_TRACE_NET http_parser consume parser=%p stream=%p listener=%p ok=0\n",
                     static_cast<void*>(p),
                     static_cast<void*>(stream),
                     static_cast<void*>(&p->consumed_listener));
      }
    }
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserUnconsume(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  ClearConsumedStreamBinding(p);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserGetCurrentBuffer(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  return CreateBufferCopy(env, p->current_buffer_data, p->current_buffer_len);
}

napi_value ParserRemove(napi_env env, napi_callback_info info) {
  Parser* p = Unwrap<Parser>(env, info);
  if (p == nullptr) return nullptr;
  ClearConsumedStreamBinding(p);
  ParserDetachFromConnectionsList(p);
  p->last_message_start = 0;
  ParserQueueDestroy(p);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserClose(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  if (self == nullptr) return nullptr;

  Parser* p = nullptr;
  if (napi_unwrap(env, self, reinterpret_cast<void**>(&p)) != napi_ok || p == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  ClearConsumedStreamBinding(p);
  ParserDetachFromConnectionsList(p);
  p->last_message_start = 0;
  ParserQueueDestroy(p);

  void* removed = nullptr;
  if (napi_remove_wrap(env, self, &removed) == napi_ok && removed != nullptr) {
    Parser* removed_parser = static_cast<Parser*>(removed);
    if (removed_parser->wrapper_ref != nullptr) {
      napi_delete_reference(env, removed_parser->wrapper_ref);
      removed_parser->wrapper_ref = nullptr;
    }
    delete removed_parser;
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ParserFree(napi_env env, napi_callback_info info) {
  return ParserRemove(env, info);
}

napi_value ConnectionsListAll(napi_env env, napi_callback_info info) {
  ConnectionsList* list = Unwrap<ConnectionsList>(env, info);
  if (list == nullptr || list->finalized) {
    napi_value empty = nullptr;
    napi_create_array_with_length(env, 0, &empty);
    return empty;
  }
  napi_value arr = nullptr;
  napi_create_array_with_length(env, list->all.size(), &arr);
  uint32_t idx = 0;
  for (Parser* p : list->all) {
    napi_value obj = GetWrappedObject(env, p->wrapper_ref);
    if (obj != nullptr) napi_set_element(env, arr, idx++, obj);
  }
  return arr;
}

napi_value ConnectionsListIdle(napi_env env, napi_callback_info info) {
  ConnectionsList* list = Unwrap<ConnectionsList>(env, info);
  if (list == nullptr || list->finalized) {
    napi_value empty = nullptr;
    napi_create_array_with_length(env, 0, &empty);
    return empty;
  }
  napi_value arr = nullptr;
  napi_create_array(env, &arr);
  uint32_t idx = 0;
  for (Parser* p : list->all) {
    if (p->last_message_start == 0) {
      napi_value obj = GetWrappedObject(env, p->wrapper_ref);
      if (obj != nullptr) napi_set_element(env, arr, idx++, obj);
    }
  }
  return arr;
}

napi_value ConnectionsListActive(napi_env env, napi_callback_info info) {
  ConnectionsList* list = Unwrap<ConnectionsList>(env, info);
  if (list == nullptr || list->finalized) {
    napi_value empty = nullptr;
    napi_create_array_with_length(env, 0, &empty);
    return empty;
  }
  napi_value arr = nullptr;
  napi_create_array_with_length(env, list->active.size(), &arr);
  uint32_t idx = 0;
  for (Parser* p : list->active) {
    napi_value obj = GetWrappedObject(env, p->wrapper_ref);
    if (obj != nullptr) napi_set_element(env, arr, idx++, obj);
  }
  return arr;
}

napi_value ConnectionsListExpired(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  ConnectionsList* list = Unwrap<ConnectionsList>(env, info);
  if (list == nullptr || list->finalized) {
    napi_value empty = nullptr;
    napi_create_array_with_length(env, 0, &empty);
    return empty;
  }
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  uint32_t headers_timeout_ms = 0;
  uint32_t request_timeout_ms = 0;
  if (argc > 0 && argv[0] != nullptr) napi_get_value_uint32(env, argv[0], &headers_timeout_ms);
  if (argc > 1 && argv[1] != nullptr) napi_get_value_uint32(env, argv[1], &request_timeout_ms);
  uint64_t headers_timeout = static_cast<uint64_t>(headers_timeout_ms) * 1000000ULL;
  uint64_t request_timeout = static_cast<uint64_t>(request_timeout_ms) * 1000000ULL;
  if (request_timeout > 0 && headers_timeout > request_timeout) std::swap(headers_timeout, request_timeout);
  const uint64_t now = uv_hrtime();
  const uint64_t headers_deadline = (headers_timeout > 0 && now > headers_timeout) ? now - headers_timeout : 0;
  const uint64_t request_deadline = (request_timeout > 0 && now > request_timeout) ? now - request_timeout : 0;

  std::vector<Parser*> expired;
  for (Parser* p : list->active) {
    const bool header_expired =
        !p->headers_completed && headers_deadline > 0 && p->last_message_start <= headers_deadline;
    const bool req_expired =
        request_deadline > 0 && p->last_message_start <= request_deadline;
    if (header_expired || req_expired) expired.push_back(p);
  }
  for (Parser* p : expired) list->active.erase(p);

  napi_value arr = nullptr;
  napi_create_array_with_length(env, expired.size(), &arr);
  for (uint32_t i = 0; i < expired.size(); i++) {
    napi_value obj = GetWrappedObject(env, expired[i]->wrapper_ref);
    if (obj != nullptr) napi_set_element(env, arr, i, obj);
  }
  return arr;
}

void SetNamedValue(napi_env env, napi_value obj, const char* key, napi_value val) {
  if (obj != nullptr && val != nullptr) napi_set_named_property(env, obj, key, val);
}

void SetNamedU32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value v = nullptr;
  napi_create_uint32(env, value, &v);
  SetNamedValue(env, obj, key, v);
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

napi_value CreateMethodsArray(napi_env env, bool all_methods) {
  std::vector<const char*> names;
#define ADD_METHOD(num, name, string) names.push_back(#string);
  if (all_methods) {
    HTTP_ALL_METHOD_MAP(ADD_METHOD)
  } else {
    HTTP_METHOD_MAP(ADD_METHOD)
  }
#undef ADD_METHOD
  napi_value arr = nullptr;
  napi_create_array_with_length(env, names.size(), &arr);
  for (uint32_t i = 0; i < names.size(); i++) {
    napi_value s = nullptr;
    napi_create_string_utf8(env, names[i], NAPI_AUTO_LENGTH, &s);
    if (s != nullptr) napi_set_element(env, arr, i, s);
  }
  return arr;
}

}  // namespace

napi_value EdgeInstallHttpParserBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor parser_props[] = {
      {"close", nullptr, ParserClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"free", nullptr, ParserFree, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"remove", nullptr, ParserRemove, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"execute", nullptr, ParserExecute, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"finish", nullptr, ParserFinish, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"initialize", nullptr, ParserInitialize, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"pause", nullptr, ParserPause, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"resume", nullptr, ParserResume, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"consume", nullptr, ParserConsume, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"unconsume", nullptr, ParserUnconsume, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getCurrentBuffer", nullptr, ParserGetCurrentBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };

  napi_value parser_ctor = nullptr;
  if (napi_define_class(env,
                        "HTTPParser",
                        NAPI_AUTO_LENGTH,
                        ParserCtor,
                        nullptr,
                        sizeof(parser_props) / sizeof(parser_props[0]),
                        parser_props,
                        &parser_ctor) != napi_ok ||
      parser_ctor == nullptr) {
    return nullptr;
  }

  SetNamedU32(env, parser_ctor, "REQUEST", HTTP_REQUEST);
  SetNamedU32(env, parser_ctor, "RESPONSE", HTTP_RESPONSE);
  SetNamedU32(env, parser_ctor, "kOnMessageBegin", kOnMessageBegin);
  SetNamedU32(env, parser_ctor, "kOnHeaders", kOnHeaders);
  SetNamedU32(env, parser_ctor, "kOnHeadersComplete", kOnHeadersComplete);
  SetNamedU32(env, parser_ctor, "kOnBody", kOnBody);
  SetNamedU32(env, parser_ctor, "kOnMessageComplete", kOnMessageComplete);
  SetNamedU32(env, parser_ctor, "kOnExecute", kOnExecute);
  SetNamedU32(env, parser_ctor, "kOnTimeout", kOnTimeout);

  SetNamedU32(env, parser_ctor, "kLenientNone", kLenientNone);
  SetNamedU32(env, parser_ctor, "kLenientHeaders", kLenientHeaders);
  SetNamedU32(env, parser_ctor, "kLenientChunkedLength", kLenientChunkedLength);
  SetNamedU32(env, parser_ctor, "kLenientKeepAlive", kLenientKeepAlive);
  SetNamedU32(env, parser_ctor, "kLenientTransferEncoding", kLenientTransferEncoding);
  SetNamedU32(env, parser_ctor, "kLenientVersion", kLenientVersion);
  SetNamedU32(env, parser_ctor, "kLenientDataAfterClose", kLenientDataAfterClose);
  SetNamedU32(env, parser_ctor, "kLenientOptionalLFAfterCR", kLenientOptionalLFAfterCR);
  SetNamedU32(env, parser_ctor, "kLenientOptionalCRLFAfterChunk", kLenientOptionalCRLFAfterChunk);
  SetNamedU32(env, parser_ctor, "kLenientOptionalCRBeforeLF", kLenientOptionalCRBeforeLF);
  SetNamedU32(env, parser_ctor, "kLenientSpacesAfterChunkSize", kLenientSpacesAfterChunkSize);
  SetNamedU32(env, parser_ctor, "kLenientAll", kLenientAll);

  napi_property_descriptor list_props[] = {
      {"all", nullptr, ConnectionsListAll, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"idle", nullptr, ConnectionsListIdle, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"active", nullptr, ConnectionsListActive, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"expired", nullptr, ConnectionsListExpired, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };
  napi_value list_ctor = nullptr;
  if (napi_define_class(env,
                        "ConnectionsList",
                        NAPI_AUTO_LENGTH,
                        ConnectionsListCtor,
                        nullptr,
                        sizeof(list_props) / sizeof(list_props[0]),
                        list_props,
                        &list_ctor) != napi_ok ||
      list_ctor == nullptr) {
    return nullptr;
  }

  SetNamedValue(env, binding, "HTTPParser", parser_ctor);
  SetNamedValue(env, binding, "ConnectionsList", list_ctor);
  SetNamedValue(env, binding, "methods", CreateMethodsArray(env, false));
  SetNamedValue(env, binding, "allMethods", CreateMethodsArray(env, true));

  return binding;
}
