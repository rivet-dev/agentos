#include "internal_binding/binding_initializers.h"
#include "internal_binding/binding_performance.h"

#include <algorithm>
#include <array>
#include <cstdarg>
#include <climits>
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <cstring>
#include <deque>
#include <memory>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#include "nghttp2/nghttp2.h"
#include "uv.h"

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "../edge_async_wrap.h"
#include "../edge_handle_scope.h"
#include "../edge_runtime.h"
#include "../edge_runtime_platform.h"
#include "../edge_stream_base.h"
#include "../edge_stream_wrap.h"

namespace internal_binding {

namespace {

constexpr size_t kSettingsCount = 7;
constexpr size_t kMaxAdditionalSettings = 10;
constexpr size_t kSettingsBufferLength = kSettingsCount + 1 + 1 + (2 * kMaxAdditionalSettings);
constexpr size_t kSessionStateCount = 9;
constexpr size_t kStreamStateCount = 6;
constexpr size_t kStreamStatsCount = 6;
constexpr size_t kSessionStatsCount = 9;
constexpr size_t kOptionsBufferLength = 14;

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

constexpr uint32_t kDefaultSettingsHeaderTableSize = 4096;
constexpr uint32_t kDefaultSettingsEnablePush = 1;
constexpr uint32_t kDefaultSettingsMaxConcurrentStreams = 0xffffffffu;
constexpr uint32_t kDefaultSettingsInitialWindowSize = 65535;
constexpr uint32_t kDefaultSettingsMaxFrameSize = 16384;
constexpr uint32_t kDefaultSettingsMaxHeaderListSize = 65535;
constexpr uint32_t kDefaultSettingsEnableConnectProtocol = 0;
constexpr uint32_t kMaxMaxFrameSize = 16777215;
constexpr uint32_t kMaxMaxHeaderListSize = 16777215;
constexpr uint32_t kMinMaxFrameSize = kDefaultSettingsMaxFrameSize;
constexpr uint32_t kMaxInitialWindowSize = 2147483647;
constexpr uint32_t kDefaultMaxHeaderListPairs = 128;
constexpr size_t kDefaultMaxPings = 10;
constexpr size_t kDefaultMaxSettings = 10;
constexpr uint64_t kDefaultMaxSessionMemory = 10000000;
constexpr uint32_t kPerformanceEntryHttp2 = 2;

constexpr int kStreamOptionEmptyPayload = 0x1;
constexpr int kStreamOptionGetTrailers = 0x2;
constexpr int kSessionTypeServer = 0;
constexpr int kSessionTypeClient = 1;

struct SessionJSFields {
  uint8_t bitfield = 0;
  uint8_t priority_listener_count = 0;
  uint8_t frame_error_listener_count = 0;
  uint32_t max_invalid_frames = 1000;
  uint32_t max_rejected_streams = 100;
};

enum SessionUint8Fields {
  kBitfield = offsetof(SessionJSFields, bitfield),
  kSessionPriorityListenerCount = offsetof(SessionJSFields, priority_listener_count),
  kSessionFrameErrorListenerCount = offsetof(SessionJSFields, frame_error_listener_count),
  kSessionMaxInvalidFrames = offsetof(SessionJSFields, max_invalid_frames),
  kSessionMaxRejectedStreams = offsetof(SessionJSFields, max_rejected_streams),
  kSessionUint8FieldCount = sizeof(SessionJSFields),
};

enum SessionBitfieldFlags {
  kSessionHasRemoteSettingsListeners = 0,
  kSessionRemoteSettingsIsUpToDate = 1,
  kSessionHasPingListeners = 2,
  kSessionHasAltsvcListeners = 3,
};

enum SettingsIndex {
  kSettingsHeaderTableSize = 0,
  kSettingsEnablePush = 1,
  kSettingsInitialWindowSize = 2,
  kSettingsMaxFrameSize = 3,
  kSettingsMaxConcurrentStreams = 4,
  kSettingsMaxHeaderListSize = 5,
  kSettingsEnableConnectProtocol = 6,
  kSettingsFlags = 7,
};

enum OptionsIndex {
  kOptionsMaxDeflateDynamicTableSize = 0,
  kOptionsMaxReservedRemoteStreams = 1,
  kOptionsMaxSendHeaderBlockLength = 2,
  kOptionsPeerMaxConcurrentStreams = 3,
  kOptionsPaddingStrategy = 4,
  kOptionsMaxHeaderListPairs = 5,
  kOptionsMaxOutstandingPings = 6,
  kOptionsMaxOutstandingSettings = 7,
  kOptionsMaxSessionMemory = 8,
  kOptionsMaxSettings = 9,
  kOptionsStreamResetRate = 10,
  kOptionsStreamResetBurst = 11,
  kOptionsStrictFieldWhitespaceValidation = 12,
  kOptionsFlags = 13,
};

enum SessionStateIndex {
  kSessionStateEffectiveLocalWindowSize = 0,
  kSessionStateEffectiveRecvDataLength = 1,
  kSessionStateNextStreamId = 2,
  kSessionStateLocalWindowSize = 3,
  kSessionStateLastProcStreamId = 4,
  kSessionStateRemoteWindowSize = 5,
  kSessionStateOutboundQueueSize = 6,
  kSessionStateHdDeflateDynamicTableSize = 7,
  kSessionStateHdInflateDynamicTableSize = 8,
};

enum StreamStateIndex {
  kStreamStateState = 0,
  kStreamStateWeight = 1,
  kStreamStateSumDependencyWeight = 2,
  kStreamStateLocalClose = 3,
  kStreamStateRemoteClose = 4,
  kStreamStateLocalWindowSize = 5,
};

enum Http2Callbacks {
  kCallbackError = 0,
  kCallbackPriority = 1,
  kCallbackSettings = 2,
  kCallbackPing = 3,
  kCallbackHeaders = 4,
  kCallbackFrameError = 5,
  kCallbackGoawayData = 6,
  kCallbackAltsvc = 7,
  kCallbackOrigin = 8,
  kCallbackStreamTrailers = 9,
  kCallbackStreamClose = 10,
  kCallbackCount = 11,
};

struct NamedIntConstant {
  const char* name;
  int64_t value;
};

struct NamedStringConstant {
  const char* name;
  const char* value;
};

struct Http2BindingState {
  explicit Http2BindingState(napi_env env_in) : env(env_in) {}
  ~Http2BindingState() {
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &session_ctor_ref);
    DeleteRefIfPresent(env, &stream_ctor_ref);
    for (napi_ref& ref : callback_refs) {
      DeleteRefIfPresent(env, &ref);
    }
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref session_ctor_ref = nullptr;
  napi_ref stream_ctor_ref = nullptr;
  std::array<napi_ref, kCallbackCount> callback_refs{};
  double* session_state = nullptr;
  double* stream_state = nullptr;
  double* stream_stats = nullptr;
  double* session_stats = nullptr;
  uint32_t* options_buffer = nullptr;
  uint32_t* settings_buffer = nullptr;
};

struct Http2SessionWrap;

struct OutboundChunk {
  std::vector<uint8_t> data;
  size_t offset = 0;
  napi_ref req_ref = nullptr;
};

struct PendingHeaderBlock {
  int32_t stream_id = 0;
  int32_t category = NGHTTP2_HCAT_HEADERS;
  uint8_t flags = 0;
  size_t total_length = 0;
  std::vector<std::string> headers;
  std::vector<std::string> sensitive_headers;
};

struct PendingSettings {
  napi_ref callback_ref = nullptr;
  napi_ref resource_ref = nullptr;
  int64_t async_id = -1;
  uint64_t start_time = 0;
};

struct PendingPing {
  napi_ref callback_ref = nullptr;
  napi_ref resource_ref = nullptr;
  int64_t async_id = -1;
  uint64_t start_time = 0;
  std::array<uint8_t, 8> payload{};
};

struct DeferredSessionFlushTask {
  Http2SessionWrap* session = nullptr;
  bool wrapper_refed = false;
};

struct CustomSettingsState {
  size_t number = 0;
  std::array<nghttp2_settings_entry, kMaxAdditionalSettings> entries{};
};

struct Http2SessionWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref fields_ref = nullptr;
  SessionJSFields* fields = nullptr;
  napi_ref parent_handle_ref = nullptr;
  EdgeStreamBase* parent_stream_base = nullptr;
  EdgeStreamListener parent_stream_listener{};
  // The base our parent_stream_listener is actually registered on. Tracked
  // separately from parent_stream_base because several paths null the latter
  // (its "native parent stream is usable" meaning) without unregistering the
  // listener. Kept in lockstep with registration so Http2SessionFinalize can
  // always remove the listener before `delete wrap` frees it — otherwise it
  // dangles in the parent stream's listener_state.current (use-after-free).
  EdgeStreamBase* parent_stream_listener_base = nullptr;
  int32_t type = kSessionTypeServer;
  int64_t async_id = -1;
  int32_t next_stream_id = 2;
  uint64_t chunks_sent_since_last_write = 0;
  nghttp2_session* session = nullptr;
  nghttp2_session_callbacks* callbacks = nullptr;
  std::unordered_map<int32_t, PendingHeaderBlock> pending_headers;
  std::unordered_map<int32_t, struct Http2StreamWrap*> streams;
  std::unordered_set<int32_t> closed_stream_ids;
  std::vector<int32_t> pending_rst_stream_ids;
  std::unordered_map<int32_t, uint32_t> pending_rst_codes;
  std::deque<PendingSettings> pending_settings;
  std::deque<PendingPing> pending_pings;
  std::vector<uint8_t> pending_parent_read_bytes;
  size_t pending_parent_read_offset = 0;
  std::deque<std::vector<uint8_t>> pending_parent_write_chunks;
  size_t pending_parent_write_length = 0;
  std::deque<napi_ref> pending_parent_write_req_refs;
  CustomSettingsState local_custom_settings;
  CustomSettingsState remote_custom_settings;
  uint32_t invalid_frame_count = 0;
  uint32_t rejected_stream_count = 0;
  uint32_t padding_strategy = 0;
  size_t max_header_pairs = kDefaultMaxHeaderListPairs;
  size_t max_header_length = kDefaultSettingsMaxHeaderListSize;
  size_t max_outstanding_pings = kDefaultMaxPings;
  size_t max_outstanding_settings = kDefaultMaxSettings;
  uint64_t max_session_memory = kDefaultMaxSessionMemory;
  uint64_t frames_received = 0;
  uint64_t frames_sent = 0;
  uint64_t data_received = 0;
  uint64_t data_sent = 0;
  uint64_t start_time = 0;
  const char* custom_recv_error_code = nullptr;
  bool consumed = false;
  bool wants_graceful_close = false;
  bool graceful_close_notified = false;
  bool destroyed = false;
  bool done_notified = false;
  bool parent_write_in_progress = false;
  bool parent_reading_stopped = false;
  bool parent_closed = false;
  bool receive_paused = false;
  bool in_scope = false;
  bool write_scheduled = false;
  bool goaway_pending = false;
};

struct Http2StreamWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  napi_ref onread_ref = nullptr;
  EdgeStreamBase base{};
  Http2SessionWrap* session = nullptr;
  int32_t id = 0;
  int64_t async_id = -1;
  int32_t current_category = NGHTTP2_HCAT_HEADERS;
  uint64_t bytes_written = 0;
  uint32_t rst_code = NGHTTP2_NO_ERROR;
  std::deque<OutboundChunk> outbound_chunks;
  std::deque<std::vector<uint8_t>> inbound_chunks;
  napi_ref shutdown_req_ref = nullptr;
  size_t buffered_inbound_bytes = 0;
  size_t available_outbound_length = 0;
  size_t max_header_pairs = kDefaultMaxHeaderListPairs;
  size_t max_header_length = kDefaultSettingsMaxHeaderListSize;
  bool reading = false;
  bool shutdown_requested = false;
  bool local_closed = false;
  bool remote_closed = false;
  bool inbound_eof = false;
  bool has_trailers = false;
  bool trailers_notified = false;
  bool destroyed = false;
  bool destroy_scheduled = false;
  bool close_pending = false;
  bool session_ref_held = false;
  uint32_t pending_close_code = NGHTTP2_NO_ERROR;
};

const char* SessionTypeName(const Http2SessionWrap* session) {
  return session != nullptr && session->type == kSessionTypeClient ? "client" : "server";
}

bool IsHttp2NativeDebugEnabled() {
  const char* value = std::getenv("NODE_DEBUG_NATIVE");
  if (value == nullptr) return false;
  return std::strstr(value, "http2") != nullptr || std::strstr(value, "HTTP2") != nullptr;
}

std::string SessionDiagnosticName(const Http2SessionWrap* session) {
  return std::string("Http2Session ") + SessionTypeName(session) + " (" +
         std::to_string(session != nullptr ? session->async_id : -1) + ")";
}

std::string StreamDiagnosticName(const Http2StreamWrap* stream) {
  std::string session_name = "session already destroyed";
  if (stream != nullptr && stream->session != nullptr) {
    session_name = SessionDiagnosticName(stream->session);
  }
  return "HttpStream " + std::to_string(stream != nullptr ? stream->id : 0) + " (" +
         std::to_string(stream != nullptr ? stream->async_id : -1) + ") [" + session_name + "]";
}

bool SetNamedInt64(napi_env env, napi_value obj, const char* name, int64_t value);
bool SetNamedString(napi_env env, napi_value obj, const char* name, const char* value);
bool GetByteSpan(napi_env env,
                 napi_value value,
                 const uint8_t** data,
                 size_t* len,
                 std::string* temp_utf8);
Http2StreamWrap* UnwrapStream(napi_env env, napi_value value);
void SetStreamWriteState(napi_env env, size_t bytes_written, bool async);
void QueueOutboundChunk(Http2StreamWrap* stream, napi_value req_obj, const uint8_t* data, size_t len);
void MaybeScheduleSessionFlush(Http2SessionWrap* session);

uv_handle_t* Http2StreamGetHandle(EdgeStreamBase*) {
  return nullptr;
}

uv_stream_t* Http2StreamGetLibuvStream(EdgeStreamBase*) {
  return nullptr;
}

int Http2StreamWriteBufferDirect(EdgeStreamBase* base,
                                 napi_value req_obj,
                                 napi_value payload,
                                 bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (base == nullptr || base->env == nullptr) return UV_EBADF;

  napi_value self = EdgeStreamBaseGetWrapper(base);
  Http2StreamWrap* wrap = UnwrapStream(base->env, self);
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || wrap->destroyed) {
    return UV_EPIPE;
  }

  const uint8_t* data = nullptr;
  size_t len = 0;
  std::string temp_utf8;
  if (!GetByteSpan(base->env, payload, &data, &len, &temp_utf8)) {
    return UV_EINVAL;
  }

  if (req_obj != nullptr) {
    EdgeStreamReqActivate(base->env, req_obj, kEdgeProviderWriteWrap, wrap->async_id);
  }
  SetStreamWriteState(base->env, len, true);
  QueueOutboundChunk(wrap, req_obj, data, len);
  (void)nghttp2_session_resume_data(wrap->session->session, wrap->id);
  MaybeScheduleSessionFlush(wrap->session);
  return 0;
}

const EdgeStreamBaseOps kHttp2StreamOps = {
    Http2StreamGetHandle,
    Http2StreamGetLibuvStream,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    nullptr,
    Http2StreamWriteBufferDirect,
};

void DebugSession(Http2SessionWrap* session, const char* format, ...) {
  if (!IsHttp2NativeDebugEnabled() || session == nullptr || format == nullptr) return;
  std::fprintf(stderr, "HTTP2 %d: %s ", uv_os_getpid(), SessionDiagnosticName(session).c_str());
  va_list args;
  va_start(args, format);
  std::vfprintf(stderr, format, args);
  va_end(args);
  std::fputc('\n', stderr);
}

void DebugStream(Http2StreamWrap* stream, const char* format, ...) {
  if (!IsHttp2NativeDebugEnabled() || stream == nullptr || format == nullptr) return;
  std::fprintf(stderr, "HTTP2 %d: %s ", uv_os_getpid(), StreamDiagnosticName(stream).c_str());
  va_list args;
  va_start(args, format);
  std::vfprintf(stderr, format, args);
  va_end(args);
  std::fputc('\n', stderr);
}

void EmitHttp2SessionPerformance(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr ||
      !PerformanceHasObserver(session->env, kPerformanceEntryHttp2)) {
    return;
  }

  napi_value details = nullptr;
  if (napi_create_object(session->env, &details) != napi_ok || details == nullptr) return;
  (void)SetNamedString(
      session->env, details, "type", session->type == kSessionTypeServer ? "server" : "client");
  (void)SetNamedInt64(session->env, details, "framesReceived", session->frames_received);
  (void)SetNamedInt64(session->env, details, "framesSent", session->frames_sent);
  (void)SetNamedInt64(session->env, details, "bytesRead", session->data_received);
  (void)SetNamedInt64(session->env, details, "bytesWritten", session->data_sent);
  (void)SetNamedInt64(session->env, details, "pingRTT", 0);
  (void)SetNamedInt64(session->env, details, "streamCount", static_cast<int64_t>(session->streams.size()));
  (void)SetNamedInt64(session->env, details, "maxConcurrentStreams", 0);
  napi_value average = nullptr;
  if (napi_create_double(session->env, 0, &average) == napi_ok && average != nullptr) {
    napi_set_named_property(session->env, details, "streamAverageDuration", average);
  }
  PerformanceEmitEntry(session->env, "Http2Session", "http2", 0, 0, details);
}

uint64_t CurrentSessionMemory(Http2SessionWrap* session) {
  if (session == nullptr) return 0;
  uint64_t total = sizeof(Http2SessionWrap);
  total += session->pending_parent_read_bytes.size();
  total += session->pending_parent_write_length;
  for (const auto& it : session->pending_headers) {
    total += it.second.total_length;
  }
  for (const auto& it : session->streams) {
    const Http2StreamWrap* stream = it.second;
    if (stream == nullptr) continue;
    total += sizeof(Http2StreamWrap);
    total += stream->available_outbound_length;
    total += stream->buffered_inbound_bytes;
    for (const auto& chunk : stream->inbound_chunks) total += chunk.size();
  }
  return total;
}

bool HasAvailableSessionMemory(Http2SessionWrap* session, uint64_t amount) {
  if (session == nullptr) return false;
  return CurrentSessionMemory(session) + amount <= session->max_session_memory;
}

bool SessionCanAddStream(Http2SessionWrap* session) {
  if (session == nullptr || session->session == nullptr) return false;
  const uint32_t max_concurrent_streams =
      nghttp2_session_get_local_settings(session->session, NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS);
  const size_t max_size = std::min(session->streams.max_size(), static_cast<size_t>(max_concurrent_streams));
  return session->streams.size() < max_size && HasAvailableSessionMemory(session, sizeof(Http2StreamWrap));
}

constexpr std::array<const char*, 14> kErrorCodeNames = {{
    "NGHTTP2_NO_ERROR",
    "NGHTTP2_PROTOCOL_ERROR",
    "NGHTTP2_INTERNAL_ERROR",
    "NGHTTP2_FLOW_CONTROL_ERROR",
    "NGHTTP2_SETTINGS_TIMEOUT",
    "NGHTTP2_STREAM_CLOSED",
    "NGHTTP2_FRAME_SIZE_ERROR",
    "NGHTTP2_REFUSED_STREAM",
    "NGHTTP2_CANCEL",
    "NGHTTP2_COMPRESSION_ERROR",
    "NGHTTP2_CONNECT_ERROR",
    "NGHTTP2_ENHANCE_YOUR_CALM",
    "NGHTTP2_INADEQUATE_SECURITY",
    "NGHTTP2_HTTP_1_1_REQUIRED",
}};

constexpr NamedIntConstant kNumericConstants[] = {
    {"NGHTTP2_ERR_FRAME_SIZE_ERROR", NGHTTP2_ERR_FRAME_SIZE_ERROR},
    {"NGHTTP2_SESSION_SERVER", 0},
    {"NGHTTP2_SESSION_CLIENT", 1},
    {"NGHTTP2_STREAM_STATE_IDLE", NGHTTP2_STREAM_STATE_IDLE},
    {"NGHTTP2_STREAM_STATE_OPEN", NGHTTP2_STREAM_STATE_OPEN},
    {"NGHTTP2_STREAM_STATE_RESERVED_LOCAL", NGHTTP2_STREAM_STATE_RESERVED_LOCAL},
    {"NGHTTP2_STREAM_STATE_RESERVED_REMOTE", NGHTTP2_STREAM_STATE_RESERVED_REMOTE},
    {"NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL", NGHTTP2_STREAM_STATE_HALF_CLOSED_LOCAL},
    {"NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE", NGHTTP2_STREAM_STATE_HALF_CLOSED_REMOTE},
    {"NGHTTP2_STREAM_STATE_CLOSED", NGHTTP2_STREAM_STATE_CLOSED},
    {"NGHTTP2_FLAG_NONE", NGHTTP2_FLAG_NONE},
    {"NGHTTP2_FLAG_END_STREAM", NGHTTP2_FLAG_END_STREAM},
    {"NGHTTP2_FLAG_END_HEADERS", NGHTTP2_FLAG_END_HEADERS},
    {"NGHTTP2_FLAG_ACK", NGHTTP2_FLAG_ACK},
    {"NGHTTP2_FLAG_PADDED", NGHTTP2_FLAG_PADDED},
    {"NGHTTP2_FLAG_PRIORITY", NGHTTP2_FLAG_PRIORITY},
    {"DEFAULT_SETTINGS_HEADER_TABLE_SIZE", kDefaultSettingsHeaderTableSize},
    {"DEFAULT_SETTINGS_ENABLE_PUSH", kDefaultSettingsEnablePush},
    {"DEFAULT_SETTINGS_MAX_CONCURRENT_STREAMS", static_cast<int64_t>(kDefaultSettingsMaxConcurrentStreams)},
    {"DEFAULT_SETTINGS_INITIAL_WINDOW_SIZE", kDefaultSettingsInitialWindowSize},
    {"DEFAULT_SETTINGS_MAX_FRAME_SIZE", kDefaultSettingsMaxFrameSize},
    {"DEFAULT_SETTINGS_MAX_HEADER_LIST_SIZE", kDefaultSettingsMaxHeaderListSize},
    {"DEFAULT_SETTINGS_ENABLE_CONNECT_PROTOCOL", kDefaultSettingsEnableConnectProtocol},
    {"MAX_MAX_FRAME_SIZE", kMaxMaxFrameSize},
    {"MIN_MAX_FRAME_SIZE", kMinMaxFrameSize},
    {"MAX_INITIAL_WINDOW_SIZE", kMaxInitialWindowSize},
    {"NGHTTP2_SETTINGS_HEADER_TABLE_SIZE", NGHTTP2_SETTINGS_HEADER_TABLE_SIZE},
    {"NGHTTP2_SETTINGS_ENABLE_PUSH", NGHTTP2_SETTINGS_ENABLE_PUSH},
    {"NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS", NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS},
    {"NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE", NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE},
    {"NGHTTP2_SETTINGS_MAX_FRAME_SIZE", NGHTTP2_SETTINGS_MAX_FRAME_SIZE},
    {"NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE", NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE},
    {"NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL", NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL},
    {"PADDING_STRATEGY_NONE", 0},
    {"PADDING_STRATEGY_ALIGNED", 1},
    {"PADDING_STRATEGY_MAX", 2},
    {"PADDING_STRATEGY_CALLBACK", 1},
    {"NGHTTP2_NO_ERROR", NGHTTP2_NO_ERROR},
    {"NGHTTP2_PROTOCOL_ERROR", NGHTTP2_PROTOCOL_ERROR},
    {"NGHTTP2_INTERNAL_ERROR", NGHTTP2_INTERNAL_ERROR},
    {"NGHTTP2_FLOW_CONTROL_ERROR", NGHTTP2_FLOW_CONTROL_ERROR},
    {"NGHTTP2_SETTINGS_TIMEOUT", NGHTTP2_SETTINGS_TIMEOUT},
    {"NGHTTP2_STREAM_CLOSED", NGHTTP2_STREAM_CLOSED},
    {"NGHTTP2_FRAME_SIZE_ERROR", NGHTTP2_FRAME_SIZE_ERROR},
    {"NGHTTP2_REFUSED_STREAM", NGHTTP2_REFUSED_STREAM},
    {"NGHTTP2_CANCEL", NGHTTP2_CANCEL},
    {"NGHTTP2_COMPRESSION_ERROR", NGHTTP2_COMPRESSION_ERROR},
    {"NGHTTP2_CONNECT_ERROR", NGHTTP2_CONNECT_ERROR},
    {"NGHTTP2_ENHANCE_YOUR_CALM", NGHTTP2_ENHANCE_YOUR_CALM},
    {"NGHTTP2_INADEQUATE_SECURITY", NGHTTP2_INADEQUATE_SECURITY},
    {"NGHTTP2_HTTP_1_1_REQUIRED", NGHTTP2_HTTP_1_1_REQUIRED},
    {"NGHTTP2_DEFAULT_WEIGHT", NGHTTP2_DEFAULT_WEIGHT},
    {"NGHTTP2_HCAT_REQUEST", NGHTTP2_HCAT_REQUEST},
    {"NGHTTP2_HCAT_RESPONSE", NGHTTP2_HCAT_RESPONSE},
    {"NGHTTP2_HCAT_PUSH_RESPONSE", NGHTTP2_HCAT_PUSH_RESPONSE},
    {"NGHTTP2_HCAT_HEADERS", NGHTTP2_HCAT_HEADERS},
};

constexpr NamedIntConstant kHiddenNumericConstants[] = {
    {"NGHTTP2_NV_FLAG_NONE", NGHTTP2_NV_FLAG_NONE},
    {"NGHTTP2_NV_FLAG_NO_INDEX", NGHTTP2_NV_FLAG_NO_INDEX},
    {"NGHTTP2_ERR_DEFERRED", NGHTTP2_ERR_DEFERRED},
    {"NGHTTP2_ERR_STREAM_ID_NOT_AVAILABLE", NGHTTP2_ERR_STREAM_ID_NOT_AVAILABLE},
    {"NGHTTP2_ERR_INVALID_ARGUMENT", NGHTTP2_ERR_INVALID_ARGUMENT},
    {"NGHTTP2_ERR_STREAM_CLOSED", NGHTTP2_ERR_STREAM_CLOSED},
    {"NGHTTP2_ERR_NOMEM", NGHTTP2_ERR_NOMEM},
    {"STREAM_OPTION_EMPTY_PAYLOAD", kStreamOptionEmptyPayload},
    {"STREAM_OPTION_GET_TRAILERS", kStreamOptionGetTrailers},
    {"kBitfield", kBitfield},
    {"kSessionPriorityListenerCount", kSessionPriorityListenerCount},
};

constexpr NamedIntConstant kSessionFieldConstants[] = {
    {"kSessionFrameErrorListenerCount", kSessionFrameErrorListenerCount},
    {"kSessionMaxInvalidFrames", kSessionMaxInvalidFrames},
    {"kSessionMaxRejectedStreams", kSessionMaxRejectedStreams},
    {"kSessionUint8FieldCount", kSessionUint8FieldCount},
    {"kSessionHasRemoteSettingsListeners", kSessionHasRemoteSettingsListeners},
    {"kSessionRemoteSettingsIsUpToDate", kSessionRemoteSettingsIsUpToDate},
    {"kSessionHasPingListeners", kSessionHasPingListeners},
};

constexpr NamedIntConstant kSessionFieldConstantsTail[] = {
    {"kSessionHasAltsvcListeners", kSessionHasAltsvcListeners},
};

constexpr NamedStringConstant kHeaderConstants[] = {
    {"HTTP2_HEADER_STATUS", ":status"},
    {"HTTP2_HEADER_METHOD", ":method"},
    {"HTTP2_HEADER_AUTHORITY", ":authority"},
    {"HTTP2_HEADER_SCHEME", ":scheme"},
    {"HTTP2_HEADER_PATH", ":path"},
    {"HTTP2_HEADER_PROTOCOL", ":protocol"},
    {"HTTP2_HEADER_ACCEPT_ENCODING", "accept-encoding"},
    {"HTTP2_HEADER_ACCEPT_LANGUAGE", "accept-language"},
    {"HTTP2_HEADER_ACCEPT_RANGES", "accept-ranges"},
    {"HTTP2_HEADER_ACCEPT", "accept"},
    {"HTTP2_HEADER_ACCESS_CONTROL_ALLOW_CREDENTIALS", "access-control-allow-credentials"},
    {"HTTP2_HEADER_ACCESS_CONTROL_ALLOW_HEADERS", "access-control-allow-headers"},
    {"HTTP2_HEADER_ACCESS_CONTROL_ALLOW_METHODS", "access-control-allow-methods"},
    {"HTTP2_HEADER_ACCESS_CONTROL_ALLOW_ORIGIN", "access-control-allow-origin"},
    {"HTTP2_HEADER_ACCESS_CONTROL_EXPOSE_HEADERS", "access-control-expose-headers"},
    {"HTTP2_HEADER_ACCESS_CONTROL_REQUEST_HEADERS", "access-control-request-headers"},
    {"HTTP2_HEADER_ACCESS_CONTROL_REQUEST_METHOD", "access-control-request-method"},
    {"HTTP2_HEADER_AGE", "age"},
    {"HTTP2_HEADER_AUTHORIZATION", "authorization"},
    {"HTTP2_HEADER_CACHE_CONTROL", "cache-control"},
    {"HTTP2_HEADER_CONNECTION", "connection"},
    {"HTTP2_HEADER_CONTENT_DISPOSITION", "content-disposition"},
    {"HTTP2_HEADER_CONTENT_ENCODING", "content-encoding"},
    {"HTTP2_HEADER_CONTENT_LENGTH", "content-length"},
    {"HTTP2_HEADER_CONTENT_TYPE", "content-type"},
    {"HTTP2_HEADER_COOKIE", "cookie"},
    {"HTTP2_HEADER_DATE", "date"},
    {"HTTP2_HEADER_ETAG", "etag"},
    {"HTTP2_HEADER_FORWARDED", "forwarded"},
    {"HTTP2_HEADER_HOST", "host"},
    {"HTTP2_HEADER_IF_MODIFIED_SINCE", "if-modified-since"},
    {"HTTP2_HEADER_IF_NONE_MATCH", "if-none-match"},
    {"HTTP2_HEADER_IF_RANGE", "if-range"},
    {"HTTP2_HEADER_LAST_MODIFIED", "last-modified"},
    {"HTTP2_HEADER_LINK", "link"},
    {"HTTP2_HEADER_LOCATION", "location"},
    {"HTTP2_HEADER_RANGE", "range"},
    {"HTTP2_HEADER_REFERER", "referer"},
    {"HTTP2_HEADER_SERVER", "server"},
    {"HTTP2_HEADER_SET_COOKIE", "set-cookie"},
    {"HTTP2_HEADER_STRICT_TRANSPORT_SECURITY", "strict-transport-security"},
    {"HTTP2_HEADER_TRANSFER_ENCODING", "transfer-encoding"},
    {"HTTP2_HEADER_TE", "te"},
    {"HTTP2_HEADER_UPGRADE_INSECURE_REQUESTS", "upgrade-insecure-requests"},
    {"HTTP2_HEADER_UPGRADE", "upgrade"},
    {"HTTP2_HEADER_USER_AGENT", "user-agent"},
    {"HTTP2_HEADER_VARY", "vary"},
    {"HTTP2_HEADER_X_CONTENT_TYPE_OPTIONS", "x-content-type-options"},
    {"HTTP2_HEADER_X_FRAME_OPTIONS", "x-frame-options"},
    {"HTTP2_HEADER_KEEP_ALIVE", "keep-alive"},
    {"HTTP2_HEADER_PROXY_CONNECTION", "proxy-connection"},
    {"HTTP2_HEADER_X_XSS_PROTECTION", "x-xss-protection"},
    {"HTTP2_HEADER_ALT_SVC", "alt-svc"},
    {"HTTP2_HEADER_CONTENT_SECURITY_POLICY", "content-security-policy"},
    {"HTTP2_HEADER_EARLY_DATA", "early-data"},
    {"HTTP2_HEADER_EXPECT_CT", "expect-ct"},
    {"HTTP2_HEADER_ORIGIN", "origin"},
    {"HTTP2_HEADER_PURPOSE", "purpose"},
    {"HTTP2_HEADER_TIMING_ALLOW_ORIGIN", "timing-allow-origin"},
    {"HTTP2_HEADER_X_FORWARDED_FOR", "x-forwarded-for"},
    {"HTTP2_HEADER_PRIORITY", "priority"},
    {"HTTP2_HEADER_ACCEPT_CHARSET", "accept-charset"},
    {"HTTP2_HEADER_ACCESS_CONTROL_MAX_AGE", "access-control-max-age"},
    {"HTTP2_HEADER_ALLOW", "allow"},
    {"HTTP2_HEADER_CONTENT_LANGUAGE", "content-language"},
    {"HTTP2_HEADER_CONTENT_LOCATION", "content-location"},
    {"HTTP2_HEADER_CONTENT_MD5", "content-md5"},
    {"HTTP2_HEADER_CONTENT_RANGE", "content-range"},
    {"HTTP2_HEADER_DNT", "dnt"},
    {"HTTP2_HEADER_EXPECT", "expect"},
    {"HTTP2_HEADER_EXPIRES", "expires"},
    {"HTTP2_HEADER_FROM", "from"},
    {"HTTP2_HEADER_IF_MATCH", "if-match"},
    {"HTTP2_HEADER_IF_UNMODIFIED_SINCE", "if-unmodified-since"},
    {"HTTP2_HEADER_MAX_FORWARDS", "max-forwards"},
    {"HTTP2_HEADER_PREFER", "prefer"},
    {"HTTP2_HEADER_PROXY_AUTHENTICATE", "proxy-authenticate"},
    {"HTTP2_HEADER_PROXY_AUTHORIZATION", "proxy-authorization"},
    {"HTTP2_HEADER_REFRESH", "refresh"},
    {"HTTP2_HEADER_RETRY_AFTER", "retry-after"},
    {"HTTP2_HEADER_TRAILER", "trailer"},
    {"HTTP2_HEADER_TK", "tk"},
    {"HTTP2_HEADER_VIA", "via"},
    {"HTTP2_HEADER_WARNING", "warning"},
    {"HTTP2_HEADER_WWW_AUTHENTICATE", "www-authenticate"},
    {"HTTP2_HEADER_HTTP2_SETTINGS", "http2-settings"},
};

constexpr NamedStringConstant kMethodConstants[] = {
    {"HTTP2_METHOD_ACL", "ACL"},
    {"HTTP2_METHOD_BASELINE_CONTROL", "BASELINE-CONTROL"},
    {"HTTP2_METHOD_BIND", "BIND"},
    {"HTTP2_METHOD_CHECKIN", "CHECKIN"},
    {"HTTP2_METHOD_CHECKOUT", "CHECKOUT"},
    {"HTTP2_METHOD_CONNECT", "CONNECT"},
    {"HTTP2_METHOD_COPY", "COPY"},
    {"HTTP2_METHOD_DELETE", "DELETE"},
    {"HTTP2_METHOD_GET", "GET"},
    {"HTTP2_METHOD_HEAD", "HEAD"},
    {"HTTP2_METHOD_LABEL", "LABEL"},
    {"HTTP2_METHOD_LINK", "LINK"},
    {"HTTP2_METHOD_LOCK", "LOCK"},
    {"HTTP2_METHOD_MERGE", "MERGE"},
    {"HTTP2_METHOD_MKACTIVITY", "MKACTIVITY"},
    {"HTTP2_METHOD_MKCALENDAR", "MKCALENDAR"},
    {"HTTP2_METHOD_MKCOL", "MKCOL"},
    {"HTTP2_METHOD_MKREDIRECTREF", "MKREDIRECTREF"},
    {"HTTP2_METHOD_MKWORKSPACE", "MKWORKSPACE"},
    {"HTTP2_METHOD_MOVE", "MOVE"},
    {"HTTP2_METHOD_OPTIONS", "OPTIONS"},
    {"HTTP2_METHOD_ORDERPATCH", "ORDERPATCH"},
    {"HTTP2_METHOD_PATCH", "PATCH"},
    {"HTTP2_METHOD_POST", "POST"},
    {"HTTP2_METHOD_PRI", "PRI"},
    {"HTTP2_METHOD_PROPFIND", "PROPFIND"},
    {"HTTP2_METHOD_PROPPATCH", "PROPPATCH"},
    {"HTTP2_METHOD_PUT", "PUT"},
    {"HTTP2_METHOD_REBIND", "REBIND"},
    {"HTTP2_METHOD_REPORT", "REPORT"},
    {"HTTP2_METHOD_SEARCH", "SEARCH"},
    {"HTTP2_METHOD_TRACE", "TRACE"},
    {"HTTP2_METHOD_UNBIND", "UNBIND"},
    {"HTTP2_METHOD_UNCHECKOUT", "UNCHECKOUT"},
    {"HTTP2_METHOD_UNLINK", "UNLINK"},
    {"HTTP2_METHOD_UNLOCK", "UNLOCK"},
    {"HTTP2_METHOD_UPDATE", "UPDATE"},
    {"HTTP2_METHOD_UPDATEREDIRECTREF", "UPDATEREDIRECTREF"},
    {"HTTP2_METHOD_VERSION_CONTROL", "VERSION-CONTROL"},
};

constexpr NamedIntConstant kStatusConstants[] = {
    {"HTTP_STATUS_CONTINUE", 100},
    {"HTTP_STATUS_SWITCHING_PROTOCOLS", 101},
    {"HTTP_STATUS_PROCESSING", 102},
    {"HTTP_STATUS_EARLY_HINTS", 103},
    {"HTTP_STATUS_OK", 200},
    {"HTTP_STATUS_CREATED", 201},
    {"HTTP_STATUS_ACCEPTED", 202},
    {"HTTP_STATUS_NON_AUTHORITATIVE_INFORMATION", 203},
    {"HTTP_STATUS_NO_CONTENT", 204},
    {"HTTP_STATUS_RESET_CONTENT", 205},
    {"HTTP_STATUS_PARTIAL_CONTENT", 206},
    {"HTTP_STATUS_MULTI_STATUS", 207},
    {"HTTP_STATUS_ALREADY_REPORTED", 208},
    {"HTTP_STATUS_IM_USED", 226},
    {"HTTP_STATUS_MULTIPLE_CHOICES", 300},
    {"HTTP_STATUS_MOVED_PERMANENTLY", 301},
    {"HTTP_STATUS_FOUND", 302},
    {"HTTP_STATUS_SEE_OTHER", 303},
    {"HTTP_STATUS_NOT_MODIFIED", 304},
    {"HTTP_STATUS_USE_PROXY", 305},
    {"HTTP_STATUS_TEMPORARY_REDIRECT", 307},
    {"HTTP_STATUS_PERMANENT_REDIRECT", 308},
    {"HTTP_STATUS_BAD_REQUEST", 400},
    {"HTTP_STATUS_UNAUTHORIZED", 401},
    {"HTTP_STATUS_PAYMENT_REQUIRED", 402},
    {"HTTP_STATUS_FORBIDDEN", 403},
    {"HTTP_STATUS_NOT_FOUND", 404},
    {"HTTP_STATUS_METHOD_NOT_ALLOWED", 405},
    {"HTTP_STATUS_NOT_ACCEPTABLE", 406},
    {"HTTP_STATUS_PROXY_AUTHENTICATION_REQUIRED", 407},
    {"HTTP_STATUS_REQUEST_TIMEOUT", 408},
    {"HTTP_STATUS_CONFLICT", 409},
    {"HTTP_STATUS_GONE", 410},
    {"HTTP_STATUS_LENGTH_REQUIRED", 411},
    {"HTTP_STATUS_PRECONDITION_FAILED", 412},
    {"HTTP_STATUS_PAYLOAD_TOO_LARGE", 413},
    {"HTTP_STATUS_URI_TOO_LONG", 414},
    {"HTTP_STATUS_UNSUPPORTED_MEDIA_TYPE", 415},
    {"HTTP_STATUS_RANGE_NOT_SATISFIABLE", 416},
    {"HTTP_STATUS_EXPECTATION_FAILED", 417},
    {"HTTP_STATUS_TEAPOT", 418},
    {"HTTP_STATUS_MISDIRECTED_REQUEST", 421},
    {"HTTP_STATUS_UNPROCESSABLE_ENTITY", 422},
    {"HTTP_STATUS_LOCKED", 423},
    {"HTTP_STATUS_FAILED_DEPENDENCY", 424},
    {"HTTP_STATUS_TOO_EARLY", 425},
    {"HTTP_STATUS_UPGRADE_REQUIRED", 426},
    {"HTTP_STATUS_PRECONDITION_REQUIRED", 428},
    {"HTTP_STATUS_TOO_MANY_REQUESTS", 429},
    {"HTTP_STATUS_REQUEST_HEADER_FIELDS_TOO_LARGE", 431},
    {"HTTP_STATUS_UNAVAILABLE_FOR_LEGAL_REASONS", 451},
    {"HTTP_STATUS_INTERNAL_SERVER_ERROR", 500},
    {"HTTP_STATUS_NOT_IMPLEMENTED", 501},
    {"HTTP_STATUS_BAD_GATEWAY", 502},
    {"HTTP_STATUS_SERVICE_UNAVAILABLE", 503},
    {"HTTP_STATUS_GATEWAY_TIMEOUT", 504},
    {"HTTP_STATUS_HTTP_VERSION_NOT_SUPPORTED", 505},
    {"HTTP_STATUS_VARIANT_ALSO_NEGOTIATES", 506},
    {"HTTP_STATUS_INSUFFICIENT_STORAGE", 507},
    {"HTTP_STATUS_LOOP_DETECTED", 508},
    {"HTTP_STATUS_BANDWIDTH_LIMIT_EXCEEDED", 509},
    {"HTTP_STATUS_NOT_EXTENDED", 510},
    {"HTTP_STATUS_NETWORK_AUTHENTICATION_REQUIRED", 511},
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

Http2BindingState& EnsureHttp2State(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<Http2BindingState>(
      env, kEdgeEnvironmentSlotHttp2BindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool SetNamedInt64(napi_env env, napi_value obj, const char* name, int64_t value) {
  napi_value num = nullptr;
  return napi_create_int64(env, value, &num) == napi_ok &&
         num != nullptr &&
         napi_set_named_property(env, obj, name, num) == napi_ok;
}

bool SetNamedUint32(napi_env env, napi_value obj, const char* name, uint32_t value) {
  napi_value num = nullptr;
  return napi_create_uint32(env, value, &num) == napi_ok &&
         num != nullptr &&
         napi_set_named_property(env, obj, name, num) == napi_ok;
}

bool SetNamedString(napi_env env, napi_value obj, const char* name, const char* value) {
  napi_value str = nullptr;
  return napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &str) == napi_ok &&
         str != nullptr &&
         napi_set_named_property(env, obj, name, str) == napi_ok;
}

bool DefineMethod(napi_env env, napi_value target, const char* name, napi_callback cb, void* data = nullptr) {
  napi_property_descriptor desc{
      name,
      nullptr,
      cb,
      nullptr,
      nullptr,
      nullptr,
      napi_default_method,
      data,
  };
  return napi_define_properties(env, target, 1, &desc) == napi_ok;
}

template <typename T>
bool CreateTypedArray(napi_env env,
                      napi_typedarray_type type,
                      size_t length,
                      T** data_out,
                      napi_value* out) {
  if (data_out == nullptr || out == nullptr) return false;
  *data_out = nullptr;
  *out = nullptr;

  void* raw = nullptr;
  napi_value arraybuffer = nullptr;
  const size_t byte_length = length * sizeof(T);
  if (napi_create_arraybuffer(env, byte_length, &raw, &arraybuffer) != napi_ok ||
      arraybuffer == nullptr ||
      raw == nullptr) {
    return false;
  }
  std::memset(raw, 0, byte_length);
  if (napi_create_typedarray(env, type, length, arraybuffer, 0, out) != napi_ok || *out == nullptr) {
    return false;
  }
  *data_out = static_cast<T*>(raw);
  return true;
}

void SetStreamWriteState(napi_env env, size_t bytes_written, bool async) {
  int32_t* stream_state = EdgeGetStreamBaseState(env);
  if (stream_state == nullptr) return;
  stream_state[kEdgeBytesWritten] = static_cast<int32_t>(std::min<size_t>(bytes_written, INT32_MAX));
  stream_state[kEdgeLastWriteWasAsync] = async ? 1 : 0;
}

uint32_t TranslateNghttp2ErrorCode(int lib_error_code) {
  switch (lib_error_code) {
    case NGHTTP2_ERR_STREAM_CLOSED:
      return NGHTTP2_STREAM_CLOSED;
    case NGHTTP2_ERR_HEADER_COMP:
      return NGHTTP2_COMPRESSION_ERROR;
    case NGHTTP2_ERR_FRAME_SIZE_ERROR:
      return NGHTTP2_FRAME_SIZE_ERROR;
    case NGHTTP2_ERR_FLOW_CONTROL:
      return NGHTTP2_FLOW_CONTROL_ERROR;
    case NGHTTP2_ERR_REFUSED_STREAM:
      return NGHTTP2_REFUSED_STREAM;
    case NGHTTP2_ERR_PROTO:
    case NGHTTP2_ERR_HTTP_HEADER:
    case NGHTTP2_ERR_HTTP_MESSAGING:
      return NGHTTP2_PROTOCOL_ERROR;
    default:
      return NGHTTP2_INTERNAL_ERROR;
  }
}

int32_t GetFrameId(const nghttp2_frame* frame) {
  return frame != nullptr && frame->hd.type == NGHTTP2_PUSH_PROMISE ? frame->push_promise.promised_stream_id
                                                                    : (frame != nullptr ? frame->hd.stream_id : 0);
}

size_t TypedArrayElementSize(napi_typedarray_type type) {
  switch (type) {
    case napi_int8_array:
    case napi_uint8_array:
    case napi_uint8_clamped_array:
      return 1;
    case napi_int16_array:
    case napi_uint16_array:
    case napi_float16_array:
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

bool GetByteSpan(napi_env env, napi_value value, const uint8_t** data, size_t* len, std::string* temp_utf8 = nullptr) {
  if (data == nullptr || len == nullptr) return false;
  *data = nullptr;
  *len = 0;
  if (value == nullptr) return false;

  bool is_buffer = false;
  if (napi_is_buffer(env, value, &is_buffer) == napi_ok && is_buffer) {
    void* raw = nullptr;
    if (napi_get_buffer_info(env, value, &raw, len) != napi_ok || raw == nullptr) return false;
    *data = static_cast<const uint8_t*>(raw);
    return true;
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    void* raw = nullptr;
    napi_value arraybuffer = nullptr;
    size_t offset = 0;
    size_t element_length = 0;
    if (napi_get_typedarray_info(env, value, &type, &element_length, &raw, &arraybuffer, &offset) != napi_ok ||
        raw == nullptr) {
      return false;
    }
    *data = static_cast<const uint8_t*>(raw);
    *len = element_length * TypedArrayElementSize(type);
    return true;
  }

  bool is_arraybuffer = false;
  if (napi_is_arraybuffer(env, value, &is_arraybuffer) == napi_ok && is_arraybuffer) {
    void* raw = nullptr;
    size_t byte_length = 0;
    if (napi_get_arraybuffer_info(env, value, &raw, &byte_length) != napi_ok || raw == nullptr) {
      return false;
    }
    *data = static_cast<const uint8_t*>(raw);
    *len = byte_length;
    return true;
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) == napi_ok && type == napi_string && temp_utf8 != nullptr) {
    size_t str_len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &str_len) != napi_ok) return false;
    temp_utf8->assign(str_len + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, temp_utf8->data(), temp_utf8->size(), &copied) != napi_ok) {
      temp_utf8->clear();
      return false;
    }
    temp_utf8->resize(copied);
    *data = reinterpret_cast<const uint8_t*>(temp_utf8->data());
    *len = temp_utf8->size();
    return true;
  }

  return false;
}

bool GetAsciiStringPreserveNulls(napi_env env, napi_value value, std::string* out) {
  if (out == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return false;
  size_t length = 0;
  if (napi_get_value_string_utf16(env, value, nullptr, 0, &length) != napi_ok) return false;
  std::u16string wide(length + 1, u'\0');
  size_t copied = 0;
  if (napi_get_value_string_utf16(
          env, value, reinterpret_cast<char16_t*>(wide.data()), wide.size(), &copied) != napi_ok) {
    return false;
  }
  wide.resize(copied);
  out->clear();
  out->reserve(wide.size());
  for (char16_t ch : wide) {
    if (ch > 0x7f) return false;
    out->push_back(static_cast<char>(ch));
  }
  return true;
}

bool IsStandardSettingId(int32_t setting_id) {
  switch (setting_id) {
    case NGHTTP2_SETTINGS_HEADER_TABLE_SIZE:
    case NGHTTP2_SETTINGS_ENABLE_PUSH:
    case NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS:
    case NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE:
    case NGHTTP2_SETTINGS_MAX_FRAME_SIZE:
    case NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE:
    case NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL:
      return true;
    default:
      return false;
  }
}

void FetchAllowedRemoteCustomSettings(Http2SessionWrap* session, Http2BindingState* state) {
  if (session == nullptr || state == nullptr || state->settings_buffer == nullptr) return;
  session->remote_custom_settings.number = 0;
  const uint32_t additional_count =
      std::min<uint32_t>(state->settings_buffer[kSettingsFlags + 1], kMaxAdditionalSettings);
  const size_t offset = kSettingsFlags + 2;
  for (uint32_t i = 0; i < additional_count; ++i) {
    const uint32_t id = state->settings_buffer[offset + (i * 2)];
    if (IsStandardSettingId(static_cast<int32_t>(id))) continue;
    size_t index = session->remote_custom_settings.number++;
    session->remote_custom_settings.entries[index] =
        nghttp2_settings_entry{static_cast<int32_t>((id & 0xffffu) | (1u << 16)), 0};
  }
}

void UpdateLocalCustomSettings(Http2SessionWrap* session,
                               const std::vector<nghttp2_settings_entry>& entries) {
  if (session == nullptr) return;
  size_t number = session->local_custom_settings.number;
  for (const nghttp2_settings_entry& entry : entries) {
    if (IsStandardSettingId(entry.settings_id)) continue;
    size_t j = 0;
    while (j < number) {
      if (session->local_custom_settings.entries[j].settings_id == entry.settings_id) {
        session->local_custom_settings.entries[j].value = entry.value;
        break;
      }
      ++j;
    }
    if (j == number && number < kMaxAdditionalSettings) {
      session->local_custom_settings.entries[number++] = entry;
    }
  }
  session->local_custom_settings.number = number;
}

void FillSettingsBufferCustomEntries(Http2BindingState* state, const CustomSettingsState& custom_settings) {
  if (state == nullptr || state->settings_buffer == nullptr) return;
  const size_t offset = kSettingsFlags + 2;
  uint32_t count = 0;
  for (size_t i = 0; i < custom_settings.number && count < kMaxAdditionalSettings; ++i) {
    const nghttp2_settings_entry& entry = custom_settings.entries[i];
    if ((static_cast<uint32_t>(entry.settings_id) & ~0xffffu) != 0) continue;
    state->settings_buffer[offset + (count * 2)] = static_cast<uint32_t>(entry.settings_id & 0xffff);
    state->settings_buffer[offset + (count * 2) + 1] = entry.value;
    ++count;
  }
  state->settings_buffer[kSettingsFlags + 1] = count;
}

bool ParseSettingsPayload(Http2BindingState& state, std::vector<nghttp2_settings_entry>* out) {
  if (out == nullptr || state.settings_buffer == nullptr) return false;
  out->clear();

  const uint32_t flags = state.settings_buffer[kSettingsFlags];
  auto maybe_push = [&](uint32_t bit, int32_t id, uint32_t value) {
    if (flags & (1u << bit)) {
      out->push_back(nghttp2_settings_entry{id, value});
    }
  };

  maybe_push(kSettingsHeaderTableSize, NGHTTP2_SETTINGS_HEADER_TABLE_SIZE, state.settings_buffer[kSettingsHeaderTableSize]);
  maybe_push(kSettingsEnablePush, NGHTTP2_SETTINGS_ENABLE_PUSH, state.settings_buffer[kSettingsEnablePush]);
  maybe_push(kSettingsMaxConcurrentStreams,
             NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS,
             state.settings_buffer[kSettingsMaxConcurrentStreams]);
  maybe_push(kSettingsInitialWindowSize,
             NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE,
             state.settings_buffer[kSettingsInitialWindowSize]);
  maybe_push(kSettingsMaxFrameSize, NGHTTP2_SETTINGS_MAX_FRAME_SIZE, state.settings_buffer[kSettingsMaxFrameSize]);
  maybe_push(kSettingsMaxHeaderListSize,
             NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE,
             state.settings_buffer[kSettingsMaxHeaderListSize]);
  maybe_push(kSettingsEnableConnectProtocol,
             NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL,
             state.settings_buffer[kSettingsEnableConnectProtocol]);

  const uint32_t additional_count = std::min<uint32_t>(state.settings_buffer[kSettingsFlags + 1], kMaxAdditionalSettings);
  const size_t offset = kSettingsFlags + 2;
  for (uint32_t i = 0; i < additional_count; ++i) {
    const uint32_t id = state.settings_buffer[offset + (i * 2)];
    const uint32_t value = state.settings_buffer[offset + (i * 2) + 1];
    out->push_back(nghttp2_settings_entry{static_cast<int32_t>(id), value});
  }

  return true;
}

void FillDefaultSettingsBuffer(Http2BindingState& state) {
  if (state.settings_buffer == nullptr) return;
  uint32_t flags = 0;
  state.settings_buffer[kSettingsHeaderTableSize] = kDefaultSettingsHeaderTableSize;
  flags |= (1u << kSettingsHeaderTableSize);
  state.settings_buffer[kSettingsEnablePush] = kDefaultSettingsEnablePush;
  flags |= (1u << kSettingsEnablePush);
  state.settings_buffer[kSettingsInitialWindowSize] = kDefaultSettingsInitialWindowSize;
  flags |= (1u << kSettingsInitialWindowSize);
  state.settings_buffer[kSettingsMaxFrameSize] = kDefaultSettingsMaxFrameSize;
  flags |= (1u << kSettingsMaxFrameSize);
  state.settings_buffer[kSettingsMaxConcurrentStreams] = kDefaultSettingsMaxConcurrentStreams;
  flags |= (1u << kSettingsMaxConcurrentStreams);
  state.settings_buffer[kSettingsMaxHeaderListSize] = kDefaultSettingsMaxHeaderListSize;
  flags |= (1u << kSettingsMaxHeaderListSize);
  state.settings_buffer[kSettingsEnableConnectProtocol] = kDefaultSettingsEnableConnectProtocol;
  flags |= (1u << kSettingsEnableConnectProtocol);
  state.settings_buffer[kSettingsFlags] = flags;
  state.settings_buffer[kSettingsFlags + 1] = 0;
}

Http2BindingState* GetHttp2State(napi_env env) {
  return EdgeEnvironmentGetSlotData<Http2BindingState>(env, kEdgeEnvironmentSlotHttp2BindingState);
}

bool GetNamedValue(napi_env env, napi_value object, const char* name, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  return env != nullptr && object != nullptr && name != nullptr &&
         napi_get_named_property(env, object, name, out) == napi_ok && *out != nullptr;
}

bool SetNamedValue(napi_env env, napi_value object, const char* name, napi_value value) {
  return env != nullptr && object != nullptr && name != nullptr && value != nullptr &&
         napi_set_named_property(env, object, name, value) == napi_ok;
}

double GetNumberProperty(napi_env env, napi_value object, const char* name, double fallback = 0) {
  napi_value value = nullptr;
  if (!GetNamedValue(env, object, name, &value)) return fallback;
  double out = fallback;
  if (napi_get_value_double(env, value, &out) != napi_ok) return fallback;
  return out;
}

bool GetLatin1String(napi_env env, napi_value value, std::string* out) {
  if (out == nullptr || env == nullptr || value == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_latin1(env, value, nullptr, 0, &len) != napi_ok) return false;
  out->assign(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_latin1(env, value, out->data(), out->size(), &copied) != napi_ok) {
    out->clear();
    return false;
  }
  out->resize(copied);
  return true;
}

bool ParsePackedHeaders(napi_env env,
                        napi_value value,
                        std::vector<nghttp2_nv>* out,
                        std::vector<std::string>* storage) {
  if (out == nullptr || storage == nullptr || env == nullptr || value == nullptr) return false;
  out->clear();
  storage->clear();

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) != napi_ok || !is_array) return false;

  napi_value header_string = nullptr;
  napi_value header_count = nullptr;
  if (napi_get_element(env, value, 0, &header_string) != napi_ok ||
      napi_get_element(env, value, 1, &header_count) != napi_ok ||
      header_string == nullptr ||
      header_count == nullptr) {
    return false;
  }

  uint32_t count = 0;
  if (napi_get_value_uint32(env, header_count, &count) != napi_ok) return false;
  if (count == 0) return true;

  std::string packed;
  if (!GetLatin1String(env, header_string, &packed)) return false;

  storage->reserve(count * 2);
  out->reserve(count);

  char* ptr = packed.data();
  char* end = packed.data() + packed.size();
  for (uint32_t i = 0; i < count && ptr < end; ++i) {
    char* name_ptr = ptr;
    size_t name_len = std::strlen(name_ptr);
    ptr += name_len + 1;
    if (ptr > end) break;
    char* value_ptr = ptr;
    size_t value_len = std::strlen(value_ptr);
    ptr += value_len + 1;
    if (ptr > end) break;
    const uint8_t flags = ptr < end ? static_cast<uint8_t>(*ptr++) : 0;

    storage->emplace_back(name_ptr, name_len);
    storage->emplace_back(value_ptr, value_len);
    std::string& name = (*storage)[storage->size() - 2];
    std::string& header_value = (*storage)[storage->size() - 1];
    out->push_back(nghttp2_nv{
        reinterpret_cast<uint8_t*>(name.data()),
        reinterpret_cast<uint8_t*>(header_value.data()),
        name.size(),
        header_value.size(),
        flags,
    });
  }

  return out->size() == count;
}

bool CreateJsStringArray(napi_env env, const std::vector<std::string>& values, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (napi_create_array_with_length(env, values.size(), out) != napi_ok || *out == nullptr) return false;
  for (uint32_t i = 0; i < values.size(); ++i) {
    napi_value str = nullptr;
    if (napi_create_string_utf8(env, values[i].c_str(), values[i].size(), &str) != napi_ok || str == nullptr) {
      return false;
    }
    napi_set_element(env, *out, i, str);
  }
  return true;
}

bool SetReadState(napi_env env, int32_t nread, int32_t offset = 0) {
  int32_t* state = EdgeGetStreamBaseState(env);
  if (state == nullptr) return false;
  state[kEdgeReadBytesOrError] = nread;
  state[kEdgeArrayBufferOffset] = offset;
  return true;
}

bool CallCallbackRef(napi_env env,
                     napi_ref callback_ref,
                     int64_t async_id,
                     napi_value recv,
                     size_t argc,
                     napi_value* argv,
                     napi_value* result = nullptr) {
  if (env == nullptr) return false;
  if (result != nullptr) *result = nullptr;
  edge::EscapableHandleScope scope(env);
  if (!scope.is_open()) return false;
  napi_value callback = GetRefValue(env, callback_ref);
  if (!IsFunction(env, callback) || recv == nullptr) return false;
  napi_value local_result = nullptr;
  const bool ok = EdgeAsyncWrapMakeCallback(
                      env, async_id, recv, recv, callback, argc, argv, &local_result, kEdgeMakeCallbackNone) == napi_ok;
  if (ok && result != nullptr && local_result != nullptr) {
    *result = scope.Escape(local_result);
  }
  return ok;
}

bool CallCallbackRefWithResource(napi_env env,
                                 napi_ref callback_ref,
                                 int64_t async_id,
                                 napi_ref resource_ref,
                                 napi_value recv,
                                 size_t argc,
                                 napi_value* argv,
                                 napi_value* result = nullptr) {
  if (env == nullptr) return false;
  if (result != nullptr) *result = nullptr;
  edge::EscapableHandleScope scope(env);
  if (!scope.is_open()) return false;
  napi_value callback = GetRefValue(env, callback_ref);
  napi_value resource = GetRefValue(env, resource_ref);
  napi_value effective_recv = recv != nullptr ? recv : resource;
  if (!IsFunction(env, callback) || effective_recv == nullptr) return false;
  napi_value local_result = nullptr;
  const bool ok = EdgeAsyncWrapMakeCallback(env,
                                           async_id,
                                           resource != nullptr ? resource : effective_recv,
                                           effective_recv,
                                           callback,
                                           argc,
                                           argv,
                                           &local_result,
                                           kEdgeMakeCallbackNone) == napi_ok;
  if (ok && result != nullptr && local_result != nullptr) {
    *result = scope.Escape(local_result);
  }
  return ok;
}

bool CallNamedIntMethod(napi_env env,
                        napi_value recv,
                        const char* name,
                        size_t argc,
                        napi_value* argv,
                        int32_t* out) {
  if (out == nullptr) return false;
  *out = 0;
  if (env == nullptr) return false;
  edge::HandleScope scope(env);
  if (!scope.is_open()) return false;
  napi_value fn = nullptr;
  napi_value result = nullptr;
  return GetNamedValue(env, recv, name, &fn) &&
         IsFunction(env, fn) &&
         napi_call_function(env, recv, fn, argc, argv, &result) == napi_ok &&
         result != nullptr &&
         napi_get_value_int32(env, result, out) == napi_ok;
}

Http2SessionWrap* UnwrapSession(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  Http2SessionWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

Http2StreamWrap* UnwrapStream(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  Http2StreamWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

void ClearPendingParentRead(Http2SessionWrap* session);
void AppendPendingParentRead(Http2SessionWrap* session, const uint8_t* data, size_t len);
void ConsumeHTTP2Data(Http2SessionWrap* session);
bool SessionHasPendingDataNow(Http2SessionWrap* session);
void MaybeNotifyGracefulCloseComplete(Http2SessionWrap* session);
void ScheduleDeferredStreamDestroy(Http2StreamWrap* stream);
void DestroyStreamHandle(Http2StreamWrap* stream);

void DeletePendingSettings(napi_env env, PendingSettings* pending) {
  if (pending == nullptr) return;
  DeleteRefIfPresent(env, &pending->callback_ref);
  DeleteRefIfPresent(env, &pending->resource_ref);
  pending->async_id = -1;
  pending->start_time = 0;
}

void DeletePendingPing(napi_env env, PendingPing* pending) {
  if (pending == nullptr) return;
  DeleteRefIfPresent(env, &pending->callback_ref);
  DeleteRefIfPresent(env, &pending->resource_ref);
  pending->async_id = -1;
  pending->start_time = 0;
  pending->payload.fill(0);
}

bool InitPendingAsyncResource(napi_env env,
                              const char* type,
                              napi_ref* resource_ref,
                              int64_t* async_id) {
  if (env == nullptr || type == nullptr || resource_ref == nullptr || async_id == nullptr) return false;
  *resource_ref = nullptr;
  *async_id = -1;
  napi_value resource = nullptr;
  if (napi_create_object(env, &resource) != napi_ok || resource == nullptr) return false;
  if (napi_create_reference(env, resource, 1, resource_ref) != napi_ok || *resource_ref == nullptr) {
    DeleteRefIfPresent(env, resource_ref);
    return false;
  }
  *async_id = EdgeAsyncWrapNextId(env);
  const int64_t trigger_async_id = EdgeAsyncWrapExecutionAsyncId(env);
  EdgeAsyncWrapEmitInitString(env, *async_id, type, trigger_async_id, resource);
  return true;
}

double DurationMillisSince(uint64_t start_time) {
  if (start_time == 0) return 0;
  const uint64_t now = uv_hrtime();
  return now > start_time ? static_cast<double>(now - start_time) / 1e6 : 0;
}

void CompletePendingSettings(napi_env env, PendingSettings* pending, bool ack) {
  if (env == nullptr || pending == nullptr) return;
  napi_value resource = GetRefValue(env, pending->resource_ref);
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_boolean(env, ack, &argv[0]);
  napi_create_double(env, ack ? DurationMillisSince(pending->start_time) : 0, &argv[1]);
  (void)CallCallbackRefWithResource(env,
                                    pending->callback_ref,
                                    pending->async_id,
                                    pending->resource_ref,
                                    resource,
                                    2,
                                    argv);
  if (pending->async_id > 0) EdgeAsyncWrapQueueDestroyId(env, pending->async_id);
  DeletePendingSettings(env, pending);
}

void CompletePendingPing(napi_env env, PendingPing* pending, bool ack, const uint8_t* payload = nullptr) {
  if (env == nullptr || pending == nullptr) return;
  napi_value resource = GetRefValue(env, pending->resource_ref);
  napi_value buffer = nullptr;
  if (ack && payload != nullptr) {
    void* copy = nullptr;
    napi_create_buffer_copy(env, 8, payload, &copy, &buffer);
  } else {
    buffer = Undefined(env);
  }
  napi_value argv[3] = {nullptr, nullptr, buffer};
  napi_get_boolean(env, ack, &argv[0]);
  napi_create_double(env, ack ? DurationMillisSince(pending->start_time) : 0, &argv[1]);
  (void)CallCallbackRefWithResource(env,
                                    pending->callback_ref,
                                    pending->async_id,
                                    pending->resource_ref,
                                    resource,
                                    3,
                                    argv);
  if (pending->async_id > 0) EdgeAsyncWrapQueueDestroyId(env, pending->async_id);
  DeletePendingPing(env, pending);
}

void Http2SessionFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<Http2SessionWrap*>(data);
  if (wrap == nullptr) return;
  for (auto& entry : wrap->streams) {
    if (entry.second != nullptr) {
      entry.second->session = nullptr;
      entry.second->session_ref_held = false;
    }
  }
  wrap->streams.clear();
  for (auto& pending : wrap->pending_settings) DeletePendingSettings(env, &pending);
  for (auto& pending : wrap->pending_pings) DeletePendingPing(env, &pending);
  ClearPendingParentRead(wrap);
  // Remove via parent_stream_listener_base, not parent_stream_base: several
  // paths null parent_stream_base (its "native parent usable" meaning) without
  // unregistering the listener, so it can be null here while the listener is
  // still registered. parent_handle_ref keeps the parent base alive until just
  // below, so the removal is safe. (A no-op if an earlier path already removed
  // it.)
  if (wrap->parent_stream_listener_base != nullptr) {
    (void)EdgeStreamBaseRemoveListener(wrap->parent_stream_listener_base, &wrap->parent_stream_listener);
    wrap->parent_stream_listener_base = nullptr;
  }
  wrap->parent_stream_base = nullptr;
  wrap->parent_stream_listener.env = nullptr;
  wrap->parent_stream_listener.on_alloc = nullptr;
  wrap->parent_stream_listener.on_read = nullptr;
  wrap->parent_stream_listener.on_after_write = nullptr;
  wrap->parent_stream_listener.on_after_shutdown = nullptr;
  wrap->parent_stream_listener.on_close = nullptr;
  wrap->parent_stream_listener.data = nullptr;
  DeleteRefIfPresent(env, &wrap->parent_handle_ref);
  DeleteRefIfPresent(env, &wrap->fields_ref);
  DeleteRefIfPresent(env, &wrap->wrapper_ref);
  if (wrap->session != nullptr) nghttp2_session_del(wrap->session);
  if (wrap->callbacks != nullptr) nghttp2_session_callbacks_del(wrap->callbacks);
  delete wrap;
}

void Http2StreamFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<Http2StreamWrap*>(data);
  if (wrap == nullptr) return;
  EdgeStreamBaseFinalize(&wrap->base);
  for (auto& chunk : wrap->outbound_chunks) DeleteRefIfPresent(env, &chunk.req_ref);
  DeleteRefIfPresent(env, &wrap->shutdown_req_ref);
  DeleteRefIfPresent(env, &wrap->active_ref);
  DeleteRefIfPresent(env, &wrap->onread_ref);
  delete wrap;
}

void CallNamedFunctionIfPresent(napi_env env, napi_value recv, const char* name, size_t argc = 0, napi_value* argv = nullptr) {
  if (env == nullptr || recv == nullptr || name == nullptr) return;
  napi_value fn = nullptr;
  if (napi_get_named_property(env, recv, name, &fn) != napi_ok || !IsFunction(env, fn)) return;
  napi_value ignored = nullptr;
  (void)napi_call_function(env, recv, fn, argc, argv, &ignored);
}

void MaybeInvokeGracefulClose(Http2SessionWrap* session) {
  if (session == nullptr || !session->wants_graceful_close || !session->streams.empty()) return;
  MaybeNotifyGracefulCloseComplete(session);
}

int FlushSessionOutput(Http2SessionWrap* session, napi_value req_obj = nullptr);
void MaybeScheduleSessionFlush(Http2SessionWrap* session);
void ScheduleSessionFlush(Http2SessionWrap* session);

bool SessionHasPendingDataNow(Http2SessionWrap* session) {
  if (session == nullptr || session->session == nullptr) return false;
  if (session->goaway_pending || session->write_scheduled || session->parent_write_in_progress ||
      !session->pending_parent_write_chunks.empty()) {
    return true;
  }
  const int want_write = nghttp2_session_want_write(session->session);
  const int want_read = nghttp2_session_want_read(session->session);
  return want_write != 0 || want_read != 0;
}

void MaybeNotifyGracefulCloseComplete(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr || !session->wants_graceful_close ||
      session->graceful_close_notified || SessionHasPendingDataNow(session)) {
    return;
  }
  session->graceful_close_notified = true;
  napi_value self = GetRefValue(session->env, session->wrapper_ref);
  if (self != nullptr) CallNamedFunctionIfPresent(session->env, self, "ongracefulclosecomplete");
}

void CompleteReqRef(napi_env env, napi_ref* ref, int status) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_value req = GetRefValue(env, *ref);
  DeleteRefIfPresent(env, ref);
  if (req != nullptr) EdgeStreamBaseInvokeReqOnComplete(env, req, status, nullptr, 0);
}

void CompleteStreamReq(Http2StreamWrap* stream, napi_ref* ref, int status, bool shutdown = false) {
  if (stream == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_value req = GetRefValue(stream->env, *ref);
  DeleteRefIfPresent(stream->env, ref);
  if (req == nullptr) return;
  if (shutdown) {
    EdgeStreamBaseEmitAfterShutdown(&stream->base, req, status);
  } else {
    EdgeStreamBaseEmitAfterWrite(&stream->base, req, status);
  }
}

void RemoveStreamFromSession(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->session == nullptr) return;
  Http2SessionWrap* session = stream->session;
  session->streams.erase(stream->id);
  MaybeInvokeGracefulClose(session);
  if (stream->session_ref_held && session->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    (void)napi_reference_unref(session->env, session->wrapper_ref, &ignored);
  }
  stream->session_ref_held = false;
  stream->session = nullptr;
  DeleteRefIfPresent(stream->env, &stream->active_ref);
}

bool HasPendingRstStream(Http2SessionWrap* session, int32_t stream_id) {
  if (session == nullptr) return false;
  return std::find(session->pending_rst_stream_ids.begin(),
                   session->pending_rst_stream_ids.end(),
                   stream_id) != session->pending_rst_stream_ids.end();
}

void AddPendingRstStream(Http2SessionWrap* session, int32_t stream_id) {
  if (session == nullptr || stream_id <= 0) return;
  auto it = session->streams.find(stream_id);
  if (it != session->streams.end() && it->second != nullptr) {
    session->pending_rst_codes[stream_id] = it->second->rst_code;
  } else if (session->pending_rst_codes.find(stream_id) == session->pending_rst_codes.end()) {
    session->pending_rst_codes.emplace(stream_id, NGHTTP2_NO_ERROR);
  }
  DebugSession(session,
               "queue pending rst stream=%d code=%u already_pending=%s",
               stream_id,
               session->pending_rst_codes[stream_id],
               HasPendingRstStream(session, stream_id) ? "true" : "false");
  if (HasPendingRstStream(session, stream_id)) return;
  session->pending_rst_stream_ids.push_back(stream_id);
}

void FlushPendingRstStreams(Http2SessionWrap* session) {
  if (session == nullptr || session->session == nullptr || session->destroyed || session->parent_write_in_progress ||
      session->pending_rst_stream_ids.empty()) {
    return;
  }

  std::vector<int32_t> pending;
  pending.swap(session->pending_rst_stream_ids);
  for (int32_t stream_id : pending) {
    uint32_t rst_code = NGHTTP2_NO_ERROR;
    auto code_it = session->pending_rst_codes.find(stream_id);
    if (code_it != session->pending_rst_codes.end()) {
      rst_code = code_it->second;
      session->pending_rst_codes.erase(code_it);
    } else {
      auto stream_it = session->streams.find(stream_id);
      if (stream_it != session->streams.end() && stream_it->second != nullptr) {
        rst_code = stream_it->second->rst_code;
      }
    }
    const int rc = nghttp2_submit_rst_stream(session->session, NGHTTP2_FLAG_NONE, stream_id, rst_code);
    DebugSession(session, "flush pending rst stream=%d code=%u rc=%d", stream_id, rst_code, rc);
  }
  MaybeScheduleSessionFlush(session);
}

bool CreateStreamHandle(Http2SessionWrap* session,
                        int32_t id,
                        int32_t category,
                        napi_value* object_out,
                        Http2StreamWrap** wrap_out) {
  if (object_out == nullptr || wrap_out == nullptr || session == nullptr) return false;
  *object_out = nullptr;
  *wrap_out = nullptr;
  Http2BindingState* state = GetHttp2State(session->env);
  napi_value ctor = state != nullptr ? GetRefValue(session->env, state->stream_ctor_ref) : nullptr;
  if (ctor == nullptr) return false;
  napi_value argv[1] = {nullptr};
  napi_create_int32(session->env, id, &argv[0]);
  if (napi_new_instance(session->env, ctor, 1, argv, object_out) != napi_ok || *object_out == nullptr) {
    return false;
  }
  Http2StreamWrap* wrap = UnwrapStream(session->env, *object_out);
  if (wrap == nullptr) return false;
  wrap->session = session;
  wrap->id = id;
  wrap->current_category = category;
  wrap->max_header_pairs =
      session->max_header_pairs == 0 ? kDefaultMaxHeaderListPairs : session->max_header_pairs;
  uint32_t max_header_length = kDefaultSettingsMaxHeaderListSize;
  if (session->session != nullptr) {
    max_header_length =
        nghttp2_session_get_local_settings(session->session, NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE);
  }
  wrap->max_header_length = std::min<size_t>(max_header_length, kMaxMaxHeaderListSize);
  napi_create_reference(session->env, *object_out, 1, &wrap->active_ref);
  if (session->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    if (napi_reference_ref(session->env, session->wrapper_ref, &ignored) == napi_ok) {
      wrap->session_ref_held = true;
    }
  }
  session->streams[id] = wrap;
  *wrap_out = wrap;
  return true;
}

void EmitInternalError(Http2SessionWrap* session, int32_t code, const char* custom_error = nullptr) {
  if (session == nullptr || session->env == nullptr) return;
  if (custom_error != nullptr) {
    DebugSession(session, "internal error %d (%s)", code, custom_error);
  } else {
    DebugSession(session, "internal error %d", code);
  }
  Http2BindingState* state = GetHttp2State(session->env);
  napi_value self = GetRefValue(session->env, session->wrapper_ref);
  if (state == nullptr || self == nullptr) return;
  napi_value argv[2] = {nullptr, nullptr};
  napi_create_int32(session->env, code, &argv[0]);
  if (custom_error != nullptr) {
    napi_create_string_utf8(session->env, custom_error, NAPI_AUTO_LENGTH, &argv[1]);
  } else {
    argv[1] = Undefined(session->env);
  }
  (void)CallCallbackRef(session->env, state->callback_refs[kCallbackError], session->async_id, self, 2, argv);
}

int WriteBufferToParent(Http2SessionWrap* session,
                        napi_value req_obj,
                        const uint8_t* data,
                        size_t len,
                        bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (session == nullptr || session->env == nullptr || data == nullptr || len == 0) return 0;
  if (session->parent_closed) return UV_EPIPE;
  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return 0;
  napi_value buffer = nullptr;
  void* copy = nullptr;
  if (napi_create_buffer_copy(session->env, len, data, &copy, &buffer) != napi_ok || buffer == nullptr) {
    return UV_ENOMEM;
  }
  if (session->parent_stream_base != nullptr) {
    return EdgeStreamBaseWriteBufferDirect(session->parent_stream_base, req_obj, buffer, async_out);
  }
  napi_value argv[2] = {req_obj, buffer};
  int32_t rc = 0;
  if (!CallNamedIntMethod(session->env, parent, "writeBuffer", 2, argv, &rc)) {
    return UV_EPIPE;
  }
  if (async_out != nullptr && rc == 0) {
    int32_t* stream_state = EdgeGetStreamBaseState(session->env);
    *async_out = stream_state != nullptr && stream_state[kEdgeLastWriteWasAsync] != 0;
  }
  return rc;
}

int WriteChunksToParent(Http2SessionWrap* session, napi_value req_obj, bool* async_out) {
  if (async_out != nullptr) *async_out = false;
  if (session == nullptr || session->env == nullptr || session->pending_parent_write_chunks.empty()) return 0;
  if (session->parent_closed) return UV_EPIPE;

  if (session->pending_parent_write_chunks.size() == 1) {
    const std::vector<uint8_t>& chunk = session->pending_parent_write_chunks.front();
    return WriteBufferToParent(session, req_obj, chunk.data(), chunk.size(), async_out);
  }

  napi_value chunks = nullptr;
  if (napi_create_array_with_length(session->env, session->pending_parent_write_chunks.size(), &chunks) != napi_ok ||
      chunks == nullptr) {
    return UV_ENOMEM;
  }

  uint32_t index = 0;
  for (const std::vector<uint8_t>& chunk : session->pending_parent_write_chunks) {
    napi_value buffer = nullptr;
    void* copy = nullptr;
    if (napi_create_buffer_copy(session->env, chunk.size(), chunk.data(), &copy, &buffer) != napi_ok ||
        buffer == nullptr ||
        napi_set_element(session->env, chunks, index++, buffer) != napi_ok) {
      return UV_ENOMEM;
    }
  }

  if (session->parent_stream_base != nullptr) {
    return EdgeStreamBaseWritevDirect(session->parent_stream_base, req_obj, chunks, async_out);
  }

  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return UV_EPIPE;
  napi_value writev = nullptr;
  if (napi_get_named_property(session->env, parent, "writev", &writev) != napi_ok ||
      !IsFunction(session->env, writev)) {
    return UV_EPIPE;
  }
  napi_value argv[3] = {req_obj, chunks, EdgeStreamBaseMakeBool(session->env, true)};
  napi_value result = nullptr;
  if (napi_call_function(session->env, parent, writev, 3, argv, &result) != napi_ok || result == nullptr) {
    return UV_EPROTO;
  }
  int32_t rc = UV_EPROTO;
  if (napi_get_value_int32(session->env, result, &rc) != napi_ok) return UV_EPROTO;
  if (async_out != nullptr && rc == 0) {
    int32_t* stream_state = EdgeGetStreamBaseState(session->env);
    *async_out = stream_state != nullptr && stream_state[kEdgeLastWriteWasAsync] != 0;
  }
  return rc;
}

bool HasJsOnRead(Http2StreamWrap* stream) {
  if (stream == nullptr) return false;
  napi_value self = GetRefValue(stream->env, stream->wrapper_ref);
  napi_value callback = GetRefValue(stream->env, stream->onread_ref);
  return self != nullptr && IsFunction(stream->env, callback);
}

bool HasActiveNativeReadListener(Http2StreamWrap* stream) {
  if (stream == nullptr) return false;
  return stream->base.listener_state.current != nullptr &&
         stream->base.listener_state.current != &stream->base.default_listener;
}

enum class InboundDelivery {
  kQueued,
  kDeliveredPaused,
  kDeliveredActive,
};

void EmitInboundEOF(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->env == nullptr) return;
  DebugStream(stream, "emitting inbound EOF (canEmit=%s, queued=%zu)",
              (HasJsOnRead(stream) || HasActiveNativeReadListener(stream)) ? "true" : "false",
              stream->inbound_chunks.size());
  if (EdgeStreamBaseEmitEOF(&stream->base)) {
    stream->inbound_eof = false;
    return;
  }
  if (!HasJsOnRead(stream)) {
    stream->inbound_eof = true;
    return;
  }

  napi_value self = GetRefValue(stream->env, stream->wrapper_ref);
  napi_value callback = GetRefValue(stream->env, stream->onread_ref);
  if (self == nullptr || !IsFunction(stream->env, callback)) {
    stream->inbound_eof = true;
    return;
  }

  stream->inbound_eof = false;
  SetReadState(stream->env, UV_EOF);
  napi_value argv[1] = {Undefined(stream->env)};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      stream->env, stream->async_id, self, self, callback, 1, argv, &ignored, kEdgeMakeCallbackNone);
  SetReadState(stream->env, 0);
}

void FlushQueuedInboundConsumption(Http2StreamWrap* stream);
int FlushSessionOutput(Http2SessionWrap* session, napi_value req_obj);
void ScheduleSessionFlush(Http2SessionWrap* session);
void MaybeScheduleSessionFlush(Http2SessionWrap* session);
void ConsumeHTTP2Data(Http2SessionWrap* session);
bool HasPendingRstStream(Http2SessionWrap* session, int32_t stream_id);
void AddPendingRstStream(Http2SessionWrap* session, int32_t stream_id);
void FlushPendingRstStreams(Http2SessionWrap* session);

void ClearPendingParentRead(Http2SessionWrap* session) {
  if (session == nullptr) return;
  session->pending_parent_read_bytes.clear();
  session->pending_parent_read_offset = 0;
  session->receive_paused = false;
}

void AppendPendingParentRead(Http2SessionWrap* session, const uint8_t* data, size_t len) {
  if (session == nullptr || data == nullptr || len == 0) return;
  if (session->pending_parent_read_offset == 0 && session->pending_parent_read_bytes.empty()) {
    session->pending_parent_read_bytes.assign(data, data + len);
    return;
  }

  const size_t pending =
      session->pending_parent_read_offset <= session->pending_parent_read_bytes.size()
          ? session->pending_parent_read_bytes.size() - session->pending_parent_read_offset
          : 0;
  std::vector<uint8_t> merged;
  merged.reserve(pending + len);
  if (pending > 0) {
    merged.insert(merged.end(),
                  session->pending_parent_read_bytes.begin() + session->pending_parent_read_offset,
                  session->pending_parent_read_bytes.end());
  }
  merged.insert(merged.end(), data, data + len);
  session->pending_parent_read_bytes.swap(merged);
  session->pending_parent_read_offset = 0;
}

bool ParentStreamOnAlloc(EdgeStreamListener* listener, size_t suggested_size, uv_buf_t* out) {
  if (listener == nullptr || out == nullptr) return false;
  char* buffer = static_cast<char*>(malloc(suggested_size));
  if (buffer == nullptr && suggested_size > 0) return false;
  *out = uv_buf_init(buffer, static_cast<unsigned int>(suggested_size));
  return true;
}

void QueueOutboundChunk(Http2StreamWrap* stream, napi_value req_obj, const uint8_t* data, size_t len) {
  if (stream == nullptr) return;
  OutboundChunk chunk;
  if (data != nullptr && len > 0) chunk.data.assign(data, data + len);
  if (req_obj != nullptr) (void)napi_create_reference(stream->env, req_obj, 1, &chunk.req_ref);
  stream->outbound_chunks.push_back(std::move(chunk));
  stream->available_outbound_length += len;
}

void CallStreamTrailersCallback(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->trailers_notified) return;
  Http2BindingState* state = GetHttp2State(stream->env);
  napi_value self = GetRefValue(stream->env, stream->wrapper_ref);
  if (state == nullptr || self == nullptr) return;
  stream->has_trailers = false;
  stream->trailers_notified = true;
  (void)CallCallbackRef(
      stream->env, state->callback_refs[kCallbackStreamTrailers], stream->async_id, self, 0, nullptr);
}

void CopyDataIntoPendingWrite(Http2SessionWrap* session, const uint8_t* data, size_t len) {
  if (session == nullptr || data == nullptr || len == 0) return;
  session->pending_parent_write_chunks.emplace_back(data, data + len);
  session->pending_parent_write_length += len;
}

void ReleaseParentReadBuffer(const uv_buf_t* buf) {
  if (buf == nullptr || buf->base == nullptr) return;
  free(buf->base);
}

void ClearPendingParentWrite(Http2SessionWrap* session, int status) {
  (void)status;
  if (session == nullptr) return;
  session->pending_parent_write_chunks.clear();
  session->pending_parent_write_length = 0;
  while (!session->pending_parent_write_req_refs.empty()) {
    napi_ref req_ref = session->pending_parent_write_req_refs.front();
    session->pending_parent_write_req_refs.pop_front();
    // Match Node's Http2Session::ClearOutgoing(): once nghttp2 has serialized
    // stream data to the parent socket write, the stream write itself is done.
    CompleteReqRef(session->env, &req_ref, 0);
  }
}

bool IsParentReading(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr) return false;
  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return false;
  napi_value reading_value = nullptr;
  bool reading = false;
  if (napi_get_named_property(session->env, parent, "reading", &reading_value) != napi_ok || reading_value == nullptr) {
    return false;
  }
  if (napi_get_value_bool(session->env, reading_value, &reading) != napi_ok) return false;
  return reading;
}

void NotifySessionDone(Http2SessionWrap* session, bool resume_parent_reading) {
  if (session == nullptr || session->done_notified || session->env == nullptr) return;
  session->done_notified = true;
  EmitHttp2SessionPerformance(session);
  napi_value self = GetRefValue(session->env, session->wrapper_ref);
  if (self != nullptr) CallNamedFunctionIfPresent(session->env, self, "ondone");
  if (session->destroyed && session->parent_stream_base != nullptr) {
    (void)EdgeStreamBaseRemoveListener(session->parent_stream_base, &session->parent_stream_listener);
    session->parent_stream_base = nullptr;
  }
  if (!resume_parent_reading || session->destroyed == false) return;
  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return;
  int32_t rc = 0;
  if (CallNamedIntMethod(session->env, parent, "readStart", 0, nullptr, &rc) &&
      (rc == 0 || rc == UV_EALREADY)) {
    session->parent_reading_stopped = false;
  }
}

void MaybeStopParentReading(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr || session->parent_reading_stopped || session->destroyed ||
      session->session == nullptr) {
    return;
  }
  if (session->wants_graceful_close) {
    return;
  }
  const int want_read = nghttp2_session_want_read(session->session);
  if (!session->parent_write_in_progress && want_read != 0) {
    return;
  }
  if (!session->parent_write_in_progress && want_read == 0 && session->streams.empty()) {
    return;
  }
  if (!IsParentReading(session)) {
    session->parent_reading_stopped = true;
    return;
  }
  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return;
  int32_t rc = 0;
  if (CallNamedIntMethod(session->env, parent, "readStop", 0, nullptr, &rc) && rc == 0) {
    session->parent_reading_stopped = true;
  }
}

void MaybeResumeParentReading(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr || !session->parent_reading_stopped || session->destroyed ||
      session->session == nullptr || session->parent_write_in_progress) {
    return;
  }
  if (nghttp2_session_want_read(session->session) == 0) return;
  if (IsParentReading(session)) {
    session->parent_reading_stopped = false;
    return;
  }
  napi_value parent = GetRefValue(session->env, session->parent_handle_ref);
  if (parent == nullptr) return;
  int32_t rc = 0;
  if (CallNamedIntMethod(session->env, parent, "readStart", 0, nullptr, &rc) &&
      (rc == 0 || rc == UV_EALREADY)) {
    session->parent_reading_stopped = false;
  }
}

void RunDeferredSessionFlushTask(napi_env env, void* data) {
  auto* task = static_cast<DeferredSessionFlushTask*>(data);
  Http2SessionWrap* session = task != nullptr ? task->session : nullptr;
  if (session != nullptr && session->env == env && !session->destroyed &&
      !session->parent_write_in_progress && session->write_scheduled) {
    session->write_scheduled = false;
    (void)FlushSessionOutput(session);
  }
  if (task != nullptr && task->wrapper_refed && session != nullptr && session->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    (void)napi_reference_unref(env, session->wrapper_ref, &ignored);
    task->wrapper_refed = false;
  }
}

void DeleteDeferredSessionFlushTask(napi_env env, void* data) {
  auto* task = static_cast<DeferredSessionFlushTask*>(data);
  if (task == nullptr) return;
  Http2SessionWrap* session = task->session;
  if (task->wrapper_refed && session != nullptr && session->env == env && session->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    (void)napi_reference_unref(env, session->wrapper_ref, &ignored);
  }
  delete task;
}

void MaybeScheduleSessionFlush(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr || session->destroyed || session->session == nullptr) return;
  if (session->write_scheduled) return;
  if (nghttp2_session_want_write(session->session) == 0 &&
      session->pending_rst_stream_ids.empty() &&
      !session->goaway_pending) {
    return;
  }
  session->write_scheduled = true;
  ScheduleSessionFlush(session);
}

void ScheduleSessionFlush(Http2SessionWrap* session) {
  if (session == nullptr || session->env == nullptr) return;
  auto* task = new DeferredSessionFlushTask();
  task->session = session;
  bool wrapper_refed = false;
  if (session->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    if (napi_reference_ref(session->env, session->wrapper_ref, &ignored) == napi_ok) {
      wrapper_refed = true;
    }
  }
  task->wrapper_refed = wrapper_refed;
  if (EdgeRuntimePlatformEnqueueTask(session->env,
                                     RunDeferredSessionFlushTask,
                                     task,
                                     DeleteDeferredSessionFlushTask,
                                     kEdgeRuntimePlatformTaskRefed) != napi_ok) {
    session->write_scheduled = false;
    if (task->wrapper_refed && session->wrapper_ref != nullptr) {
      uint32_t ref_ignored = 0;
      (void)napi_reference_unref(session->env, session->wrapper_ref, &ref_ignored);
      task->wrapper_refed = false;
    }
    delete task;
    (void)FlushSessionOutput(session);
  }
}

void RunDeferredStreamDestroy(Http2StreamWrap* stream) {
  if (stream == nullptr) return;
  stream->destroy_scheduled = false;
  if (stream->session != nullptr &&
      stream->session->session != nullptr &&
      !stream->session->destroyed &&
      !stream->session->parent_write_in_progress &&
      stream->session->write_scheduled) {
    stream->session->write_scheduled = false;
    (void)FlushSessionOutput(stream->session);
  }
  const int pending_write_status =
      (stream->rst_code == NGHTTP2_NO_ERROR || stream->rst_code == NGHTTP2_CANCEL) ? 0 : UV_ECANCELED;
  while (!stream->outbound_chunks.empty()) {
    OutboundChunk chunk = std::move(stream->outbound_chunks.front());
    stream->outbound_chunks.pop_front();
    CompleteStreamReq(stream, &chunk.req_ref, pending_write_status);
  }
  stream->available_outbound_length = 0;
  CompleteStreamReq(stream, &stream->shutdown_req_ref, UV_ECANCELED, true);
  if (stream->session != nullptr) RemoveStreamFromSession(stream);
  EdgeStreamBaseOnClosed(&stream->base);
}

napi_value DeferredDestroyStreamCallback(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, &self, &data);
  auto* stream = static_cast<Http2StreamWrap*>(data);
  if (stream == nullptr) return Undefined(env);
  RunDeferredStreamDestroy(stream);
  if (stream->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    (void)napi_reference_unref(env, stream->wrapper_ref, &ignored);
  }
  return Undefined(env);
}

void ScheduleDeferredStreamDestroy(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->env == nullptr || stream->destroy_scheduled) return;
  stream->destroy_scheduled = true;
  bool wrapper_refed = false;
  if (stream->wrapper_ref != nullptr) {
    uint32_t ignored = 0;
    if (napi_reference_ref(stream->env, stream->wrapper_ref, &ignored) == napi_ok) {
      wrapper_refed = true;
    }
  }
  napi_value callback = nullptr;
  if (napi_create_function(stream->env,
                           "__ubiHttp2DeferredDestroyStream",
                           NAPI_AUTO_LENGTH,
                           DeferredDestroyStreamCallback,
                           stream,
                           &callback) != napi_ok ||
      callback == nullptr) {
    stream->destroy_scheduled = false;
    if (wrapper_refed && stream->wrapper_ref != nullptr) {
      uint32_t ignored = 0;
      (void)napi_reference_unref(stream->env, stream->wrapper_ref, &ignored);
    }
    return;
  }
  napi_value global = GetGlobal(stream->env);
  napi_value set_immediate = nullptr;
  napi_valuetype set_immediate_type = napi_undefined;
  if (global == nullptr ||
      napi_get_named_property(stream->env, global, "setImmediate", &set_immediate) != napi_ok ||
      set_immediate == nullptr ||
      napi_typeof(stream->env, set_immediate, &set_immediate_type) != napi_ok ||
      set_immediate_type != napi_function) {
    stream->destroy_scheduled = false;
    if (wrapper_refed && stream->wrapper_ref != nullptr) {
      uint32_t ignored = 0;
      (void)napi_reference_unref(stream->env, stream->wrapper_ref, &ignored);
    }
    return;
  }
  napi_value argv[1] = {callback};
  napi_value ignored = nullptr;
  if (napi_call_function(stream->env, global, set_immediate, 1, argv, &ignored) != napi_ok) {
    stream->destroy_scheduled = false;
    RunDeferredStreamDestroy(stream);
    if (wrapper_refed && stream->wrapper_ref != nullptr) {
      uint32_t ref_ignored = 0;
      (void)napi_reference_unref(stream->env, stream->wrapper_ref, &ref_ignored);
    }
  }
}

void DestroyStreamHandle(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->destroyed) return;
  DebugStream(stream, "tearing down stream");
  if (stream->session != nullptr && HasPendingRstStream(stream->session, stream->id) &&
      stream->session->session != nullptr && !stream->session->in_scope) {
    (void)nghttp2_submit_rst_stream(stream->session->session, NGHTTP2_FLAG_NONE, stream->id, stream->rst_code);
  }
  stream->destroyed = true;
  ScheduleDeferredStreamDestroy(stream);
}

void InvokeStreamCloseCallback(Http2StreamWrap* stream, uint32_t error_code) {
  if (stream == nullptr) return;
  Http2BindingState* state = GetHttp2State(stream->env);
  napi_value stream_obj = GetRefValue(stream->env, stream->wrapper_ref);
  if (state != nullptr && stream_obj != nullptr) {
    napi_value argv[1] = {nullptr};
    napi_value callback_result = nullptr;
    napi_create_uint32(stream->env, error_code, &argv[0]);
    const bool called = CallCallbackRef(
        stream->env, state->callback_refs[kCallbackStreamClose], stream->async_id, stream_obj, 1, argv, &callback_result);
    bool should_destroy = false;
    if (!called || callback_result == nullptr) {
      should_destroy = true;
    } else {
      napi_valuetype type = napi_undefined;
      bool keep_open = false;
      if (napi_typeof(stream->env, callback_result, &type) != napi_ok || type != napi_boolean ||
          napi_get_value_bool(stream->env, callback_result, &keep_open) != napi_ok) {
        should_destroy = true;
      } else {
        should_destroy = !keep_open;
      }
    }
    DebugStream(stream, "stream close callback requested destroy=%s", should_destroy ? "true" : "false");
    if (should_destroy) {
      DestroyStreamHandle(stream);
    }
  } else {
    DebugStream(stream, "stream close callback missing, destroying");
    DestroyStreamHandle(stream);
  }
}

void ConsumeHTTP2Data(Http2SessionWrap* session) {
  if (session == nullptr || session->session == nullptr || session->destroyed ||
      session->pending_parent_read_bytes.empty()) {
    ClearPendingParentRead(session);
    return;
  }
  if (session->pending_parent_read_offset > session->pending_parent_read_bytes.size()) {
    ClearPendingParentRead(session);
    return;
  }

  const uint8_t* data =
      session->pending_parent_read_bytes.data() + session->pending_parent_read_offset;
  const size_t len = session->pending_parent_read_bytes.size() - session->pending_parent_read_offset;
  DebugSession(session,
               "consume input len=%zu offset=%zu want_read=%d paused=%s",
               len,
               session->pending_parent_read_offset,
               nghttp2_session_want_read(session->session),
               session->receive_paused ? "true" : "false");
  session->receive_paused = false;
  session->custom_recv_error_code = nullptr;
  session->in_scope = true;

  const ssize_t rc = nghttp2_session_mem_recv(session->session, data, len);
  session->in_scope = false;
  DebugSession(session,
               "consume input result=%zd paused=%s remaining=%zu",
               rc,
               session->receive_paused ? "true" : "false",
               session->pending_parent_read_bytes.size() -
                   std::min(session->pending_parent_read_offset, session->pending_parent_read_bytes.size()));
  if (session->receive_paused) {
    if (rc > 0) {
      session->pending_parent_read_offset += static_cast<size_t>(rc);
    }
  } else {
    ClearPendingParentRead(session);
  }

  if (rc < 0) {
    EmitInternalError(session, static_cast<int32_t>(rc), session->custom_recv_error_code);
    return;
  }

  if (!session->destroyed && !session->receive_paused) {
    MaybeScheduleSessionFlush(session);
    MaybeStopParentReading(session);
  }
}

bool ParentStreamOnRead(EdgeStreamListener* listener, ssize_t nread, const uv_buf_t* buf) {
  if (listener == nullptr) return false;
  auto* session = static_cast<Http2SessionWrap*>(listener->data);
  if (session != nullptr) {
    DebugSession(session, "parent read nread=%zd", nread);
  }
  if (session == nullptr || session->destroyed || session->session == nullptr) {
    ReleaseParentReadBuffer(buf);
    return nread >= 0;
  }
  if (nread < 0) {
    session->parent_closed = true;
    session->parent_stream_base = nullptr;
    session->write_scheduled = false;
    ReleaseParentReadBuffer(buf);
    return false;
  }
  if (nread == 0 || buf == nullptr || buf->base == nullptr) {
    ReleaseParentReadBuffer(buf);
    return true;
  }

  AppendPendingParentRead(session,
                          reinterpret_cast<const uint8_t*>(buf->base),
                          static_cast<size_t>(nread));
  ReleaseParentReadBuffer(buf);
  if (session->receive_paused) {
    return true;
  }
  ConsumeHTTP2Data(session);
  return true;
}

bool ParentStreamOnAfterShutdown(EdgeStreamListener* listener, napi_value req_obj, int status) {
  if (listener == nullptr) return false;
  return EdgeStreamPassAfterShutdown(listener, req_obj, status);
}

bool ParentStreamOnAfterWrite(EdgeStreamListener* listener, napi_value req_obj, int status) {
  if (listener == nullptr) return false;
  auto* session = static_cast<Http2SessionWrap*>(listener->data);
  if (session == nullptr) return false;
  DebugSession(session,
               "parent after write status=%d pending_read=%zu scheduled=%s destroyed=%s",
               status,
               session->pending_parent_read_bytes.size() -
                   std::min(session->pending_parent_read_offset, session->pending_parent_read_bytes.size()),
               session->write_scheduled ? "true" : "false",
               session->destroyed ? "true" : "false");
  session->parent_write_in_progress = false;
  ClearPendingParentWrite(session, status);
  if (status == 0) {
    FlushPendingRstStreams(session);
  }
  MaybeNotifyGracefulCloseComplete(session);
  MaybeResumeParentReading(session);
  (void)EdgeStreamPassAfterWrite(listener, req_obj, 0);
  if (session->destroyed) {
    NotifySessionDone(session, true);
    return true;
  }
  if (session->pending_parent_read_offset < session->pending_parent_read_bytes.size()) {
    ConsumeHTTP2Data(session);
  }
  if (session->write_scheduled) ScheduleSessionFlush(session);
  return true;
}

void ParentStreamOnClose(EdgeStreamListener* listener) {
  if (listener == nullptr) return;
  auto* session = static_cast<Http2SessionWrap*>(listener->data);
  if (session == nullptr) return;
  session->parent_closed = true;
  session->parent_stream_base = nullptr;
  listener->previous = nullptr;
}

void CompleteParentWriteRequest(Http2SessionWrap* session, napi_value req_obj, int status) {
  if (session == nullptr || req_obj == nullptr) return;
  if (session->parent_stream_base != nullptr) {
    EdgeStreamBaseEmitAfterWrite(session->parent_stream_base, req_obj, status);
  } else {
    EdgeStreamBaseInvokeReqOnComplete(session->env, req_obj, status, nullptr, 0);
  }
}

int FlushSessionOutput(Http2SessionWrap* session, napi_value req_obj) {
  if (session == nullptr || session->session == nullptr || session->destroyed) return 0;
  DebugSession(session,
               "flush output start write_in_progress=%s want_write=%d pending_rst=%zu goaway=%s",
               session->parent_write_in_progress ? "true" : "false",
               nghttp2_session_want_write(session->session),
               session->pending_rst_stream_ids.size(),
               session->goaway_pending ? "true" : "false");
  if (session->parent_write_in_progress) {
    session->write_scheduled = true;
    return 0;
  }
  session->pending_parent_write_chunks.clear();
  session->pending_parent_write_length = 0;
  const bool restore_in_scope = !session->in_scope;
  if (restore_in_scope) session->in_scope = true;
  for (;;) {
    const uint8_t* data = nullptr;
    const ssize_t len = nghttp2_session_mem_send(session->session, &data);
    if (len < 0) {
      if (restore_in_scope) session->in_scope = false;
      EmitInternalError(session, static_cast<int32_t>(len));
      return static_cast<int>(len);
    }
    if (len == 0 || data == nullptr) break;
    CopyDataIntoPendingWrite(session, data, len);
  }
  if (restore_in_scope) session->in_scope = false;
  if (session->pending_parent_write_chunks.empty()) {
    DebugSession(session, "flush output produced no parent bytes");
    if (!session->pending_rst_stream_ids.empty() && !session->parent_write_in_progress) {
      FlushPendingRstStreams(session);
    }
    MaybeNotifyGracefulCloseComplete(session);
    return 0;
  }

  napi_value actual_req = req_obj != nullptr ? req_obj : EdgeCreateStreamReqObject(session->env);
  if (actual_req == nullptr) return UV_ENOMEM;
  session->chunks_sent_since_last_write++;
  session->data_sent += session->pending_parent_write_length;
  bool write_async = false;
  const int rc = WriteChunksToParent(session, actual_req, &write_async);
  DebugSession(session,
               "flush output writing %zu chunks (%zu bytes) async=%s rc=%d",
               session->pending_parent_write_chunks.size(),
               session->pending_parent_write_length,
               write_async ? "true" : "false",
               rc);
  if (rc == 0) {
    if (write_async) {
      session->parent_write_in_progress = true;
    } else {
      CompleteParentWriteRequest(session, actual_req, 0);
    }
    MaybeStopParentReading(session);
  } else {
    CompleteParentWriteRequest(session, actual_req, rc);
  }
  return rc;
}

InboundDelivery DeliverInboundChunk(Http2StreamWrap* stream, const uint8_t* data, size_t len) {
  if (stream == nullptr || stream->env == nullptr || data == nullptr || len == 0) {
    return InboundDelivery::kQueued;
  }
  DebugStream(stream,
              "delivering inbound chunk len=%zu reading=%s has_onread=%s native_listener=%s",
              len,
              stream->reading ? "true" : "false",
              HasJsOnRead(stream) ? "true" : "false",
              HasActiveNativeReadListener(stream) ? "true" : "false");
  if (EdgeStreamBaseEmitReadBuffer(&stream->base, data, len)) {
    if (HasActiveNativeReadListener(stream)) return InboundDelivery::kDeliveredActive;
    return stream->reading ? InboundDelivery::kDeliveredActive : InboundDelivery::kDeliveredPaused;
  }
  napi_value self = GetRefValue(stream->env, stream->wrapper_ref);
  napi_value callback = GetRefValue(stream->env, stream->onread_ref);
  if (self == nullptr || !IsFunction(stream->env, callback)) {
    stream->inbound_chunks.emplace_back(data, data + len);
    return InboundDelivery::kQueued;
  }

  napi_value arraybuffer = nullptr;
  void* out = nullptr;
  if (napi_create_arraybuffer(stream->env, len, &out, &arraybuffer) != napi_ok || out == nullptr ||
      arraybuffer == nullptr) {
    return InboundDelivery::kQueued;
  }
  std::memcpy(out, data, len);
  DebugStream(stream, "invoking JS onread for inbound chunk len=%zu", len);
  SetReadState(stream->env, static_cast<int32_t>(std::min<size_t>(len, INT32_MAX)));
  napi_value argv[1] = {arraybuffer};
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      stream->env, stream->async_id, self, self, callback, 1, argv, &ignored, kEdgeMakeCallbackNone);
  SetReadState(stream->env, 0);
  return stream->reading ? InboundDelivery::kDeliveredActive : InboundDelivery::kDeliveredPaused;
}

void FlushInboundChunks(Http2StreamWrap* stream) {
  if (stream == nullptr || !stream->reading) return;
  while (!stream->inbound_chunks.empty()) {
    std::vector<uint8_t> chunk = std::move(stream->inbound_chunks.front());
    stream->inbound_chunks.pop_front();
    (void)DeliverInboundChunk(stream, chunk.data(), chunk.size());
    if (!stream->reading) break;
  }
  FlushQueuedInboundConsumption(stream);
  if (stream->reading && stream->inbound_eof) EmitInboundEOF(stream);
}

void FlushQueuedInboundConsumption(Http2StreamWrap* stream) {
  if (stream == nullptr || stream->session == nullptr || stream->session->session == nullptr ||
      stream->buffered_inbound_bytes == 0) {
    return;
  }
  const size_t amount = stream->buffered_inbound_bytes;
  stream->buffered_inbound_bytes = 0;
  (void)nghttp2_session_consume_stream(stream->session->session, stream->id, amount);
  MaybeScheduleSessionFlush(stream->session);
}

ssize_t SelectDwordAlignedPadding(size_t frame_len, size_t max_payload_len) {
  const size_t remainder = (frame_len + 9) % 8;
  if (remainder == 0) return static_cast<ssize_t>(frame_len);
  return static_cast<ssize_t>(std::min(max_payload_len, frame_len + (8 - remainder)));
}

ssize_t SelectMaxFrameSizePadding(size_t /*frame_len*/, size_t max_payload_len) {
  return static_cast<ssize_t>(max_payload_len);
}

ssize_t OnSelectPadding(nghttp2_session*,
                        const nghttp2_frame* frame,
                        size_t max_payload_len,
                        void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr) {
    return static_cast<ssize_t>(frame != nullptr ? frame->hd.length : 0);
  }
  switch (session->padding_strategy) {
    case 2:
      return SelectMaxFrameSizePadding(frame->hd.length, max_payload_len);
    case 1:
      return SelectDwordAlignedPadding(frame->hd.length, max_payload_len);
    default:
      return static_cast<ssize_t>(frame->hd.length);
  }
}

int OnInvalidHeader(nghttp2_session*,
                    const nghttp2_frame*,
                    nghttp2_rcbuf*,
                    nghttp2_rcbuf*,
                    uint8_t,
                    void*) {
  return 0;
}

int OnInvalidFrameRecv(nghttp2_session*,
                       const nghttp2_frame*,
                       int lib_error_code,
                       void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr) return 0;
  const uint32_t max_invalid_frames =
      session->fields != nullptr ? session->fields->max_invalid_frames : 1000;
  if (session->invalid_frame_count++ > max_invalid_frames) {
    session->custom_recv_error_code = "ERR_HTTP2_TOO_MANY_INVALID_FRAMES";
    return 1;
  }

  if (nghttp2_is_fatal(lib_error_code) || lib_error_code == NGHTTP2_ERR_STREAM_CLOSED ||
      lib_error_code == NGHTTP2_ERR_PROTO) {
    EmitInternalError(session, lib_error_code);
  }
  return 0;
}

int OnNghttpError(nghttp2_session*, int lib_error_code, const char*, size_t, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr) return 0;
  if (lib_error_code == NGHTTP2_ERR_SETTINGS_EXPECTED) {
    EmitInternalError(session, NGHTTP2_ERR_PROTO);
  }
  return 0;
}

ssize_t OnDataSourceRead(nghttp2_session*,
                         int32_t,
                         uint8_t* buf,
                         size_t length,
                         uint32_t* data_flags,
                         nghttp2_data_source* source,
                         void*) {
  auto* stream = source != nullptr ? static_cast<Http2StreamWrap*>(source->ptr) : nullptr;
  if (stream == nullptr) return NGHTTP2_ERR_CALLBACK_FAILURE;

  size_t copied = 0;
  while (!stream->outbound_chunks.empty() && stream->outbound_chunks.front().data.empty()) {
    OutboundChunk chunk = std::move(stream->outbound_chunks.front());
    stream->outbound_chunks.pop_front();
    CompleteStreamReq(stream, &chunk.req_ref, 0);
  }

  if (stream->available_outbound_length > 0) {
    copied = std::min(stream->available_outbound_length, length);
    if (copied > 0) {
      stream->available_outbound_length -= copied;
      *data_flags |= NGHTTP2_DATA_FLAG_NO_COPY;
    }
  }

  if (copied == 0) {
    if (stream->shutdown_requested) {
      *data_flags |= NGHTTP2_DATA_FLAG_EOF;
      if (stream->has_trailers) *data_flags |= NGHTTP2_DATA_FLAG_NO_END_STREAM;
      stream->local_closed = true;
      if (stream->has_trailers) CallStreamTrailersCallback(stream);
      CompleteStreamReq(stream, &stream->shutdown_req_ref, 0, true);
      return 0;
    }
    EdgeStreamBaseEmitWantsWrite(&stream->base, length);
    if (stream->available_outbound_length > 0 || stream->shutdown_requested) {
      return OnDataSourceRead(nullptr, 0, buf, length, data_flags, source, nullptr);
    }
    return NGHTTP2_ERR_DEFERRED;
  }

  if (stream->available_outbound_length == 0 && stream->shutdown_requested) {
    *data_flags |= NGHTTP2_DATA_FLAG_EOF;
    if (stream->has_trailers) *data_flags |= NGHTTP2_DATA_FLAG_NO_END_STREAM;
    stream->local_closed = true;
    if (stream->has_trailers) CallStreamTrailersCallback(stream);
    CompleteStreamReq(stream, &stream->shutdown_req_ref, 0, true);
  }
  return static_cast<ssize_t>(copied);
}

int OnSendData(nghttp2_session*, nghttp2_frame* frame, const uint8_t* framehd, size_t length, nghttp2_data_source*, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr) return 0;
  auto it = session->streams.find(frame->hd.stream_id);
  if (it == session->streams.end() || it->second == nullptr) return 0;
  Http2StreamWrap* stream = it->second;

  CopyDataIntoPendingWrite(session, framehd, 9);
  if (frame->data.padlen > 0) {
    const uint8_t padding_byte = frame->data.padlen - 1;
    CopyDataIntoPendingWrite(session, &padding_byte, 1);
  }

  while (length > 0 && !stream->outbound_chunks.empty()) {
    OutboundChunk& chunk = stream->outbound_chunks.front();
    const size_t available = chunk.data.size() - chunk.offset;
    const size_t to_copy = std::min(available, length);
    if (to_copy > 0) {
      CopyDataIntoPendingWrite(session, chunk.data.data() + chunk.offset, to_copy);
      chunk.offset += to_copy;
      stream->bytes_written += to_copy;
      length -= to_copy;
    }
    if (chunk.offset >= chunk.data.size()) {
      if (chunk.req_ref != nullptr) {
        session->pending_parent_write_req_refs.push_back(chunk.req_ref);
        chunk.req_ref = nullptr;
      }
      stream->outbound_chunks.pop_front();
    }
  }

  if (frame->data.padlen > 0) {
    std::vector<uint8_t> padding(frame->data.padlen - 1, 0);
    if (!padding.empty()) CopyDataIntoPendingWrite(session, padding.data(), padding.size());
  }

  if ((frame->hd.flags & NGHTTP2_FLAG_END_STREAM) != 0 && stream->shutdown_req_ref != nullptr) {
    session->pending_parent_write_req_refs.push_back(stream->shutdown_req_ref);
    stream->shutdown_req_ref = nullptr;
  }

  return 0;
}

int OnBeginHeaders(nghttp2_session*, const nghttp2_frame* frame, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr) return 0;
  const int32_t frame_id = GetFrameId(frame);
  DebugSession(session, "begin headers for stream %d", frame_id);
  auto stream_it = session->streams.find(frame_id);
  if (stream_it == session->streams.end() || stream_it->second == nullptr) {
    if (!SessionCanAddStream(session)) {
      const uint32_t max_rejected_streams =
          session->fields != nullptr ? session->fields->max_rejected_streams : 100;
      if (session->rejected_stream_count++ > max_rejected_streams) {
        return NGHTTP2_ERR_CALLBACK_FAILURE;
      }
      (void)nghttp2_submit_rst_stream(
          session->session, NGHTTP2_FLAG_NONE, frame_id, NGHTTP2_ENHANCE_YOUR_CALM);
      return NGHTTP2_ERR_TEMPORAL_CALLBACK_FAILURE;
    }
    napi_value stream_obj = nullptr;
    Http2StreamWrap* stream = nullptr;
    if (!CreateStreamHandle(session, frame_id, frame->headers.cat, &stream_obj, &stream) ||
        stream == nullptr) {
      const uint32_t max_rejected_streams =
          session->fields != nullptr ? session->fields->max_rejected_streams : 100;
      if (session->rejected_stream_count++ > max_rejected_streams) {
        return NGHTTP2_ERR_CALLBACK_FAILURE;
      }
      (void)nghttp2_submit_rst_stream(
          session->session, NGHTTP2_FLAG_NONE, frame_id, NGHTTP2_ENHANCE_YOUR_CALM);
      return NGHTTP2_ERR_TEMPORAL_CALLBACK_FAILURE;
    }
    session->rejected_stream_count = 0;
  } else if (stream_it->second != nullptr) {
    stream_it->second->current_category = frame->headers.cat;
  }
  PendingHeaderBlock& block = session->pending_headers[frame_id];
  block.stream_id = frame_id;
  block.category = frame->headers.cat;
  block.flags = frame->hd.flags;
  block.total_length = 0;
  block.headers.clear();
  block.sensitive_headers.clear();
  return 0;
}

int OnHeader(nghttp2_session*,
             const nghttp2_frame* frame,
             const uint8_t* name,
             size_t namelen,
             const uint8_t* value,
             size_t valuelen,
             uint8_t flags,
             void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr) return 0;
  if (namelen == 0) return 0;
  auto stream_it = session->streams.find(GetFrameId(frame));
  if (stream_it == session->streams.end() || stream_it->second == nullptr) {
    return NGHTTP2_ERR_TEMPORAL_CALLBACK_FAILURE;
  }
  Http2StreamWrap* stream = stream_it->second;
  if (stream->destroyed) return 0;
  PendingHeaderBlock& block = session->pending_headers[GetFrameId(frame)];
  const size_t header_pairs = block.headers.size() / 2;
  const size_t header_length = namelen + valuelen + 32;
  if (!HasAvailableSessionMemory(session, header_length) ||
      header_pairs >= stream->max_header_pairs ||
      block.total_length + header_length > stream->max_header_length) {
    (void)nghttp2_submit_rst_stream(
        session->session, NGHTTP2_FLAG_NONE, GetFrameId(frame), NGHTTP2_ENHANCE_YOUR_CALM);
    return NGHTTP2_ERR_TEMPORAL_CALLBACK_FAILURE;
  }
  block.total_length += header_length;
  block.headers.emplace_back(reinterpret_cast<const char*>(name), namelen);
  block.headers.emplace_back(reinterpret_cast<const char*>(value), valuelen);
  if ((flags & NGHTTP2_NV_FLAG_NO_INDEX) != 0) {
    block.sensitive_headers.emplace_back(reinterpret_cast<const char*>(name), namelen);
  }
  return 0;
}

int OnHeaderRcbuf(nghttp2_session*,
                  const nghttp2_frame* frame,
                  nghttp2_rcbuf* name,
                  nghttp2_rcbuf* value,
                  uint8_t flags,
                  void* user_data) {
  if (name == nullptr || value == nullptr) return 0;
  const nghttp2_vec name_buf = nghttp2_rcbuf_get_buf(name);
  const nghttp2_vec value_buf = nghttp2_rcbuf_get_buf(value);
  return OnHeader(nullptr,
                  frame,
                  reinterpret_cast<const uint8_t*>(name_buf.base),
                  name_buf.len,
                  reinterpret_cast<const uint8_t*>(value_buf.base),
                  value_buf.len,
                  flags,
                  user_data);
}

int OnDataChunkRecv(nghttp2_session*, uint8_t, int32_t stream_id, const uint8_t* data, size_t len, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr) return 0;
  if (len == 0) return 0;
  (void)nghttp2_session_consume_connection(session->session, len);
  auto it = session->streams.find(stream_id);
  if (it == session->streams.end() || it->second == nullptr) return 0;
  Http2StreamWrap* stream = it->second;
  if (stream->destroyed) {
    (void)nghttp2_session_consume_stream(session->session, stream_id, len);
    return 0;
  }
  const InboundDelivery delivery = DeliverInboundChunk(stream, data, len);
  if (delivery == InboundDelivery::kDeliveredActive) {
    (void)nghttp2_session_consume_stream(session->session, stream_id, len);
  } else {
    stream->buffered_inbound_bytes += len;
  }
  if (session->parent_write_in_progress) {
    MaybeStopParentReading(session);
    session->receive_paused = true;
    return NGHTTP2_ERR_PAUSE;
  }
  return 0;
}

int OnFrameRecv(nghttp2_session*, const nghttp2_frame* frame, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr || session->env == nullptr) return 0;
  Http2BindingState* state = GetHttp2State(session->env);
  napi_value session_obj = GetRefValue(session->env, session->wrapper_ref);
  if (state == nullptr || session_obj == nullptr) return 0;
  session->frames_received++;

  switch (frame->hd.type) {
    case NGHTTP2_DATA: {
      DebugSession(session, "handling data frame for stream %d", frame->hd.stream_id);
      session->data_received += frame->hd.length;
      auto stream_it = session->streams.find(frame->hd.stream_id);
      if (stream_it != session->streams.end() && stream_it->second != nullptr &&
          !stream_it->second->destroyed &&
          (frame->hd.flags & NGHTTP2_FLAG_END_STREAM) != 0) {
        stream_it->second->remote_closed = true;
        EmitInboundEOF(stream_it->second);
      } else if (frame->hd.length == 0) {
        const uint32_t max_invalid_frames =
            session->fields != nullptr ? session->fields->max_invalid_frames : 1000;
        if (session->invalid_frame_count++ > max_invalid_frames) {
          session->custom_recv_error_code = "ERR_HTTP2_TOO_MANY_INVALID_FRAMES";
          return 1;
        }
      }
      break;
    }
    case NGHTTP2_PUSH_PROMISE:
    case NGHTTP2_HEADERS: {
      const int32_t frame_id = GetFrameId(frame);
      auto pending_it = session->pending_headers.find(frame_id);
      if (pending_it == session->pending_headers.end()) break;
      PendingHeaderBlock pending_block = std::move(pending_it->second);
      session->pending_headers.erase(pending_it);
      auto stream_it = session->streams.find(frame_id);
      Http2StreamWrap* stream = stream_it != session->streams.end() ? stream_it->second : nullptr;
      napi_value stream_obj = GetRefValue(session->env, stream != nullptr ? stream->wrapper_ref : nullptr);
      if (stream == nullptr || stream_obj == nullptr) break;
      stream->current_category = pending_block.category;
      napi_value headers = nullptr;
      napi_value sensitive = nullptr;
      if (!CreateJsStringArray(session->env, pending_block.headers, &headers) ||
          !CreateJsStringArray(session->env, pending_block.sensitive_headers, &sensitive)) {
        break;
      }
      napi_value argv[6] = {stream_obj, nullptr, nullptr, nullptr, headers, sensitive};
      napi_create_int32(session->env, frame_id, &argv[1]);
      napi_create_int32(session->env, pending_block.category, &argv[2]);
      napi_create_int32(session->env, frame->hd.flags, &argv[3]);
      (void)CallCallbackRef(
          session->env, state->callback_refs[kCallbackHeaders], session->async_id, session_obj, 6, argv);
      break;
    }
    case NGHTTP2_SETTINGS: {
      if ((frame->hd.flags & NGHTTP2_FLAG_ACK) != 0) {
        if (session->pending_settings.empty()) {
          EmitInternalError(session, NGHTTP2_ERR_PROTO);
          break;
        }
        PendingSettings pending = std::move(session->pending_settings.front());
        session->pending_settings.pop_front();
        CompletePendingSettings(session->env, &pending, true);
      } else {
        if (session->fields != nullptr) {
          session->fields->bitfield &= ~(1 << kSessionRemoteSettingsIsUpToDate);
        }
        if (session->remote_custom_settings.number > 0) {
          nghttp2_settings_entry* iv = frame->settings.iv;
          const size_t niv = frame->settings.niv;
          for (size_t i = 0; i < niv; ++i) {
            const int32_t settings_id = iv[i].settings_id;
            if (IsStandardSettingId(settings_id)) continue;
            for (size_t j = 0; j < session->remote_custom_settings.number; ++j) {
              if ((session->remote_custom_settings.entries[j].settings_id & 0xffff) == settings_id) {
                session->remote_custom_settings.entries[j].settings_id = settings_id;
                session->remote_custom_settings.entries[j].value = iv[i].value;
                break;
              }
            }
          }
        }
        if (session->fields == nullptr ||
            (session->fields->bitfield & (1 << kSessionHasRemoteSettingsListeners)) == 0) {
          break;
        }
        (void)CallCallbackRef(
            session->env, state->callback_refs[kCallbackSettings], session->async_id, session_obj, 0, nullptr);
      }
      break;
    }
    case NGHTTP2_PING: {
      if ((frame->hd.flags & NGHTTP2_FLAG_ACK) != 0) {
        if (session->pending_pings.empty()) {
          EmitInternalError(session, NGHTTP2_ERR_PROTO);
          break;
        }
        PendingPing pending = std::move(session->pending_pings.front());
        session->pending_pings.pop_front();
        CompletePendingPing(session->env, &pending, true, frame->ping.opaque_data);
      } else {
        if (session->fields == nullptr ||
            (session->fields->bitfield & (1 << kSessionHasPingListeners)) == 0) {
          break;
        }
        napi_value buffer = nullptr;
        void* copy = nullptr;
        napi_create_buffer_copy(session->env,
                                sizeof(frame->ping.opaque_data),
                                frame->ping.opaque_data,
                                &copy,
                                &buffer);
        napi_value argv[1] = {buffer};
        (void)CallCallbackRef(session->env, state->callback_refs[kCallbackPing], session->async_id, session_obj, 1, argv);
      }
      break;
    }
    case NGHTTP2_GOAWAY: {
      napi_value buffer = nullptr;
      void* copy = nullptr;
      napi_create_buffer_copy(session->env,
                              frame->goaway.opaque_data_len,
                              frame->goaway.opaque_data,
                              &copy,
                              &buffer);
      napi_value argv[3] = {nullptr, nullptr, buffer};
      napi_create_uint32(session->env, frame->goaway.error_code, &argv[0]);
      napi_create_int32(session->env, frame->goaway.last_stream_id, &argv[1]);
      (void)CallCallbackRef(
          session->env, state->callback_refs[kCallbackGoawayData], session->async_id, session_obj, 3, argv);
      break;
    }
    case NGHTTP2_PRIORITY: {
      if (session->fields != nullptr && session->fields->priority_listener_count == 0) break;
      napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
      napi_create_int32(session->env, frame->hd.stream_id, &argv[0]);
      napi_create_int32(session->env, frame->priority.pri_spec.stream_id, &argv[1]);
      napi_create_int32(session->env, frame->priority.pri_spec.weight, &argv[2]);
      napi_get_boolean(session->env, frame->priority.pri_spec.exclusive != 0, &argv[3]);
      (void)CallCallbackRef(
          session->env, state->callback_refs[kCallbackPriority], session->async_id, session_obj, 4, argv);
      break;
    }
    case NGHTTP2_ALTSVC: {
      if (session->fields == nullptr ||
          (session->fields->bitfield & (1 << kSessionHasAltsvcListeners)) == 0) {
        break;
      }
      nghttp2_ext_altsvc* altsvc = static_cast<nghttp2_ext_altsvc*>(frame->ext.payload);
      if (altsvc == nullptr) break;
      napi_value argv[3] = {nullptr, nullptr, nullptr};
      napi_create_int32(session->env, frame->hd.stream_id, &argv[0]);
      napi_create_string_latin1(session->env,
                                reinterpret_cast<const char*>(altsvc->origin),
                                altsvc->origin_len,
                                &argv[1]);
      napi_create_string_latin1(session->env,
                                reinterpret_cast<const char*>(altsvc->field_value),
                                altsvc->field_value_len,
                                &argv[2]);
      (void)CallCallbackRef(
          session->env, state->callback_refs[kCallbackAltsvc], session->async_id, session_obj, 3, argv);
      break;
    }
    case NGHTTP2_ORIGIN: {
      nghttp2_ext_origin* origin = static_cast<nghttp2_ext_origin*>(frame->ext.payload);
      if (origin == nullptr) break;
      napi_value origins = nullptr;
      if (napi_create_array_with_length(session->env, origin->nov, &origins) != napi_ok || origins == nullptr) {
        break;
      }
      for (size_t i = 0; i < origin->nov; ++i) {
        napi_value value = nullptr;
        napi_create_string_latin1(session->env,
                                  reinterpret_cast<const char*>(origin->ov[i].origin),
                                  origin->ov[i].origin_len,
                                  &value);
        napi_set_element(session->env, origins, i, value);
      }
      napi_value argv[1] = {origins};
      (void)CallCallbackRef(
          session->env, state->callback_refs[kCallbackOrigin], session->async_id, session_obj, 1, argv);
      break;
    }
    default:
      break;
  }
  return 0;
}

int OnFrameNotSend(nghttp2_session*, const nghttp2_frame* frame, int lib_error_code, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr || frame == nullptr) return 0;
  if (lib_error_code == NGHTTP2_ERR_SESSION_CLOSING || lib_error_code == NGHTTP2_ERR_STREAM_CLOSED ||
      lib_error_code == NGHTTP2_ERR_STREAM_CLOSING) {
    return 0;
  }
  Http2BindingState* state = GetHttp2State(session->env);
  napi_value session_obj = GetRefValue(session->env, session->wrapper_ref);
  if (state == nullptr || session_obj == nullptr) return 0;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_create_int32(session->env, frame->hd.stream_id, &argv[0]);
  napi_create_int32(session->env, frame->hd.type, &argv[1]);
  napi_create_uint32(session->env, TranslateNghttp2ErrorCode(lib_error_code), &argv[2]);
  (void)CallCallbackRef(
      session->env, state->callback_refs[kCallbackFrameError], session->async_id, session_obj, 3, argv);
  return 0;
}

int OnFrameSent(nghttp2_session*, const nghttp2_frame* frame, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session != nullptr) {
    session->frames_sent++;
    if (frame != nullptr && frame->hd.type == NGHTTP2_GOAWAY) {
      session->goaway_pending = false;
    }
  }
  return 0;
}

int OnStreamClose(nghttp2_session*, int32_t stream_id, uint32_t error_code, void* user_data) {
  auto* session = static_cast<Http2SessionWrap*>(user_data);
  if (session == nullptr) return 0;
  auto it = session->streams.find(stream_id);
  if (it == session->streams.end() || it->second == nullptr) return 0;
  Http2StreamWrap* stream = it->second;
  if (stream->destroyed) return 0;
  stream->remote_closed = true;
  stream->rst_code = error_code;
  DebugStream(stream, "closed with code %u", error_code);
  if (error_code == NGHTTP2_NO_ERROR && !stream->reading && !stream->inbound_chunks.empty()) {
    stream->close_pending = true;
    stream->pending_close_code = error_code;
    DebugStream(stream, "deferring stream close callback until queued inbound data is drained");
    return 0;
  }
  InvokeStreamCloseCallback(stream, error_code);
  return 0;
}

napi_value SessionGetFields(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  napi_value fields = wrap != nullptr ? GetRefValue(env, wrap->fields_ref) : nullptr;
  return fields != nullptr ? fields : Undefined(env);
}

napi_value SessionGetChunksSentSinceLastWrite(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  napi_value out = nullptr;
  napi_create_double(env, static_cast<double>(wrap != nullptr ? wrap->chunks_sent_since_last_write : 0), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionSetChunksSentSinceLastWrite(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  double value = 0;
  if (wrap != nullptr && argc >= 1 && napi_get_value_double(env, argv[0], &value) == napi_ok) {
    wrap->chunks_sent_since_last_write = value < 0 ? 0 : static_cast<uint64_t>(value);
  }
  return Undefined(env);
}

napi_value SessionUpdateChunksSent(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  napi_value out = nullptr;
  const double value = static_cast<double>(wrap != nullptr ? wrap->chunks_sent_since_last_write : 0);
  if (wrap != nullptr) {
    wrap->chunks_sent_since_last_write = static_cast<uint64_t>(value);
    napi_set_named_property(env, self, "chunksSentSinceLastWrite", EdgeStreamBaseMakeDouble(env, value));
  }
  napi_create_double(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionConsume(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);

  if (wrap->parent_stream_base != nullptr) {
    (void)EdgeStreamBaseRemoveListener(wrap->parent_stream_base, &wrap->parent_stream_listener);
    wrap->parent_stream_base = nullptr;
  }
  DeleteRefIfPresent(env, &wrap->parent_handle_ref);
  (void)napi_create_reference(env, argv[0], 1, &wrap->parent_handle_ref);
  wrap->parent_closed = false;

  wrap->parent_stream_base = EdgeStreamBaseFromValue(env, argv[0]);
  DebugSession(wrap, "consume parent stream base=%s", wrap->parent_stream_base != nullptr ? "true" : "false");
  if (wrap->parent_stream_base != nullptr) {
    wrap->parent_stream_listener.on_alloc = ParentStreamOnAlloc;
    wrap->parent_stream_listener.on_read = ParentStreamOnRead;
    wrap->parent_stream_listener.on_after_write = ParentStreamOnAfterWrite;
    wrap->parent_stream_listener.on_after_shutdown = ParentStreamOnAfterShutdown;
    wrap->parent_stream_listener.on_close = ParentStreamOnClose;
    wrap->parent_stream_listener.data = wrap;
    (void)EdgeStreamBasePushListener(wrap->parent_stream_base, &wrap->parent_stream_listener);
    wrap->parent_stream_listener_base = wrap->parent_stream_base;
  }

  int32_t ignored = 0;
  (void)CallNamedIntMethod(env, argv[0], "readStart", 0, nullptr, &ignored);
  wrap->parent_reading_stopped = false;
  wrap->consumed = true;
  return Undefined(env);
}

napi_value SessionReceive(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr && wrap->session != nullptr && argc >= 1) {
    const uint8_t* data = nullptr;
    size_t len = 0;
    std::string temp_utf8;
    if (GetByteSpan(env, argv[0], &data, &len, &temp_utf8) && data != nullptr) {
      AppendPendingParentRead(wrap, data, len);
      ConsumeHTTP2Data(wrap);
    }
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionDestroy(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr && !wrap->destroyed) {
    uint32_t code = NGHTTP2_NO_ERROR;
    bool socket_closed = false;
    if (argc >= 1) (void)napi_get_value_uint32(env, argv[0], &code);
    if (argc >= 2) (void)napi_get_value_bool(env, argv[1], &socket_closed);
    const bool parent_gone = socket_closed || wrap->parent_closed;
    MaybeStopParentReading(wrap);
    ClearPendingParentRead(wrap);
    if (parent_gone && wrap->parent_stream_base != nullptr) {
      (void)EdgeStreamBaseRemoveListener(wrap->parent_stream_base, &wrap->parent_stream_listener);
      wrap->parent_stream_base = nullptr;
    }
    if (parent_gone) wrap->parent_closed = true;
    if (!parent_gone && wrap->session != nullptr) {
      (void)nghttp2_session_terminate_session(wrap->session, code);
      (void)FlushSessionOutput(wrap);
    }
    wrap->destroyed = true;
    while (!wrap->pending_pings.empty()) {
      PendingPing pending = std::move(wrap->pending_pings.front());
      wrap->pending_pings.pop_front();
      CompletePendingPing(env, &pending, false);
    }
    if (!wrap->parent_write_in_progress || wrap->parent_handle_ref == nullptr) {
      NotifySessionDone(wrap, !parent_gone);
    }
  }
  return Undefined(env);
}

napi_value SessionGoaway(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr && wrap->session != nullptr) {
    uint32_t code = NGHTTP2_NO_ERROR;
    int32_t last_stream_id = 0;
    if (argc >= 1) (void)napi_get_value_uint32(env, argv[0], &code);
    if (argc >= 2) (void)napi_get_value_int32(env, argv[1], &last_stream_id);
    const uint8_t* data = nullptr;
    size_t len = 0;
    std::string temp_utf8;
    if (argc >= 3) (void)GetByteSpan(env, argv[2], &data, &len, &temp_utf8);
    if (last_stream_id <= 0) last_stream_id = nghttp2_session_get_last_proc_stream_id(wrap->session);
    if (nghttp2_submit_goaway(wrap->session, NGHTTP2_FLAG_NONE, last_stream_id, code, data, len) == 0) {
      wrap->goaway_pending = true;
      if (!wrap->parent_write_in_progress && !wrap->in_scope) {
        (void)FlushSessionOutput(wrap);
      } else {
        MaybeScheduleSessionFlush(wrap);
      }
    }
  }
  return Undefined(env);
}

napi_value SessionHasPendingData(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  bool has_pending = SessionHasPendingDataNow(wrap);
  napi_value out = nullptr;
  napi_get_boolean(env, has_pending, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionSettings(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  Http2BindingState* state = GetHttp2State(env);
  bool ok = true;
  PendingSettings pending;
  bool has_pending = false;
  if (wrap != nullptr && wrap->session != nullptr && state != nullptr) {
    if (argc >= 1 && IsFunction(env, argv[0])) {
      if (napi_create_reference(env, argv[0], 1, &pending.callback_ref) != napi_ok ||
          pending.callback_ref == nullptr ||
          !InitPendingAsyncResource(env, "HTTP2SETTINGS", &pending.resource_ref, &pending.async_id)) {
        DeletePendingSettings(env, &pending);
        ok = false;
      } else {
        pending.start_time = uv_hrtime();
        has_pending = true;
      }
    }
    if (ok && wrap->pending_settings.size() >= wrap->max_outstanding_settings) {
      if (has_pending) CompletePendingSettings(env, &pending, false);
      ok = false;
    }
    if (!ok) {
      napi_value out = nullptr;
      napi_get_boolean(env, false, &out);
      return out != nullptr ? out : Undefined(env);
    }
    std::vector<nghttp2_settings_entry> entries;
    ok = ParseSettingsPayload(*state, &entries);
    if (ok) {
      UpdateLocalCustomSettings(wrap, entries);
      ok = nghttp2_submit_settings(wrap->session, NGHTTP2_FLAG_NONE, entries.data(), entries.size()) == 0;
      if (ok && has_pending) wrap->pending_settings.push_back(std::move(pending));
      if (ok) MaybeScheduleSessionFlush(wrap);
    }
    if (!ok && has_pending) DeletePendingSettings(env, &pending);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionRequest(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap == nullptr || wrap->session == nullptr || argc < 2) return Undefined(env);

  std::vector<nghttp2_nv> headers;
  std::vector<std::string> storage;
  if (!ParsePackedHeaders(env, argv[0], &headers, &storage)) return Undefined(env);

  int32_t options = 0;
  int32_t parent = 0;
  int32_t weight = NGHTTP2_DEFAULT_WEIGHT;
  bool exclusive = false;
  (void)napi_get_value_int32(env, argv[1], &options);
  if (argc >= 3) (void)napi_get_value_int32(env, argv[2], &parent);
  if (argc >= 4) (void)napi_get_value_int32(env, argv[3], &weight);
  if (argc >= 5) (void)napi_get_value_bool(env, argv[4], &exclusive);

  napi_value stream_obj = nullptr;
  Http2StreamWrap* stream = nullptr;
  if (!CreateStreamHandle(wrap, 0, NGHTTP2_HCAT_HEADERS, &stream_obj, &stream)) return Undefined(env);

  nghttp2_priority_spec priority;
  nghttp2_priority_spec_init(&priority, parent, weight, exclusive ? 1 : 0);

  nghttp2_data_provider provider{};
  nghttp2_data_provider* provider_ptr = nullptr;
  if ((options & kStreamOptionEmptyPayload) == 0) {
    provider.source.ptr = stream;
    provider.read_callback = OnDataSourceRead;
    provider_ptr = &provider;
  } else {
    stream->shutdown_requested = true;
    stream->local_closed = true;
  }
  stream->has_trailers = (options & kStreamOptionGetTrailers) != 0;
  DebugSession(wrap,
               "submitting request empty_payload=%s trailers=%s",
               (options & kStreamOptionEmptyPayload) != 0 ? "true" : "false",
               stream->has_trailers ? "true" : "false");

  const int32_t stream_id =
      nghttp2_submit_request(wrap->session, &priority, headers.data(), headers.size(), provider_ptr, stream);
  if (stream_id <= 0) {
    RemoveStreamFromSession(stream);
    napi_value out = nullptr;
    napi_create_int32(env, stream_id, &out);
    return out != nullptr ? out : Undefined(env);
  }

  wrap->streams.erase(0);
  wrap->streams[stream_id] = stream;
  stream->id = stream_id;
  MaybeScheduleSessionFlush(wrap);
  return stream_obj != nullptr ? stream_obj : Undefined(env);
}

napi_value SessionSetNextStreamId(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  bool ok = false;
  int32_t id = 0;
  if (wrap != nullptr && argc >= 1 && napi_get_value_int32(env, argv[0], &id) == napi_ok) {
    wrap->next_stream_id = id;
    ok = wrap->session == nullptr || nghttp2_session_set_next_stream_id(wrap->session, id) >= 0;
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionSetLocalWindowSize(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  int32_t window_size = 0;
  if (argc >= 1) (void)napi_get_value_int32(env, argv[0], &window_size);
  const int rc = wrap != nullptr && wrap->session != nullptr
                     ? nghttp2_session_set_local_window_size(wrap->session, NGHTTP2_FLAG_NONE, 0, window_size)
                     : 0;
  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionRefreshState(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  Http2BindingState* state = GetHttp2State(env);
  if (wrap != nullptr && state != nullptr && state->session_state != nullptr) {
    state->session_state[kSessionStateEffectiveLocalWindowSize] =
        wrap->session != nullptr ? nghttp2_session_get_effective_local_window_size(wrap->session)
                                 : kDefaultSettingsInitialWindowSize;
    state->session_state[kSessionStateEffectiveRecvDataLength] =
        wrap->session != nullptr ? nghttp2_session_get_effective_recv_data_length(wrap->session) : 0;
    state->session_state[kSessionStateNextStreamId] =
        wrap->session != nullptr ? nghttp2_session_get_next_stream_id(wrap->session) : wrap->next_stream_id;
    state->session_state[kSessionStateLocalWindowSize] =
        wrap->session != nullptr ? nghttp2_session_get_local_window_size(wrap->session)
                                 : kDefaultSettingsInitialWindowSize;
    state->session_state[kSessionStateLastProcStreamId] =
        wrap->session != nullptr ? nghttp2_session_get_last_proc_stream_id(wrap->session) : 0;
    state->session_state[kSessionStateRemoteWindowSize] =
        wrap->session != nullptr ? nghttp2_session_get_remote_window_size(wrap->session)
                                 : kDefaultSettingsInitialWindowSize;
    state->session_state[kSessionStateOutboundQueueSize] =
        wrap->session != nullptr ? nghttp2_session_get_outbound_queue_size(wrap->session) : 0;
    state->session_state[kSessionStateHdDeflateDynamicTableSize] =
        wrap->session != nullptr ? nghttp2_session_get_hd_deflate_dynamic_table_size(wrap->session)
                                 : kDefaultSettingsHeaderTableSize;
    state->session_state[kSessionStateHdInflateDynamicTableSize] =
        wrap->session != nullptr ? nghttp2_session_get_hd_inflate_dynamic_table_size(wrap->session)
                                 : kDefaultSettingsHeaderTableSize;
  }
  return Undefined(env);
}

napi_value SessionLocalSettings(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2BindingState* state = GetHttp2State(env);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (state != nullptr) FillDefaultSettingsBuffer(*state);
  if (state != nullptr && state->settings_buffer != nullptr && wrap != nullptr && wrap->session != nullptr) {
    state->settings_buffer[kSettingsHeaderTableSize] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_HEADER_TABLE_SIZE);
    state->settings_buffer[kSettingsEnablePush] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_ENABLE_PUSH);
    state->settings_buffer[kSettingsMaxConcurrentStreams] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS);
    state->settings_buffer[kSettingsInitialWindowSize] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE);
    state->settings_buffer[kSettingsMaxFrameSize] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_MAX_FRAME_SIZE);
    state->settings_buffer[kSettingsMaxHeaderListSize] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE);
    state->settings_buffer[kSettingsEnableConnectProtocol] =
        nghttp2_session_get_local_settings(wrap->session, NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL);
    FillSettingsBufferCustomEntries(state, wrap->local_custom_settings);
  }
  return Undefined(env);
}

napi_value SessionRemoteSettings(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2BindingState* state = GetHttp2State(env);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (state != nullptr) FillDefaultSettingsBuffer(*state);
  if (state != nullptr && state->settings_buffer != nullptr && wrap != nullptr && wrap->session != nullptr) {
    state->settings_buffer[kSettingsHeaderTableSize] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_HEADER_TABLE_SIZE);
    state->settings_buffer[kSettingsEnablePush] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_ENABLE_PUSH);
    state->settings_buffer[kSettingsMaxConcurrentStreams] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS);
    state->settings_buffer[kSettingsInitialWindowSize] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_INITIAL_WINDOW_SIZE);
    state->settings_buffer[kSettingsMaxFrameSize] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_MAX_FRAME_SIZE);
    state->settings_buffer[kSettingsMaxHeaderListSize] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_MAX_HEADER_LIST_SIZE);
    state->settings_buffer[kSettingsEnableConnectProtocol] =
        nghttp2_session_get_remote_settings(wrap->session, NGHTTP2_SETTINGS_ENABLE_CONNECT_PROTOCOL);
    FillSettingsBufferCustomEntries(state, wrap->remote_custom_settings);
  }
  return Undefined(env);
}

napi_value SessionPing(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  bool ok = true;
  PendingPing pending;
  bool has_pending = false;
  if (wrap != nullptr && wrap->session != nullptr) {
    pending.payload.fill(0);
    if (argc >= 2 && IsFunction(env, argv[1])) {
      if (napi_create_reference(env, argv[1], 1, &pending.callback_ref) != napi_ok ||
          pending.callback_ref == nullptr ||
          !InitPendingAsyncResource(env, "HTTP2PING", &pending.resource_ref, &pending.async_id)) {
        DeletePendingPing(env, &pending);
        ok = false;
      } else {
        pending.start_time = uv_hrtime();
        has_pending = true;
      }
    }
    if (!ok) {
      napi_value out = nullptr;
      napi_get_boolean(env, false, &out);
      return out != nullptr ? out : Undefined(env);
    }
    const uint8_t* payload = nullptr;
    size_t payload_len = 0;
    std::string temp_utf8;
    if (argc >= 1 && argv[0] != nullptr &&
        GetByteSpan(env, argv[0], &payload, &payload_len, &temp_utf8) &&
        payload != nullptr) {
      const size_t copy_len = std::min<size_t>(payload_len, pending.payload.size());
      std::memcpy(pending.payload.data(), payload, copy_len);
    } else if (has_pending) {
      std::memcpy(pending.payload.data(), &pending.start_time, pending.payload.size());
    }
    if (wrap->pending_pings.size() >= wrap->max_outstanding_pings) {
      if (has_pending) CompletePendingPing(env, &pending, false);
      ok = false;
    }
    if (ok) {
      ok = nghttp2_submit_ping(wrap->session, NGHTTP2_FLAG_NONE, pending.payload.data()) == 0;
      if (ok && has_pending) wrap->pending_pings.push_back(std::move(pending));
      if (ok) MaybeScheduleSessionFlush(wrap);
    }
    if (!ok && has_pending) DeletePendingPing(env, &pending);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, ok, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value SessionAltSvc(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr && wrap->session != nullptr) {
    int32_t stream_id = 0;
    if (argc >= 1) (void)napi_get_value_int32(env, argv[0], &stream_id);
    const uint8_t* origin = nullptr;
    size_t origin_len = 0;
    const uint8_t* alt = nullptr;
    size_t alt_len = 0;
    std::string temp_origin;
    std::string temp_alt;
    if (argc >= 2) (void)GetByteSpan(env, argv[1], &origin, &origin_len, &temp_origin);
    if (argc >= 3) (void)GetByteSpan(env, argv[2], &alt, &alt_len, &temp_alt);
    const int rc =
        nghttp2_submit_altsvc(wrap->session, NGHTTP2_FLAG_NONE, stream_id, origin, origin_len, alt, alt_len);
    if (rc == 0) MaybeScheduleSessionFlush(wrap);
  }
  return Undefined(env);
}

napi_value SessionOrigin(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr && wrap->session != nullptr && argc >= 2) {
    std::string packed;
    if (GetAsciiStringPreserveNulls(env, argv[0], &packed)) {
      const uint8_t* packed_data = reinterpret_cast<const uint8_t*>(packed.data());
      const size_t packed_len = packed.size();
      uint32_t count = 0;
      (void)napi_get_value_uint32(env, argv[1], &count);
      std::vector<nghttp2_origin_entry> entries;
      entries.reserve(count);
      size_t offset = 0;
      while (offset < packed_len && entries.size() < count) {
        const void* found = std::memchr(packed_data + offset, '\0', packed_len - offset);
        const size_t next = found != nullptr ? static_cast<const uint8_t*>(found) - packed_data : packed_len;
        const size_t len = next - offset;
        nghttp2_origin_entry entry{};
        entry.origin = const_cast<uint8_t*>(packed_data + offset);
        entry.origin_len = len;
        entries.push_back(entry);
        if (found == nullptr) break;
        offset = next + 1;
      }
      if (!entries.empty()) {
        const int rc = nghttp2_submit_origin(wrap->session, NGHTTP2_FLAG_NONE, entries.data(), entries.size());
        if (rc == 0) MaybeScheduleSessionFlush(wrap);
      }
    }
  }
  return Undefined(env);
}

napi_value SessionSetGracefulClose(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2SessionWrap* wrap = UnwrapSession(env, self);
  if (wrap != nullptr) {
    wrap->wants_graceful_close = true;
    if (wrap->parent_reading_stopped && !wrap->destroyed && !wrap->parent_write_in_progress &&
        wrap->parent_stream_base != nullptr) {
      const int rc = EdgeStreamBaseReadStart(wrap->parent_stream_base);
      if (rc == 0 || rc == UV_EALREADY) {
        wrap->parent_reading_stopped = false;
      }
    }
    MaybeNotifyGracefulCloseComplete(wrap);
  }
  return Undefined(env);
}

napi_value StreamGetId(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  napi_value out = nullptr;
  napi_create_int32(env, wrap != nullptr ? wrap->id : 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamDestroy(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap != nullptr) {
    DestroyStreamHandle(wrap);
  }
  return Undefined(env);
}

napi_value StreamDeprecatedPriority(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value StreamPushPromise(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || argc < 1) {
    return Undefined(env);
  }

  std::vector<nghttp2_nv> headers;
  std::vector<std::string> storage;
  if (!ParsePackedHeaders(env, argv[0], &headers, &storage)) return Undefined(env);

  int32_t options = 0;
  if (argc >= 2) (void)napi_get_value_int32(env, argv[1], &options);

  const int32_t stream_id = nghttp2_submit_push_promise(
      wrap->session->session, NGHTTP2_FLAG_NONE, wrap->id, headers.data(), headers.size(), nullptr);
  if (stream_id <= 0) {
    napi_value out = nullptr;
    napi_create_int32(env, stream_id, &out);
    return out != nullptr ? out : Undefined(env);
  }

  napi_value stream_obj = nullptr;
  Http2StreamWrap* pushed = nullptr;
  if (!CreateStreamHandle(wrap->session, stream_id, NGHTTP2_HCAT_HEADERS, &stream_obj, &pushed)) {
    napi_value out = nullptr;
    napi_create_int32(env, NGHTTP2_ERR_NOMEM, &out);
    return out != nullptr ? out : Undefined(env);
  }
  if ((options & kStreamOptionEmptyPayload) != 0) {
    pushed->shutdown_requested = true;
    pushed->local_closed = true;
  }
  MaybeScheduleSessionFlush(wrap->session);
  return stream_obj != nullptr ? stream_obj : Undefined(env);
}

napi_value StreamInfo(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || argc < 1) {
    return Undefined(env);
  }
  std::vector<nghttp2_nv> headers;
  std::vector<std::string> storage;
  if (!ParsePackedHeaders(env, argv[0], &headers, &storage)) return Undefined(env);
  const int rc = nghttp2_submit_headers(
      wrap->session->session, NGHTTP2_FLAG_NONE, wrap->id, nullptr, headers.data(), headers.size(), nullptr);
  if (rc == 0) MaybeScheduleSessionFlush(wrap->session);
  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamTrailers(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || argc < 1) {
    return Undefined(env);
  }
  std::vector<nghttp2_nv> headers;
  std::vector<std::string> storage;
  if (!ParsePackedHeaders(env, argv[0], &headers, &storage)) return Undefined(env);
  int rc = 0;
  if (headers.empty()) {
    nghttp2_data_provider provider{};
    provider.source.ptr = wrap;
    provider.read_callback = OnDataSourceRead;
    rc = nghttp2_submit_data(wrap->session->session, NGHTTP2_FLAG_END_STREAM, wrap->id, &provider);
  } else {
    rc = nghttp2_submit_trailer(wrap->session->session, wrap->id, headers.data(), headers.size());
  }
  if (rc == 0) MaybeScheduleSessionFlush(wrap->session);
  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamRespond(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || argc < 2) {
    return Undefined(env);
  }
  std::vector<nghttp2_nv> headers;
  std::vector<std::string> storage;
  if (!ParsePackedHeaders(env, argv[0], &headers, &storage)) return Undefined(env);
  int32_t options = 0;
  (void)napi_get_value_int32(env, argv[1], &options);
  wrap->has_trailers = (options & kStreamOptionGetTrailers) != 0;
  if (wrap->shutdown_requested || wrap->local_closed) {
    options |= kStreamOptionEmptyPayload;
  }

  nghttp2_data_provider provider{};
  nghttp2_data_provider* provider_ptr = nullptr;
  if ((options & kStreamOptionEmptyPayload) == 0) {
    provider.source.ptr = wrap;
    provider.read_callback = OnDataSourceRead;
    provider_ptr = &provider;
  } else {
    wrap->shutdown_requested = true;
    wrap->local_closed = true;
  }

  const int rc =
      nghttp2_submit_response(wrap->session->session, wrap->id, headers.data(), headers.size(), provider_ptr);
  if (rc == 0) MaybeScheduleSessionFlush(wrap->session);
  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamRstStream(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  uint32_t code = NGHTTP2_NO_ERROR;
  if (argc >= 1) (void)napi_get_value_uint32(env, argv[0], &code);
  if (wrap != nullptr && wrap->session != nullptr && wrap->session->session != nullptr) {
    wrap->rst_code = code;
    DebugStream(wrap, "rstStream code=%u in_scope=%s write_in_progress=%s",
                code,
                wrap->session->in_scope ? "true" : "false",
                wrap->session->parent_write_in_progress ? "true" : "false");
    if (wrap->session->in_scope) {
      AddPendingRstStream(wrap->session, wrap->id);
      MaybeScheduleSessionFlush(wrap->session);
      return Undefined(env);
    }
    if (wrap->session->parent_write_in_progress) {
      AddPendingRstStream(wrap->session, wrap->id);
      return Undefined(env);
    }
    const int rc = FlushSessionOutput(wrap->session);
    if (rc != 0) {
      AddPendingRstStream(wrap->session, wrap->id);
      return Undefined(env);
    }
    if (wrap->session->parent_write_in_progress) {
      AddPendingRstStream(wrap->session, wrap->id);
      return Undefined(env);
    }
    AddPendingRstStream(wrap->session, wrap->id);
    FlushPendingRstStreams(wrap->session);
  }
  return Undefined(env);
}

napi_value StreamRefreshState(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2BindingState* state = GetHttp2State(env);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (state != nullptr && state->stream_state != nullptr) {
    nghttp2_stream* stream = nullptr;
    nghttp2_session* session = nullptr;
    if (wrap != nullptr && wrap->session != nullptr && wrap->session->session != nullptr) {
      session = wrap->session->session;
      stream = nghttp2_session_find_stream(session, wrap->id);
    }
    if (stream == nullptr) {
      state->stream_state[kStreamStateState] = NGHTTP2_STREAM_STATE_IDLE;
      state->stream_state[kStreamStateWeight] = 0;
      state->stream_state[kStreamStateSumDependencyWeight] = 0;
      state->stream_state[kStreamStateLocalClose] = 0;
      state->stream_state[kStreamStateRemoteClose] = 0;
      state->stream_state[kStreamStateLocalWindowSize] = 0;
    } else {
      state->stream_state[kStreamStateState] = nghttp2_stream_get_state(stream);
      state->stream_state[kStreamStateWeight] = nghttp2_stream_get_weight(stream);
      state->stream_state[kStreamStateSumDependencyWeight] = nghttp2_stream_get_sum_dependency_weight(stream);
      state->stream_state[kStreamStateLocalClose] = nghttp2_session_get_stream_local_close(session, wrap->id);
      state->stream_state[kStreamStateRemoteClose] = nghttp2_session_get_stream_remote_close(session, wrap->id);
      state->stream_state[kStreamStateLocalWindowSize] = nghttp2_session_get_stream_local_window_size(session, wrap->id);
    }
  }
  return Undefined(env);
}

napi_value StreamReadStart(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap != nullptr) {
    DebugStream(wrap, "reading starting");
    wrap->reading = true;
    FlushInboundChunks(wrap);
    if (wrap->close_pending && !wrap->destroyed && wrap->inbound_chunks.empty()) {
      const uint32_t error_code = wrap->pending_close_code;
      wrap->close_pending = false;
      wrap->pending_close_code = NGHTTP2_NO_ERROR;
      InvokeStreamCloseCallback(wrap, error_code);
    }
  }
  napi_value reading = nullptr;
  napi_get_boolean(env, true, &reading);
  if (reading != nullptr) napi_set_named_property(env, self, "reading", reading);
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamReadStop(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap != nullptr) {
    DebugStream(wrap, "reading stopped");
    wrap->reading = false;
  }
  napi_value reading = nullptr;
  napi_get_boolean(env, false, &reading);
  if (reading != nullptr) napi_set_named_property(env, self, "reading", reading);
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamShutdown(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  int32_t status = 1;
  if (wrap == nullptr || wrap->session == nullptr || wrap->session->session == nullptr || wrap->destroyed) {
    status = UV_EPIPE;
  } else {
    wrap->shutdown_requested = true;
    (void)nghttp2_session_resume_data(wrap->session->session, wrap->id);
    MaybeScheduleSessionFlush(wrap->session);
  }
  napi_value out = nullptr;
  napi_create_int32(env, status, &out);
  return out != nullptr ? out : Undefined(env);
}

size_t SumWritevBytes(napi_env env, napi_value chunks, bool all_buffers) {
  bool is_array = false;
  uint32_t length = 0;
  if (napi_is_array(env, chunks, &is_array) != napi_ok || !is_array || napi_get_array_length(env, chunks, &length) != napi_ok) {
    return 0;
  }

  size_t total = 0;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value entry = nullptr;
    if (napi_get_element(env, chunks, i, &entry) != napi_ok || entry == nullptr) continue;
    const uint8_t* data = nullptr;
    size_t len = 0;
    std::string temp_utf8;
    napi_value payload = entry;
    if (!all_buffers) {
      const uint32_t value_index = (i * 2);
      if (napi_get_element(env, chunks, value_index, &payload) != napi_ok || payload == nullptr) continue;
      i++;
    }
    if (GetByteSpan(env, payload, &data, &len, &temp_utf8)) total += len;
  }
  return total;
}

napi_value StreamWriteBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  const uint8_t* data = nullptr;
  size_t len = 0;
  std::string temp_utf8;
  if (argc >= 2) (void)GetByteSpan(env, argv[1], &data, &len, &temp_utf8);
  if (argc >= 1 && argv[0] != nullptr) {
    EdgeStreamReqActivate(env, argv[0], kEdgeProviderWriteWrap, wrap != nullptr ? wrap->async_id : -1);
  }
  SetStreamWriteState(env, len, true);
  if (wrap != nullptr && wrap->session != nullptr && wrap->session->session != nullptr) {
    QueueOutboundChunk(wrap, argc >= 1 ? argv[0] : nullptr, data, len);
    (void)nghttp2_session_resume_data(wrap->session->session, wrap->id);
    MaybeScheduleSessionFlush(wrap->session);
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamWriteStringWithEncoding(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  const uint8_t* data = nullptr;
  size_t len = 0;
  std::string temp_utf8;
  if (argc >= 2) (void)GetByteSpan(env, argv[1], &data, &len, &temp_utf8);
  if (argc >= 1 && argv[0] != nullptr) {
    EdgeStreamReqActivate(env, argv[0], kEdgeProviderWriteWrap, wrap != nullptr ? wrap->async_id : -1);
  }
  SetStreamWriteState(env, len, true);
  if (wrap != nullptr && wrap->session != nullptr && wrap->session->session != nullptr) {
    QueueOutboundChunk(wrap, argc >= 1 ? argv[0] : nullptr, data, len);
    (void)nghttp2_session_resume_data(wrap->session->session, wrap->id);
    MaybeScheduleSessionFlush(wrap->session);
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamWritev(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  bool all_buffers = false;
  if (argc >= 3) (void)napi_get_value_bool(env, argv[2], &all_buffers);
  const size_t total = argc >= 2 ? SumWritevBytes(env, argv[1], all_buffers) : 0;
  if (argc >= 1 && argv[0] != nullptr) {
    EdgeStreamReqActivate(env, argv[0], kEdgeProviderWriteWrap, wrap != nullptr ? wrap->async_id : -1);
  }
  SetStreamWriteState(env, total, true);
  if (wrap != nullptr && wrap->session != nullptr && wrap->session->session != nullptr && argc >= 2) {
    bool is_array = false;
    uint32_t raw_len = 0;
    if (napi_is_array(env, argv[1], &is_array) == napi_ok && is_array &&
        napi_get_array_length(env, argv[1], &raw_len) == napi_ok) {
      const uint32_t count = all_buffers ? raw_len : raw_len / 2;
      for (uint32_t i = 0; i < count; ++i) {
        napi_value chunk = nullptr;
        napi_get_element(env, argv[1], all_buffers ? i : i * 2, &chunk);
        const uint8_t* data = nullptr;
        size_t len = 0;
        std::string temp_utf8;
        if (GetByteSpan(env, chunk, &data, &len, &temp_utf8)) {
          QueueOutboundChunk(wrap,
                             (argc >= 1 && argv[0] != nullptr && i == count - 1) ? argv[0] : nullptr,
                             data,
                             len);
        }
      }
      (void)nghttp2_session_resume_data(wrap->session->session, wrap->id);
      MaybeScheduleSessionFlush(wrap->session);
    }
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  napi_value out = nullptr;
  napi_create_int64(env, wrap != nullptr ? wrap->async_id : -1, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamGetOnRead(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  napi_value out = wrap != nullptr ? GetRefValue(env, wrap->onread_ref) : nullptr;
  return out != nullptr ? out : Undefined(env);
}

napi_value StreamSetOnRead(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);
  Http2StreamWrap* wrap = UnwrapStream(env, self);
  if (wrap != nullptr) {
    napi_value onread_value = argc >= 1 ? argv[0] : Undefined(env);
    (void)EdgeStreamBaseSetOnRead(&wrap->base, onread_value);
    DeleteRefIfPresent(env, &wrap->onread_ref);
    if (argc >= 1 && IsFunction(env, argv[0])) {
      (void)napi_create_reference(env, argv[0], 1, &wrap->onread_ref);
      FlushInboundChunks(wrap);
    }
  }
  return Undefined(env);
}

napi_value SessionCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  auto* wrap = new Http2SessionWrap();
  wrap->env = env;
  wrap->async_id = EdgeAsyncWrapNextId(env);
  if (argc >= 1) {
    int32_t type = kSessionTypeServer;
    if (napi_get_value_int32(env, argv[0], &type) == napi_ok) {
      wrap->type = type;
      wrap->next_stream_id = type == kSessionTypeClient ? 1 : 2;
    }
  }

  if (nghttp2_session_callbacks_new(&wrap->callbacks) != 0 || wrap->callbacks == nullptr) {
    delete wrap;
    return nullptr;
  }
  nghttp2_session_callbacks_set_on_begin_headers_callback(wrap->callbacks, OnBeginHeaders);
  nghttp2_session_callbacks_set_on_header_callback2(wrap->callbacks, OnHeaderRcbuf);
  nghttp2_session_callbacks_set_on_frame_recv_callback(wrap->callbacks, OnFrameRecv);
  nghttp2_session_callbacks_set_on_frame_not_send_callback(wrap->callbacks, OnFrameNotSend);
  nghttp2_session_callbacks_set_on_stream_close_callback(wrap->callbacks, OnStreamClose);
  nghttp2_session_callbacks_set_on_data_chunk_recv_callback(wrap->callbacks, OnDataChunkRecv);
  nghttp2_session_callbacks_set_on_invalid_header_callback2(wrap->callbacks, OnInvalidHeader);
  nghttp2_session_callbacks_set_error_callback2(wrap->callbacks, OnNghttpError);
  nghttp2_session_callbacks_set_send_data_callback(wrap->callbacks, OnSendData);
  nghttp2_session_callbacks_set_on_invalid_frame_recv_callback(wrap->callbacks, OnInvalidFrameRecv);
  nghttp2_session_callbacks_set_on_frame_send_callback(wrap->callbacks, OnFrameSent);

  nghttp2_option* option = nullptr;
  (void)nghttp2_option_new(&option);
  if (option != nullptr) {
    nghttp2_option_set_no_closed_streams(option, 1);
    nghttp2_option_set_no_auto_window_update(option, 1);
#if defined(__wasi__) || defined(__wasm32__)
    nghttp2_option_set_max_outbound_ack(option, 512);
#endif
    if (wrap->type == kSessionTypeClient) {
      nghttp2_option_set_builtin_recv_extension_type(option, NGHTTP2_ALTSVC);
      nghttp2_option_set_builtin_recv_extension_type(option, NGHTTP2_ORIGIN);
    }
  }
  Http2BindingState* state = GetHttp2State(env);
  if (state != nullptr) {
    FetchAllowedRemoteCustomSettings(wrap, state);
  }
  if (option != nullptr && state != nullptr && state->options_buffer != nullptr) {
    const uint32_t flags = state->options_buffer[kOptionsFlags];

    if ((flags & (1u << kOptionsMaxDeflateDynamicTableSize)) != 0) {
      nghttp2_option_set_max_deflate_dynamic_table_size(
          option, state->options_buffer[kOptionsMaxDeflateDynamicTableSize]);
    }
    if ((flags & (1u << kOptionsMaxReservedRemoteStreams)) != 0) {
      nghttp2_option_set_max_reserved_remote_streams(option, state->options_buffer[kOptionsMaxReservedRemoteStreams]);
    }
    nghttp2_option_set_peer_max_concurrent_streams(option, 100);
    if ((flags & (1u << kOptionsPeerMaxConcurrentStreams)) != 0) {
      nghttp2_option_set_peer_max_concurrent_streams(option, state->options_buffer[kOptionsPeerMaxConcurrentStreams]);
    }
    if ((flags & (1u << kOptionsMaxSendHeaderBlockLength)) != 0) {
      nghttp2_option_set_max_send_header_block_length(
          option, state->options_buffer[kOptionsMaxSendHeaderBlockLength]);
    }
    if ((flags & (1u << kOptionsStrictFieldWhitespaceValidation)) != 0) {
      nghttp2_option_set_no_rfc9113_leading_and_trailing_ws_validation(
          option, state->options_buffer[kOptionsStrictFieldWhitespaceValidation]);
    }
    if ((flags & (1u << kOptionsMaxSettings)) != 0) {
      nghttp2_option_set_max_settings(option, static_cast<size_t>(state->options_buffer[kOptionsMaxSettings]));
    }
    if ((flags & (1u << kOptionsStreamResetBurst)) != 0 && (flags & (1u << kOptionsStreamResetRate)) != 0) {
      nghttp2_option_set_stream_reset_rate_limit(option,
                                                 static_cast<uint64_t>(state->options_buffer[kOptionsStreamResetBurst]),
                                                 static_cast<uint64_t>(state->options_buffer[kOptionsStreamResetRate]));
    }

    wrap->padding_strategy = (flags & (1u << kOptionsPaddingStrategy)) != 0
                                 ? state->options_buffer[kOptionsPaddingStrategy]
                                 : 0;
    if (wrap->padding_strategy != 0) {
      nghttp2_session_callbacks_set_select_padding_callback(wrap->callbacks, OnSelectPadding);
    }

    size_t header_pairs = (flags & (1u << kOptionsMaxHeaderListPairs)) != 0
                              ? state->options_buffer[kOptionsMaxHeaderListPairs]
                              : kDefaultMaxHeaderListPairs;
    wrap->max_header_pairs = wrap->type == kSessionTypeServer ? std::max<size_t>(header_pairs, 4) : std::max<size_t>(header_pairs, 1);
    wrap->max_header_length = state->settings_buffer[kSettingsMaxHeaderListSize] > 0
                                  ? state->settings_buffer[kSettingsMaxHeaderListSize]
                                  : kDefaultSettingsMaxHeaderListSize;
    if ((flags & (1u << kOptionsMaxOutstandingPings)) != 0) {
      wrap->max_outstanding_pings = state->options_buffer[kOptionsMaxOutstandingPings];
    }
    if ((flags & (1u << kOptionsMaxOutstandingSettings)) != 0) {
      wrap->max_outstanding_settings = std::max<size_t>(state->options_buffer[kOptionsMaxOutstandingSettings], 1);
    }
    if ((flags & (1u << kOptionsMaxSessionMemory)) != 0) {
      wrap->max_session_memory = static_cast<uint64_t>(state->options_buffer[kOptionsMaxSessionMemory]) * 1000000ULL;
    }
  }

  const int create_rc = wrap->type == kSessionTypeServer
                            ? nghttp2_session_server_new3(&wrap->session, wrap->callbacks, wrap, option, nullptr)
                            : nghttp2_session_client_new3(&wrap->session, wrap->callbacks, wrap, option, nullptr);
  if (option != nullptr) nghttp2_option_del(option);
  if (create_rc != 0 || wrap->session == nullptr) {
    if (wrap->callbacks != nullptr) nghttp2_session_callbacks_del(wrap->callbacks);
    delete wrap;
    return nullptr;
  }

  if (napi_wrap(env, self, wrap, Http2SessionFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    nghttp2_session_del(wrap->session);
    nghttp2_session_callbacks_del(wrap->callbacks);
    delete wrap;
    return nullptr;
  }
  EdgeAsyncWrapEmitInitString(env, wrap->async_id, "HTTP2SESSION", EdgeAsyncWrapExecutionAsyncId(env), self);

  void* fields_data = nullptr;
  napi_value fields_ab = nullptr;
  if (napi_create_arraybuffer(env, kSessionUint8FieldCount, &fields_data, &fields_ab) != napi_ok ||
      fields_ab == nullptr ||
      fields_data == nullptr) {
    return nullptr;
  }
  std::memset(fields_data, 0, kSessionUint8FieldCount);
  auto* fields = static_cast<SessionJSFields*>(fields_data);
  fields->max_invalid_frames = 1000;
  fields->max_rejected_streams = 100;
  wrap->fields = fields;

  napi_value fields_ta = nullptr;
  if (napi_create_typedarray(env,
                             napi_uint8_array,
                             kSessionUint8FieldCount,
                             fields_ab,
                             0,
                             &fields_ta) != napi_ok ||
      fields_ta == nullptr) {
    return nullptr;
  }
  (void)napi_create_reference(env, fields_ta, 1, &wrap->fields_ref);
  (void)napi_set_named_property(env, self, "ondone", Undefined(env));
  (void)napi_set_named_property(env, self, "ongracefulclosecomplete", Undefined(env));
  (void)napi_set_named_property(env, self, "chunksSentSinceLastWrite", EdgeStreamBaseMakeDouble(env, 0));
  return self;
}

napi_value StreamCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  auto* wrap = new Http2StreamWrap();
  wrap->env = env;
  wrap->async_id = EdgeAsyncWrapNextId(env);
  EdgeStreamBaseInit(&wrap->base, env, &kHttp2StreamOps, kEdgeProviderNone);
  if (argc >= 1) {
    int32_t id = 0;
    if (napi_get_value_int32(env, argv[0], &id) == napi_ok) wrap->id = id;
  }

  if (napi_wrap(env, self, wrap, Http2StreamFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  EdgeStreamBaseSetWrapperRef(&wrap->base, wrap->wrapper_ref);
  EdgeAsyncWrapEmitInitString(env, wrap->async_id, "HTTP2STREAM", EdgeAsyncWrapExecutionAsyncId(env), self);

  napi_value false_value = nullptr;
  napi_value true_value = nullptr;
  napi_get_boolean(env, false, &false_value);
  napi_get_boolean(env, true, &true_value);
  (void)napi_set_named_property(env, self, "isStreamBase", true_value);
  (void)napi_set_named_property(env, self, "reading", false_value);
  (void)napi_set_named_property(env, self, "onread", Undefined(env));
  napi_value external = EdgeStreamBaseGetExternal(&wrap->base);
  if (external != nullptr) {
    (void)napi_set_named_property(env, self, "_externalStream", external);
  }
  return self;
}

napi_value SetCallbackFunctionsCallback(napi_env env, napi_callback_info info) {
  size_t argc = kCallbackCount;
  napi_value argv[kCallbackCount] = {};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  Http2BindingState& state = EnsureHttp2State(env);
  for (size_t i = 0; i < kCallbackCount; ++i) {
    DeleteRefIfPresent(env, &state.callback_refs[i]);
    if (i < argc && IsFunction(env, argv[i])) {
      (void)napi_create_reference(env, argv[i], 1, &state.callback_refs[i]);
    }
  }
  return Undefined(env);
}

napi_value RefreshDefaultSettingsCallback(napi_env env, napi_callback_info /*info*/) {
  Http2BindingState* state = GetHttp2State(env);
  if (state != nullptr) FillDefaultSettingsBuffer(*state);
  return Undefined(env);
}

napi_value PackSettingsCallback(napi_env env, napi_callback_info /*info*/) {
  Http2BindingState* state = GetHttp2State(env);
  if (state == nullptr) return Undefined(env);

  std::vector<nghttp2_settings_entry> entries;
  if (!ParseSettingsPayload(*state, &entries)) return Undefined(env);
  const size_t byte_length = entries.size() * 6;
  std::vector<uint8_t> payload(byte_length);
  if (nghttp2_pack_settings_payload(payload.data(), payload.size(), entries.data(), entries.size()) < 0) {
    return Undefined(env);
  }

  napi_value out = nullptr;
  void* copy = nullptr;
  if (napi_create_buffer_copy(env, payload.size(), payload.data(), &copy, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

napi_value Nghttp2ErrorStringCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  int32_t code = 0;
  if (argc >= 1) (void)napi_get_value_int32(env, argv[0], &code);
  napi_value out = nullptr;
  napi_create_string_utf8(env, nghttp2_strerror(code), NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

void PopulateConstantsObject(napi_env env, napi_value constants) {
  for (const auto& entry : kHiddenNumericConstants) {
    (void)SetNamedInt64(env, constants, entry.name, entry.value);
  }
  for (const auto& entry : kNumericConstants) {
    (void)SetNamedInt64(env, constants, entry.name, entry.value);
  }
  for (const auto& entry : kHeaderConstants) {
    (void)SetNamedString(env, constants, entry.name, entry.value);
  }
  for (const auto& entry : kMethodConstants) {
    (void)SetNamedString(env, constants, entry.name, entry.value);
  }
  for (const auto& entry : kStatusConstants) {
    (void)SetNamedInt64(env, constants, entry.name, entry.value);
  }
}

bool InstallSessionClass(napi_env env, napi_value binding, Http2BindingState* state) {
  napi_property_descriptor props[] = {
      {"origin", nullptr, SessionOrigin, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"altsvc", nullptr, SessionAltSvc, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"ping", nullptr, SessionPing, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"consume", nullptr, SessionConsume, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"receive", nullptr, SessionReceive, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"destroy", nullptr, SessionDestroy, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"goaway", nullptr, SessionGoaway, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"hasPendingData", nullptr, SessionHasPendingData, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"settings", nullptr, SessionSettings, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"request", nullptr, SessionRequest, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setNextStreamID", nullptr, SessionSetNextStreamId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setLocalWindowSize", nullptr, SessionSetLocalWindowSize, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"updateChunksSent", nullptr, SessionUpdateChunksSent, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"refreshState", nullptr, SessionRefreshState, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"localSettings", nullptr, SessionLocalSettings, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"remoteSettings", nullptr, SessionRemoteSettings, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"setGracefulClose", nullptr, SessionSetGracefulClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"fields", nullptr, nullptr, SessionGetFields, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "Http2Session",
                        NAPI_AUTO_LENGTH,
                        SessionCtor,
                        nullptr,
                        sizeof(props) / sizeof(props[0]),
                        props,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return false;
  }
  DeleteRefIfPresent(env, &state->session_ctor_ref);
  (void)napi_create_reference(env, ctor, 1, &state->session_ctor_ref);
  return napi_set_named_property(env, binding, "Http2Session", ctor) == napi_ok;
}

bool InstallStreamClass(napi_env env, napi_value binding, Http2BindingState* state) {
  napi_property_descriptor props[] = {
      {"id", nullptr, StreamGetId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"destroy", nullptr, StreamDestroy, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"priority", nullptr, StreamDeprecatedPriority, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"pushPromise", nullptr, StreamPushPromise, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"info", nullptr, StreamInfo, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"trailers", nullptr, StreamTrailers, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"respond", nullptr, StreamRespond, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"rstStream", nullptr, StreamRstStream, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"refreshState", nullptr, StreamRefreshState, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStart", nullptr, StreamReadStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"readStop", nullptr, StreamReadStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"shutdown", nullptr, StreamShutdown, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writev", nullptr, StreamWritev, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeBuffer", nullptr, StreamWriteBuffer, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"writeAsciiString", nullptr, StreamWriteStringWithEncoding, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"writeUtf8String", nullptr, StreamWriteStringWithEncoding, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"writeUcs2String", nullptr, StreamWriteStringWithEncoding, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"writeLatin1String", nullptr, StreamWriteStringWithEncoding, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"getAsyncId", nullptr, StreamGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"onread", nullptr, nullptr, StreamGetOnRead, StreamSetOnRead, nullptr, napi_default, nullptr},
  };
  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "Http2Stream",
                        NAPI_AUTO_LENGTH,
                        StreamCtor,
                        nullptr,
                        sizeof(props) / sizeof(props[0]),
                        props,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return false;
  }
  DeleteRefIfPresent(env, &state->stream_ctor_ref);
  (void)napi_create_reference(env, ctor, 1, &state->stream_ctor_ref);
  return napi_set_named_property(env, binding, "Http2Stream", ctor) == napi_ok;
}

}  // namespace

napi_value InitHttp2(napi_env env) {
  Http2BindingState& state = EnsureHttp2State(env);
  napi_value cached = GetRefValue(env, state.binding_ref);
  if (cached != nullptr) return cached;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  napi_value session_state = nullptr;
  napi_value stream_state = nullptr;
  napi_value settings_buffer = nullptr;
  napi_value options_buffer = nullptr;
  napi_value stream_stats = nullptr;
  napi_value session_stats = nullptr;
  if (!CreateTypedArray(env, napi_float64_array, kSessionStateCount, &state.session_state, &session_state) ||
      !CreateTypedArray(env, napi_float64_array, kStreamStateCount, &state.stream_state, &stream_state) ||
      !CreateTypedArray(env, napi_uint32_array, kSettingsBufferLength, &state.settings_buffer, &settings_buffer) ||
      !CreateTypedArray(env, napi_uint32_array, kOptionsBufferLength, &state.options_buffer, &options_buffer) ||
      !CreateTypedArray(env, napi_float64_array, kStreamStatsCount, &state.stream_stats, &stream_stats) ||
      !CreateTypedArray(env, napi_float64_array, kSessionStatsCount, &state.session_stats, &session_stats)) {
    return Undefined(env);
  }

  FillDefaultSettingsBuffer(state);

  napi_set_named_property(env, binding, "sessionState", session_state);
  napi_set_named_property(env, binding, "streamState", stream_state);
  napi_set_named_property(env, binding, "settingsBuffer", settings_buffer);
  napi_set_named_property(env, binding, "optionsBuffer", options_buffer);
  napi_set_named_property(env, binding, "streamStats", stream_stats);
  napi_set_named_property(env, binding, "sessionStats", session_stats);

  for (const auto& entry : kHiddenNumericConstants) {
    if (std::strncmp(entry.name, "k", 1) == 0) continue;
    (void)SetNamedInt64(env, binding, entry.name, entry.value);
  }
  for (const auto& entry : kSessionFieldConstants) {
    (void)SetNamedInt64(env, binding, entry.name, entry.value);
  }
  for (const auto& entry : kSessionFieldConstantsTail) {
    (void)SetNamedInt64(env, binding, entry.name, entry.value);
  }
  (void)SetNamedInt64(env, binding, "kBitfield", kBitfield);
  (void)SetNamedInt64(env, binding, "kSessionPriorityListenerCount", kSessionPriorityListenerCount);

  napi_value name_for_error_code = nullptr;
  if (napi_create_array_with_length(env, kErrorCodeNames.size(), &name_for_error_code) == napi_ok &&
      name_for_error_code != nullptr) {
    for (uint32_t i = 0; i < kErrorCodeNames.size(); ++i) {
      napi_value entry = nullptr;
      if (napi_create_string_utf8(env, kErrorCodeNames[i], NAPI_AUTO_LENGTH, &entry) == napi_ok && entry != nullptr) {
        napi_set_element(env, name_for_error_code, i, entry);
      }
    }
    napi_set_named_property(env, binding, "nameForErrorCode", name_for_error_code);
  }

  napi_value constants = nullptr;
  if (napi_create_object(env, &constants) != napi_ok || constants == nullptr) return Undefined(env);
  PopulateConstantsObject(env, constants);
  napi_set_named_property(env, binding, "constants", constants);

  if (!DefineMethod(env, binding, "nghttp2ErrorString", Nghttp2ErrorStringCallback) ||
      !DefineMethod(env, binding, "refreshDefaultSettings", RefreshDefaultSettingsCallback) ||
      !DefineMethod(env, binding, "packSettings", PackSettingsCallback) ||
      !DefineMethod(env, binding, "setCallbackFunctions", SetCallbackFunctionsCallback) ||
      !InstallSessionClass(env, binding, &state) ||
      !InstallStreamClass(env, binding, &state)) {
    return Undefined(env);
  }

  DeleteRefIfPresent(env, &state.binding_ref);
  (void)napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}

}  // namespace internal_binding
