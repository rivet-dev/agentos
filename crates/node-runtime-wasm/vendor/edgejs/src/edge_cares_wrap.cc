#include "edge_cares_wrap.h"

#include <arpa/inet.h>
#include <netdb.h>

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include <uv.h>

#include "ada.h"
#include "cares/include/ares.h"
#include "cares/include/ares_dns.h"
#include "cares/include/ares_dns_record.h"
#include "cares/include/ares_nameser.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_runtime.h"

#ifndef AI_ALL
#define AI_ALL 0
#endif

#ifndef AI_V4MAPPED
#define AI_V4MAPPED 0
#endif

namespace {

constexpr int kDnsOrderVerbatim = 0;
constexpr int kDnsOrderIpv4First = 1;
constexpr int kDnsOrderIpv6First = 2;
constexpr int kDnsESetSrvPending = -1000;
constexpr int ns_t_cname_or_a = -1;

struct ChannelWrap;
struct NodeAresTask;

enum class QueryKind {
  kAny,
  kA,
  kAaaa,
  kCaa,
  kCname,
  kMx,
  kNs,
  kTlsa,
  kTxt,
  kSrv,
  kPtr,
  kNaptr,
  kSoa,
  kReverse,
};

struct CaresReqWrap {
  napi_env env = nullptr;
  napi_ref req_obj_ref = nullptr;
  void* active_request_token = nullptr;
  int64_t async_id = -1;
  bool destroy_queued = false;
  uv_getaddrinfo_t ga{};
  uv_getnameinfo_t gn{};
  bool used_ga = false;
  bool used_gn = false;
  bool used_query = false;
  bool in_flight = false;
  bool pinned_ref = false;
  bool finalized = false;
  bool orphaned = false;
  uint8_t order = kDnsOrderVerbatim;
  int32_t family = 0;
  bool ttl = false;
  std::string hostname;
  std::string binding_name;
  std::string async_wrap_name = "QUERYWRAP";
  QueryKind query_kind = QueryKind::kA;
  ChannelWrap* channel = nullptr;
};

struct NodeAresTask {
  ChannelWrap* channel = nullptr;
  ares_socket_t sock = ARES_SOCKET_BAD;
  uv_poll_t poll_watcher{};
};

struct ChannelWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  ares_channel channel = nullptr;
  bool library_inited = false;
  bool query_last_ok = true;
  bool is_servers_default = true;
  int32_t timeout = -1;
  int32_t tries = 4;
  int32_t max_timeout = 0;
  int active_query_count = 0;
  bool finalized = false;
  bool destroy_scheduled = false;
  uv_timer_t* timer_handle = nullptr;
  std::unordered_map<ares_socket_t, NodeAresTask*> tasks;
  std::unordered_set<CaresReqWrap*> active_reqs;
};

struct QueryMethodData {
  const char* binding_name;
  QueryKind kind;
  ares_dns_rec_type_t rec_type;
};

struct HostentDeleter {
  void operator()(hostent* host) const {
    if (host != nullptr) ares_free_hostent(host);
  }
};

struct AresDataDeleter {
  void operator()(void* ptr) const {
    if (ptr != nullptr) ares_free_data(ptr);
  }
};

struct AresStringDeleter {
  void operator()(char* ptr) const {
    if (ptr != nullptr) ares_free_string(ptr);
  }
};

using HostentPtr = std::unique_ptr<hostent, HostentDeleter>;
using AresDataPtr = std::unique_ptr<void, AresDataDeleter>;
using AresStringPtr = std::unique_ptr<char, AresStringDeleter>;

struct CaresEnvState {
  explicit CaresEnvState(napi_env /*env_in*/) {}

  std::unordered_set<ChannelWrap*> channels;
  bool cleanup_in_progress = false;
};

int g_cares_library_refcount = 0;

CaresEnvState* GetCaresState(napi_env env) {
  return EdgeEnvironmentGetSlotData<CaresEnvState>(
      env, kEdgeEnvironmentSlotCaresChannelSet);
}

CaresEnvState& EnsureCaresState(napi_env env) {
  (void)EdgeEnsureEnvLoop(env, nullptr);
  return EdgeEnvironmentGetOrCreateSlotData<CaresEnvState>(
      env, kEdgeEnvironmentSlotCaresChannelSet);
}

napi_value GetRefValue(napi_env env, napi_ref ref);
napi_value GetCaresReqOwner(napi_env env, void* data);
void CancelCaresReq(void* data);
void EnsureReqAsyncWrap(CaresReqWrap* req, napi_value req_obj);
void CompleteQuery(CaresReqWrap* req,
                   int status,
                   const unsigned char* buf,
                   int len,
                   hostent* host,
                   bool from_host);
void CancelChannelRequests(ChannelWrap* channel);

bool EnvCleanupInProgress(napi_env env) {
  CaresEnvState* state = GetCaresState(env);
  return state != nullptr && state->cleanup_in_progress;
}

void RegisterChannelForEnv(ChannelWrap* channel) {
  if (channel == nullptr || channel->env == nullptr) return;
  EnsureCaresState(channel->env).channels.insert(channel);
}

void UnregisterChannelForEnv(ChannelWrap* channel) {
  if (channel == nullptr || channel->env == nullptr) return;
  if (EnvCleanupInProgress(channel->env)) return;
  CaresEnvState* state = GetCaresState(channel->env);
  if (state == nullptr) return;
  state->channels.erase(channel);
}

void DestroyChannelCares(ChannelWrap* channel);
void ScheduleFinalizedChannelDestroy(ChannelWrap* channel);
void StartChannelTimer(ChannelWrap* channel);

void DeleteChannelActiveRef(ChannelWrap* channel) {
  if (channel == nullptr || channel->env == nullptr || channel->active_ref == nullptr) return;
  napi_delete_reference(channel->env, channel->active_ref);
  channel->active_ref = nullptr;
}

void HoldChannelActiveRef(ChannelWrap* channel, napi_value self) {
  if (channel == nullptr || channel->env == nullptr || self == nullptr || channel->active_ref != nullptr) return;
  if (napi_create_reference(channel->env, self, 1, &channel->active_ref) != napi_ok) {
    channel->active_ref = nullptr;
  }
}

void TrackPendingReq(napi_env env, CaresReqWrap* req) {
  if (env == nullptr || req == nullptr) return;
  (void)EnsureCaresState(env);
  if (req->active_request_token != nullptr) return;
  napi_value req_obj = GetRefValue(env, req->req_obj_ref);
  EnsureReqAsyncWrap(req, req_obj);
  const char* resource_name = "QueryReqWrap";
  if (req->used_ga) {
    resource_name = "GetAddrInfoReqWrap";
  } else if (req->used_gn) {
    resource_name = "GetNameInfoReqWrap";
  } else if (!req->binding_name.empty()) {
    resource_name = req->binding_name.c_str();
  }
  req->active_request_token =
      EdgeRegisterActiveRequest(env, req_obj, resource_name, req, CancelCaresReq, GetCaresReqOwner);
}

void UntrackPendingReq(CaresReqWrap* req) {
  if (req == nullptr || req->active_request_token == nullptr) return;
  EdgeUnregisterActiveRequestToken(req->env, req->active_request_token);
  req->active_request_token = nullptr;
}

void QueueReqDestroyIfNeeded(CaresReqWrap* req) {
  if (req == nullptr || req->destroy_queued || req->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(req->env, req->async_id);
  req->destroy_queued = true;
}

void EnsureReqAsyncWrap(CaresReqWrap* req, napi_value req_obj) {
  if (req == nullptr || req->env == nullptr || req_obj == nullptr) return;
  if (req->async_id <= 0 || req->destroy_queued) {
    req->async_id = EdgeAsyncWrapNextId(req->env);
    EdgeAsyncWrapEmitInitString(
        req->env, req->async_id, req->async_wrap_name.c_str(), EdgeAsyncWrapExecutionAsyncId(req->env), req_obj);
  }
  req->destroy_queued = false;
}

napi_value GetCaresReqOwner(napi_env env, void* data) {
  auto* req = static_cast<CaresReqWrap*>(data);
  return req != nullptr ? GetRefValue(env, req->req_obj_ref) : nullptr;
}

void CancelCaresReq(void* data) {
  auto* req = static_cast<CaresReqWrap*>(data);
  if (req == nullptr) return;
  if (req->used_ga) {
    (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->ga));
    return;
  }
  if (req->used_gn) {
    (void)uv_cancel(reinterpret_cast<uv_req_t*>(&req->gn));
    return;
  }
  if (req->used_query && req->channel != nullptr && req->channel->channel != nullptr) {
    ChannelWrap* channel = req->channel;
    ares_cancel(channel->channel);
    if (channel->active_reqs.find(req) != channel->active_reqs.end()) {
      CompleteQuery(req, ARES_ECANCELLED, nullptr, 0, nullptr, false);
    }
  }
}

void PinReqObject(CaresReqWrap* req) {
  if (req == nullptr || req->env == nullptr || req->req_obj_ref == nullptr || req->pinned_ref) return;
  uint32_t ref_count = 0;
  if (napi_reference_ref(req->env, req->req_obj_ref, &ref_count) == napi_ok) {
    req->pinned_ref = true;
  }
}

void UnpinReqObject(CaresReqWrap* req) {
  if (req == nullptr || req->env == nullptr || req->req_obj_ref == nullptr || !req->pinned_ref) return;
  uint32_t ref_count = 0;
  if (napi_reference_unref(req->env, req->req_obj_ref, &ref_count) == napi_ok) {
    req->pinned_ref = false;
  }
}

void MarkReqComplete(CaresReqWrap* req) {
  if (req == nullptr) return;
  QueueReqDestroyIfNeeded(req);
  req->in_flight = false;
  req->used_ga = false;
  req->used_gn = false;
  req->used_query = false;
  req->channel = nullptr;
  req->binding_name.clear();
}

void MaybeDeleteReq(CaresReqWrap* req) {
  if (req == nullptr) return;
  if (req->orphaned || req->finalized) {
    delete req;
  }
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok) return nullptr;
  return value;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  size_t len = 0;
  if (value == nullptr) return {};
  if (napi_coerce_to_string(env, value, &value) != napi_ok ||
      napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) {
    return {};
  }
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) {
    return {};
  }
  out.resize(copied);
  return out;
}

std::string ToAsciiHostname(std::string_view input) {
  std::string out = ada::idna::to_ascii(input);
  if (!out.empty()) return out;
  return std::string(input);
}

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value MakeUint32(napi_env env, uint32_t value) {
  napi_value out = nullptr;
  napi_create_uint32(env, value, &out);
  return out;
}

napi_value MakeStringUtf8(napi_env env, const char* value) {
  napi_value out = nullptr;
  napi_create_string_utf8(env, value ? value : "", NAPI_AUTO_LENGTH, &out);
  return out;
}

napi_value MakeStringUtf8(napi_env env, const unsigned char* value) {
  return MakeStringUtf8(env, reinterpret_cast<const char*>(value));
}

napi_value MakeStringLatin1(napi_env env, const char* value, size_t len = NAPI_AUTO_LENGTH) {
  napi_value out = nullptr;
  napi_create_string_latin1(env, value ? value : "", len, &out);
  return out;
}

napi_value MakeNull(napi_env env) {
  napi_value out = nullptr;
  napi_get_null(env, &out);
  return out;
}

napi_value MakeUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

bool AppendArrayValue(napi_env env, napi_value array, napi_value value) {
  if (array == nullptr || value == nullptr) return false;
  uint32_t len = 0;
  if (napi_get_array_length(env, array, &len) != napi_ok) return false;
  return napi_set_element(env, array, len, value) == napi_ok;
}

bool SetNamedValue(napi_env env, napi_value obj, const char* name, napi_value value) {
  if (obj == nullptr || value == nullptr) return false;
  return napi_set_named_property(env, obj, name, value) == napi_ok;
}

bool SetNamedString(napi_env env, napi_value obj, const char* name, const char* value) {
  return SetNamedValue(env, obj, name, MakeStringUtf8(env, value));
}

bool SetNamedString(napi_env env, napi_value obj, const char* name, const unsigned char* value) {
  return SetNamedValue(env, obj, name, MakeStringUtf8(env, value));
}

bool SetNamedUInt(napi_env env, napi_value obj, const char* name, uint32_t value) {
  return SetNamedValue(env, obj, name, MakeUint32(env, value));
}

bool SetNamedInt(napi_env env, napi_value obj, const char* name, int32_t value) {
  return SetNamedValue(env, obj, name, MakeInt32(env, value));
}

const char* ToErrorCodeString(int status) {
  switch (status) {
    case ARES_EADDRGETNETWORKPARAMS: return "EADDRGETNETWORKPARAMS";
    case ARES_EBADFAMILY: return "EBADFAMILY";
    case ARES_EBADFLAGS: return "EBADFLAGS";
    case ARES_EBADHINTS: return "EBADHINTS";
    case ARES_EBADNAME: return "EBADNAME";
    case ARES_EBADQUERY: return "EBADQUERY";
    case ARES_EBADRESP: return "EBADRESP";
    case ARES_EBADSTR: return "EBADSTR";
    case ARES_ECANCELLED: return "ECANCELLED";
    case ARES_ECONNREFUSED: return "ECONNREFUSED";
    case ARES_EDESTRUCTION: return "EDESTRUCTION";
    case ARES_EFILE: return "EFILE";
    case ARES_EFORMERR: return "EFORMERR";
    case ARES_ELOADIPHLPAPI: return "ELOADIPHLPAPI";
    case ARES_ENODATA: return "ENODATA";
    case ARES_ENOMEM: return "ENOMEM";
    case ARES_ENONAME: return "ENONAME";
    case ARES_ENOTFOUND: return "ENOTFOUND";
    case ARES_ENOTIMP: return "ENOTIMP";
    case ARES_ENOTINITIALIZED: return "ENOTINITIALIZED";
    case ARES_EOF: return "EOF";
    case ARES_EREFUSED: return "EREFUSED";
    case ARES_ESERVFAIL: return "ESERVFAIL";
    case ARES_ETIMEOUT: return "ETIMEOUT";
    default: return "UNKNOWN_ARES_ERROR";
  }
}

uint16_t ReadUint16BE(const unsigned char* p) {
  return static_cast<uint16_t>((static_cast<uint16_t>(p[0]) << 8U) |
                               static_cast<uint16_t>(p[1]));
}

uint32_t ReadUint32BE(const unsigned char* p) {
  return (static_cast<uint32_t>(p[0]) << 24U) |
         (static_cast<uint32_t>(p[1]) << 16U) |
         (static_cast<uint32_t>(p[2]) << 8U) |
         static_cast<uint32_t>(p[3]);
}

void InvokeOnComplete(napi_env env, CaresReqWrap* req, size_t argc, napi_value* argv) {
  if (env == nullptr || req == nullptr) return;
  napi_value req_obj = GetRefValue(env, req->req_obj_ref);
  if (req_obj == nullptr) return;
  napi_value oncomplete = nullptr;
  if (napi_get_named_property(env, req_obj, "oncomplete", &oncomplete) != napi_ok || oncomplete == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, oncomplete, &type) != napi_ok || type != napi_function) return;
  napi_value ignored = nullptr;
  (void)EdgeAsyncWrapMakeCallback(
      env, req->async_id, req_obj, req_obj, oncomplete, argc, argv, &ignored, kEdgeMakeCallbackNone);
}

void CallOnCompleteError(CaresReqWrap* req, int status) {
  if (req == nullptr || req->env == nullptr) return;
  napi_value arg = MakeStringUtf8(req->env, ToErrorCodeString(status));
  InvokeOnComplete(req->env, req, 1, &arg);
}

void CallOnCompleteSuccess(CaresReqWrap* req, napi_value result, napi_value extra = nullptr) {
  if (req == nullptr || req->env == nullptr) return;
  napi_value argv[3] = {
      MakeInt32(req->env, 0),
      result ? result : MakeNull(req->env),
      extra,
  };
  const size_t argc = (extra == nullptr ? 2 : 3);
  InvokeOnComplete(req->env, req, argc, argv);
}

void CloseChannelTimer(ChannelWrap* channel) {
  if (channel == nullptr || channel->timer_handle == nullptr) return;
  uv_timer_t* timer = channel->timer_handle;
  channel->timer_handle = nullptr;
  uv_timer_stop(timer);
  timer->data = nullptr;
  uv_close(reinterpret_cast<uv_handle_t*>(timer), [](uv_handle_t* handle) {
    delete reinterpret_cast<uv_timer_t*>(handle);
  });
}

void AresTimeout(uv_timer_t* handle) {
  auto* channel = static_cast<ChannelWrap*>(handle->data);
  if (channel == nullptr || channel->channel == nullptr) return;
  ares_process_fd(channel->channel, ARES_SOCKET_BAD, ARES_SOCKET_BAD);
  if (channel->timer_handle == handle) {
    StartChannelTimer(channel);
  }
}

void StartChannelTimer(ChannelWrap* channel) {
  if (channel == nullptr || channel->env == nullptr || channel->channel == nullptr) return;
  if (channel->active_query_count <= 0) {
    if (channel->timer_handle != nullptr) {
      uv_timer_stop(channel->timer_handle);
    }
    return;
  }

  if (channel->timer_handle == nullptr) {
    auto* timer = new uv_timer_t();
    timer->data = channel;
    uv_loop_t* loop = EdgeGetEnvLoop(channel->env);
    if (loop == nullptr || uv_timer_init(loop, timer) != 0) {
      delete timer;
      return;
    }
    channel->timer_handle = timer;
  }

  struct timeval tv {};
  struct timeval* tvp = ares_timeout(channel->channel, nullptr, &tv);
  if (tvp == nullptr) {
    uv_timer_stop(channel->timer_handle);
    return;
  }

  uint64_t timeout = 0;
  if (tvp->tv_sec > 0) {
    timeout += static_cast<uint64_t>(tvp->tv_sec) * 1000;
  }
  if (tvp->tv_usec > 0) {
    timeout += static_cast<uint64_t>((tvp->tv_usec + 999) / 1000);
  }
  if (timeout == 0) timeout = 1;

  uv_timer_start(channel->timer_handle, AresTimeout, timeout, 0);
}

void ares_poll_close_cb(uv_handle_t* handle) {
  auto* task = static_cast<NodeAresTask*>(handle->data);
  delete task;
}

void ares_poll_cb(uv_poll_t* watcher, int status, int events) {
  auto* task = static_cast<NodeAresTask*>(watcher->data);
  if (task == nullptr || task->channel == nullptr) return;
  ChannelWrap* channel = task->channel;
  if (channel->channel == nullptr) return;

  if (status < 0) {
    ares_process_fd(channel->channel, task->sock, task->sock);
    StartChannelTimer(channel);
    return;
  }

  ares_process_fd(channel->channel,
                  (events & UV_READABLE) ? task->sock : ARES_SOCKET_BAD,
                  (events & UV_WRITABLE) ? task->sock : ARES_SOCKET_BAD);
  StartChannelTimer(channel);
}

void ares_sockstate_cb(void* data, ares_socket_t sock, int read, int write) {
  auto* channel = static_cast<ChannelWrap*>(data);
  if (channel == nullptr) return;

  auto it = channel->tasks.find(sock);
  NodeAresTask* task = (it == channel->tasks.end()) ? nullptr : it->second;

  if (read || write) {
    if (task == nullptr) {
      task = new NodeAresTask();
      task->channel = channel;
      task->sock = sock;
      uv_loop_t* loop = EdgeGetEnvLoop(channel->env);
      if (loop == nullptr || uv_poll_init_socket(loop, &task->poll_watcher, sock) != 0) {
        delete task;
        return;
      }
      task->poll_watcher.data = task;
      channel->tasks.emplace(sock, task);
    }

    uv_poll_start(&task->poll_watcher,
                  (read ? UV_READABLE : 0) | (write ? UV_WRITABLE : 0),
                  ares_poll_cb);
    StartChannelTimer(channel);
    return;
  }

  if (task != nullptr) {
    channel->tasks.erase(it);
    uv_poll_stop(&task->poll_watcher);
    uv_close(reinterpret_cast<uv_handle_t*>(&task->poll_watcher), ares_poll_close_cb);
  }

  if (channel->tasks.empty() && channel->active_query_count <= 0) {
    CloseChannelTimer(channel);
  } else {
    StartChannelTimer(channel);
  }
}

void DestroyChannelCares(ChannelWrap* channel) {
  if (channel == nullptr) return;

  DeleteChannelActiveRef(channel);

  for (auto& [_, task] : channel->tasks) {
    if (task != nullptr) {
      task->channel = nullptr;
      uv_poll_stop(&task->poll_watcher);
      uv_close(reinterpret_cast<uv_handle_t*>(&task->poll_watcher), ares_poll_close_cb);
    }
  }
  channel->tasks.clear();

  CloseChannelTimer(channel);

  if (channel->channel != nullptr) {
    ares_destroy(channel->channel);
    channel->channel = nullptr;
  }

  if (channel->library_inited) {
    if (g_cares_library_refcount > 0) {
      g_cares_library_refcount--;
      if (g_cares_library_refcount == 0) {
        ares_library_cleanup();
      }
    }
    channel->library_inited = false;
  }
}

int SetupChannelCares(ChannelWrap* channel) {
  if (channel == nullptr) return ARES_EBADQUERY;

  if (g_cares_library_refcount == 0) {
    int init_rc = ares_library_init(ARES_LIB_INIT_ALL);
    if (init_rc != ARES_SUCCESS) return init_rc;
  }
  g_cares_library_refcount++;
  channel->library_inited = true;

  ares_options options{};
  options.flags = ARES_FLAG_NOCHECKRESP;
  options.sock_state_cb = ares_sockstate_cb;
  options.sock_state_cb_data = channel;
  options.timeout = channel->timeout;
  options.tries = channel->tries;
  options.qcache_max_ttl = 0;

  int optmask = ARES_OPT_FLAGS |
                ARES_OPT_TIMEOUTMS |
                ARES_OPT_SOCK_STATE_CB |
                ARES_OPT_TRIES |
                ARES_OPT_QUERY_CACHE;

  if (channel->max_timeout > 0) {
    options.maxtimeout = channel->max_timeout;
    optmask |= ARES_OPT_MAXTIMEOUTMS;
  }

  int rc = ares_init_options(&channel->channel, &options, optmask);
  if (rc != ARES_SUCCESS) {
    if (channel->library_inited && g_cares_library_refcount > 0) {
      g_cares_library_refcount--;
      if (g_cares_library_refcount == 0) {
        ares_library_cleanup();
      }
    }
    channel->library_inited = false;
  }
  return rc;
}

void EnsureServers(ChannelWrap* channel) {
  if (channel == nullptr || channel->channel == nullptr) return;
  if (channel->query_last_ok || !channel->is_servers_default) return;

  ares_addr_port_node* servers = nullptr;
  int rc = ares_get_servers_ports(channel->channel, &servers);
  if (rc != ARES_SUCCESS || servers == nullptr) {
    if (servers != nullptr) ares_free_data(servers);
    return;
  }

  bool should_reset = false;
  if (servers->next == nullptr &&
      servers->family == AF_INET &&
      servers->addr.addr4.s_addr == htonl(INADDR_LOOPBACK) &&
      servers->tcp_port == 0 &&
      servers->udp_port == 0) {
    should_reset = true;
  } else {
    channel->is_servers_default = false;
  }

  ares_free_data(servers);

  if (!should_reset) return;

  DestroyChannelCares(channel);
  int reset_rc = SetupChannelCares(channel);
  if (reset_rc != ARES_SUCCESS && channel->env != nullptr) {
    napi_throw_error(channel->env, ToErrorCodeString(reset_rc), ares_strerror(reset_rc));
  }
}

void ScheduleFinalizedChannelDestroy(ChannelWrap* channel) {
  if (channel == nullptr || !channel->finalized || channel->active_query_count > 0 ||
      channel->destroy_scheduled) {
    return;
  }

  uv_loop_t* loop = EdgeGetEnvLoop(channel->env);
  auto* timer = new uv_timer_t();
  if (loop == nullptr || uv_timer_init(loop, timer) != 0) {
    delete timer;
    DestroyChannelCares(channel);
    delete channel;
    return;
  }

  channel->destroy_scheduled = true;
  timer->data = channel;
  uv_timer_start(timer, [](uv_timer_t* handle) {
    auto* channel = static_cast<ChannelWrap*>(handle != nullptr ? handle->data : nullptr);
    if (handle != nullptr) {
      handle->data = nullptr;
      uv_timer_stop(handle);
      uv_close(reinterpret_cast<uv_handle_t*>(handle), [](uv_handle_t* close_handle) {
        delete reinterpret_cast<uv_timer_t*>(close_handle);
      });
    }
    if (channel == nullptr) return;
    DestroyChannelCares(channel);
    delete channel;
  }, 0, 0);
}

void ModifyActivityQueryCount(ChannelWrap* channel, int delta) {
  if (channel == nullptr) return;
  const int prev = channel->active_query_count;
  channel->active_query_count += delta;
  if (channel->active_query_count < 0) channel->active_query_count = 0;

  if (channel->env != nullptr && channel->wrapper_ref != nullptr) {
    if (prev == 0 && channel->active_query_count > 0) {
      uint32_t ref_count = 0;
      (void)napi_reference_ref(channel->env, channel->wrapper_ref, &ref_count);
    } else if (prev > 0 && channel->active_query_count == 0) {
      DeleteChannelActiveRef(channel);
      // During env cleanup, avoid dropping the last ref from inside c-ares
      // callbacks; ChannelFinalize will run as part of env teardown.
      if (!EnvCleanupInProgress(channel->env)) {
        uint32_t ref_count = 0;
        (void)napi_reference_unref(channel->env, channel->wrapper_ref, &ref_count);
      }
    }
  }

  ScheduleFinalizedChannelDestroy(channel);
}

void CleanupReqAfterAsync(CaresReqWrap* req) {
  if (req == nullptr) return;
  UnpinReqObject(req);
  MaybeDeleteReq(req);
}

int ParseGeneralReply(napi_env env,
                      const unsigned char* buf,
                      int len,
                      int* type,
                      napi_value ret,
                      void* addrttls = nullptr,
                      int* naddrttls = nullptr) {
  hostent* host = nullptr;
  int status = ARES_EBADRESP;

  switch (*type) {
    case ns_t_a:
    case ns_t_cname:
    case ns_t_cname_or_a:
      status = ares_parse_a_reply(buf,
                                  len,
                                  &host,
                                  static_cast<ares_addrttl*>(addrttls),
                                  naddrttls);
      break;
    case ns_t_aaaa:
      status = ares_parse_aaaa_reply(buf,
                                     len,
                                     &host,
                                     static_cast<ares_addr6ttl*>(addrttls),
                                     naddrttls);
      break;
    case ns_t_ns:
      status = ares_parse_ns_reply(buf, len, &host);
      break;
    case ns_t_ptr:
      status = ares_parse_ptr_reply(buf, len, nullptr, 0, AF_INET, &host);
      break;
    default:
      return ARES_EBADQUERY;
  }

  if (status != ARES_SUCCESS) return status;
  HostentPtr host_ptr(host);
  if (host_ptr == nullptr) return ARES_EBADRESP;

  if (((*type == ns_t_cname_or_a) && host_ptr->h_name != nullptr &&
       host_ptr->h_aliases != nullptr && host_ptr->h_aliases[0] != nullptr) ||
      *type == ns_t_cname) {
    *type = ns_t_cname;
    return AppendArrayValue(env, ret, MakeStringUtf8(env, host_ptr->h_name)) ? ARES_SUCCESS : ARES_ENOMEM;
  }

  if (*type == ns_t_cname_or_a) *type = ns_t_a;

  if (*type == ns_t_ns || *type == ns_t_ptr) {
    if (host_ptr->h_aliases == nullptr) return ARES_SUCCESS;
    for (uint32_t i = 0; host_ptr->h_aliases[i] != nullptr; ++i) {
      if (!AppendArrayValue(env, ret, MakeStringUtf8(env, host_ptr->h_aliases[i]))) {
        return ARES_ENOMEM;
      }
    }
    return ARES_SUCCESS;
  }

  if (host_ptr->h_addr_list == nullptr) return ARES_SUCCESS;
  for (uint32_t i = 0; host_ptr->h_addr_list[i] != nullptr; ++i) {
    char ip[INET6_ADDRSTRLEN] = {0};
    if (uv_inet_ntop(host_ptr->h_addrtype, host_ptr->h_addr_list[i], ip, sizeof(ip)) != 0) {
      continue;
    }
    if (!AppendArrayValue(env, ret, MakeStringUtf8(env, ip))) return ARES_ENOMEM;
  }

  return ARES_SUCCESS;
}

template <typename T>
napi_value AddrTTLToArray(napi_env env, const T* addrttls, size_t count) {
  napi_value arr = nullptr;
  napi_create_array_with_length(env, count, &arr);
  for (uint32_t i = 0; i < count; ++i) {
    napi_set_element(env, arr, i, MakeUint32(env, addrttls[i].ttl));
  }
  return arr;
}

int ParseMxReply(napi_env env,
                 const unsigned char* buf,
                 int len,
                 napi_value ret,
                 bool need_type = false) {
  ares_mx_reply* mx_start = nullptr;
  int status = ares_parse_mx_reply(buf, len, &mx_start);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(mx_start);

  for (ares_mx_reply* cur = mx_start; cur != nullptr; cur = cur->next) {
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedString(env, obj, "exchange", cur->host) ||
        !SetNamedInt(env, obj, "priority", cur->priority)) {
      return ARES_ENOMEM;
    }
    if (need_type && !SetNamedString(env, obj, "type", "MX")) return ARES_ENOMEM;
    if (!AppendArrayValue(env, ret, obj)) return ARES_ENOMEM;
  }

  return ARES_SUCCESS;
}

int ParseCaaReply(napi_env env,
                  const unsigned char* buf,
                  int len,
                  napi_value ret,
                  bool need_type = false) {
  ares_caa_reply* caa_start = nullptr;
  int status = ares_parse_caa_reply(buf, len, &caa_start);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(caa_start);

  for (ares_caa_reply* cur = caa_start; cur != nullptr; cur = cur->next) {
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedInt(env, obj, "critical", cur->critical)) return ARES_ENOMEM;
    if (need_type && !SetNamedString(env, obj, "type", "CAA")) return ARES_ENOMEM;

    napi_value prop = MakeStringUtf8(env, cur->value);
    if (prop == nullptr ||
        napi_set_named_property(env,
                                obj,
                                reinterpret_cast<const char*>(cur->property),
                                prop) != napi_ok) {
      return ARES_ENOMEM;
    }

    if (!AppendArrayValue(env, ret, obj)) return ARES_ENOMEM;
  }

  return ARES_SUCCESS;
}

int ParseTlsaReply(napi_env env,
                   const unsigned char* buf,
                   int len,
                   napi_value ret) {
  ares_dns_record_t* dnsrec = nullptr;
  int status = ares_dns_parse(buf, len, 0, &dnsrec);
  if (status != ARES_SUCCESS) {
    if (dnsrec != nullptr) ares_dns_record_destroy(dnsrec);
    return status;
  }
  std::unique_ptr<ares_dns_record_t, decltype(&ares_dns_record_destroy)> free_me(dnsrec, ares_dns_record_destroy);

  const size_t rr_count = ares_dns_record_rr_cnt(dnsrec, ARES_SECTION_ANSWER);
  for (size_t i = 0; i < rr_count; ++i) {
    const ares_dns_rr_t* rr = ares_dns_record_rr_get(dnsrec, ARES_SECTION_ANSWER, i);
    if (rr == nullptr || ares_dns_rr_get_type(rr) != ARES_REC_TYPE_TLSA) continue;

    const unsigned char cert_usage = ares_dns_rr_get_u8(rr, ARES_RR_TLSA_CERT_USAGE);
    const unsigned char selector = ares_dns_rr_get_u8(rr, ARES_RR_TLSA_SELECTOR);
    const unsigned char match = ares_dns_rr_get_u8(rr, ARES_RR_TLSA_MATCH);

    size_t data_len = 0;
    const unsigned char* data = ares_dns_rr_get_bin(rr, ARES_RR_TLSA_DATA, &data_len);
    if (data == nullptr || data_len == 0) continue;

    void* ab_data = nullptr;
    napi_value ab = nullptr;
    if (napi_create_arraybuffer(env, data_len, &ab_data, &ab) != napi_ok || ab_data == nullptr) {
      return ARES_ENOMEM;
    }
    memcpy(ab_data, data, data_len);

    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedUInt(env, obj, "certUsage", cert_usage) ||
        !SetNamedUInt(env, obj, "selector", selector) ||
        !SetNamedUInt(env, obj, "match", match) ||
        !SetNamedValue(env, obj, "data", ab) ||
        !AppendArrayValue(env, ret, obj)) {
      return ARES_ENOMEM;
    }
  }

  return ARES_SUCCESS;
}

int ParseTxtReply(napi_env env,
                  const unsigned char* buf,
                  int len,
                  napi_value ret,
                  bool need_type = false) {
  ares_txt_ext* txt_out = nullptr;
  int status = ares_parse_txt_reply_ext(buf, len, &txt_out);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(txt_out);

  std::vector<napi_value> chunks;
  auto flush_chunk = [&]() -> int {
    if (chunks.empty()) return ARES_SUCCESS;

    napi_value chunk_arr = nullptr;
    napi_create_array_with_length(env, chunks.size(), &chunk_arr);
    for (uint32_t i = 0; i < chunks.size(); ++i) {
      if (napi_set_element(env, chunk_arr, i, chunks[i]) != napi_ok) return ARES_ENOMEM;
    }

    if (need_type) {
      napi_value obj = nullptr;
      napi_create_object(env, &obj);
      if (!SetNamedValue(env, obj, "entries", chunk_arr) ||
          !SetNamedString(env, obj, "type", "TXT") ||
          !AppendArrayValue(env, ret, obj)) {
        return ARES_ENOMEM;
      }
    } else if (!AppendArrayValue(env, ret, chunk_arr)) {
      return ARES_ENOMEM;
    }

    chunks.clear();
    return ARES_SUCCESS;
  };

  for (ares_txt_ext* cur = txt_out; cur != nullptr; cur = cur->next) {
    if (cur->record_start) {
      const int flush_status = flush_chunk();
      if (flush_status != ARES_SUCCESS) return flush_status;
    }
    chunks.push_back(MakeStringLatin1(env,
                                      reinterpret_cast<const char*>(cur->txt),
                                      cur->length));
  }

  return flush_chunk();
}

int ParseSrvReply(napi_env env,
                  const unsigned char* buf,
                  int len,
                  napi_value ret,
                  bool need_type = false) {
  ares_srv_reply* srv_start = nullptr;
  int status = ares_parse_srv_reply(buf, len, &srv_start);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(srv_start);

  for (ares_srv_reply* cur = srv_start; cur != nullptr; cur = cur->next) {
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedString(env, obj, "name", cur->host) ||
        !SetNamedInt(env, obj, "port", cur->port) ||
        !SetNamedInt(env, obj, "priority", cur->priority) ||
        !SetNamedInt(env, obj, "weight", cur->weight)) {
      return ARES_ENOMEM;
    }
    if (need_type && !SetNamedString(env, obj, "type", "SRV")) return ARES_ENOMEM;
    if (!AppendArrayValue(env, ret, obj)) return ARES_ENOMEM;
  }

  return ARES_SUCCESS;
}

int ParseNaptrReply(napi_env env,
                    const unsigned char* buf,
                    int len,
                    napi_value ret,
                    bool need_type = false) {
  ares_naptr_reply* naptr_start = nullptr;
  int status = ares_parse_naptr_reply(buf, len, &naptr_start);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(naptr_start);

  for (ares_naptr_reply* cur = naptr_start; cur != nullptr; cur = cur->next) {
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedString(env, obj, "flags", cur->flags) ||
        !SetNamedString(env, obj, "service", cur->service) ||
        !SetNamedString(env, obj, "regexp", cur->regexp) ||
        !SetNamedString(env, obj, "replacement", cur->replacement) ||
        !SetNamedInt(env, obj, "order", cur->order) ||
        !SetNamedInt(env, obj, "preference", cur->preference)) {
      return ARES_ENOMEM;
    }
    if (need_type && !SetNamedString(env, obj, "type", "NAPTR")) return ARES_ENOMEM;
    if (!AppendArrayValue(env, ret, obj)) return ARES_ENOMEM;
  }

  return ARES_SUCCESS;
}

int ParseSoaReplyAny(napi_env env,
                     const unsigned char* buf,
                     int len,
                     napi_value* out) {
  *out = nullptr;
  if (len < NS_HFIXEDSZ) return ARES_EBADRESP;

  const unsigned int ancount = ReadUint16BE(buf + 6);
  const unsigned char* ptr = buf + NS_HFIXEDSZ;

  char* name_temp = nullptr;
  long temp_len = 0;
  int status = ares_expand_name(ptr, buf, len, &name_temp, &temp_len);
  if (status != ARES_SUCCESS) {
    return status == ARES_EBADNAME ? ARES_EBADRESP : status;
  }
  AresStringPtr name(name_temp);

  if (ptr + temp_len + NS_QFIXEDSZ > buf + len) {
    return ARES_EBADRESP;
  }
  ptr += temp_len + NS_QFIXEDSZ;

  for (unsigned int i = 0; i < ancount; ++i) {
    char* rr_name_temp = nullptr;
    long rr_temp_len = 0;
    int status2 = ares_expand_name(ptr, buf, len, &rr_name_temp, &rr_temp_len);
    if (status2 != ARES_SUCCESS) {
      return status2 == ARES_EBADNAME ? ARES_EBADRESP : status2;
    }
    AresStringPtr rr_name(rr_name_temp);

    ptr += rr_temp_len;
    if (ptr + NS_RRFIXEDSZ > buf + len) {
      return ARES_EBADRESP;
    }

    const int rr_type = ReadUint16BE(ptr);
    const int rr_len = ReadUint16BE(ptr + 8);
    ptr += NS_RRFIXEDSZ;

    if (ptr + rr_len > buf + len) {
      return ARES_EBADRESP;
    }

    if (rr_type == ns_t_soa) {
      char* nsname_temp = nullptr;
      long nsname_temp_len = 0;
      int status3 = ares_expand_name(ptr, buf, len, &nsname_temp, &nsname_temp_len);
      if (status3 != ARES_SUCCESS) {
        return status3 == ARES_EBADNAME ? ARES_EBADRESP : status3;
      }
      AresStringPtr nsname(nsname_temp);
      ptr += nsname_temp_len;

      char* hostmaster_temp = nullptr;
      long hostmaster_temp_len = 0;
      int status4 = ares_expand_name(ptr, buf, len, &hostmaster_temp, &hostmaster_temp_len);
      if (status4 != ARES_SUCCESS) {
        return status4 == ARES_EBADNAME ? ARES_EBADRESP : status4;
      }
      AresStringPtr hostmaster(hostmaster_temp);
      ptr += hostmaster_temp_len;

      if (ptr + 5 * 4 > buf + len) {
        return ARES_EBADRESP;
      }

      const uint32_t serial = ReadUint32BE(ptr + 0 * 4);
      const uint32_t refresh = ReadUint32BE(ptr + 1 * 4);
      const uint32_t retry = ReadUint32BE(ptr + 2 * 4);
      const uint32_t expire = ReadUint32BE(ptr + 3 * 4);
      const uint32_t minttl = ReadUint32BE(ptr + 4 * 4);

      napi_value obj = nullptr;
      napi_create_object(env, &obj);
      if (!SetNamedString(env, obj, "nsname", nsname.get()) ||
          !SetNamedString(env, obj, "hostmaster", hostmaster.get()) ||
          !SetNamedUInt(env, obj, "serial", serial) ||
          !SetNamedInt(env, obj, "refresh", static_cast<int32_t>(refresh)) ||
          !SetNamedInt(env, obj, "retry", static_cast<int32_t>(retry)) ||
          !SetNamedInt(env, obj, "expire", static_cast<int32_t>(expire)) ||
          !SetNamedUInt(env, obj, "minttl", minttl) ||
          !SetNamedString(env, obj, "type", "SOA")) {
        return ARES_ENOMEM;
      }

      *out = obj;
      break;
    }

    ptr += rr_len;
  }

  return ARES_SUCCESS;
}

int ParseAnyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);

  int type = ns_t_cname_or_a;
  ares_addrttl addrttls[256];
  int naddrttls = static_cast<int>(sizeof(addrttls) / sizeof(addrttls[0]));

  int status = ParseGeneralReply(env, buf, len, &type, ret, addrttls, &naddrttls);
  const uint32_t a_count = [&]() {
    uint32_t len_val = 0;
    napi_get_array_length(env, ret, &len_val);
    return len_val;
  }();

  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  if (type == ns_t_a) {
    for (uint32_t i = 0; i < a_count; ++i) {
      napi_value address = nullptr;
      if (napi_get_element(env, ret, i, &address) != napi_ok) return ARES_ENOMEM;
      napi_value obj = nullptr;
      napi_create_object(env, &obj);
      if (!SetNamedValue(env, obj, "address", address) ||
          !SetNamedUInt(env, obj, "ttl", addrttls[i].ttl) ||
          !SetNamedString(env, obj, "type", "A") ||
          napi_set_element(env, ret, i, obj) != napi_ok) {
        return ARES_ENOMEM;
      }
    }
  } else {
    for (uint32_t i = 0; i < a_count; ++i) {
      napi_value value = nullptr;
      if (napi_get_element(env, ret, i, &value) != napi_ok) return ARES_ENOMEM;
      napi_value obj = nullptr;
      napi_create_object(env, &obj);
      if (!SetNamedValue(env, obj, "value", value) ||
          !SetNamedString(env, obj, "type", "CNAME") ||
          napi_set_element(env, ret, i, obj) != napi_ok) {
        return ARES_ENOMEM;
      }
    }
  }

  ares_addr6ttl addr6ttls[256];
  int naddr6ttls = static_cast<int>(sizeof(addr6ttls) / sizeof(addr6ttls[0]));
  type = ns_t_aaaa;
  status = ParseGeneralReply(env, buf, len, &type, ret, addr6ttls, &naddr6ttls);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  uint32_t total_after_aaaa = 0;
  napi_get_array_length(env, ret, &total_after_aaaa);
  for (uint32_t i = a_count; i < total_after_aaaa; ++i) {
    napi_value address = nullptr;
    if (napi_get_element(env, ret, i, &address) != napi_ok) return ARES_ENOMEM;
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedValue(env, obj, "address", address) ||
        !SetNamedUInt(env, obj, "ttl", addr6ttls[i - a_count].ttl) ||
        !SetNamedString(env, obj, "type", "AAAA") ||
        napi_set_element(env, ret, i, obj) != napi_ok) {
      return ARES_ENOMEM;
    }
  }

  status = ParseMxReply(env, buf, len, ret, true);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  uint32_t old_count = 0;
  napi_get_array_length(env, ret, &old_count);
  type = ns_t_ns;
  status = ParseGeneralReply(env, buf, len, &type, ret);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  uint32_t ns_count = 0;
  napi_get_array_length(env, ret, &ns_count);
  for (uint32_t i = old_count; i < ns_count; ++i) {
    napi_value value = nullptr;
    if (napi_get_element(env, ret, i, &value) != napi_ok) return ARES_ENOMEM;
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedValue(env, obj, "value", value) ||
        !SetNamedString(env, obj, "type", "NS") ||
        napi_set_element(env, ret, i, obj) != napi_ok) {
      return ARES_ENOMEM;
    }
  }

  status = ParseTxtReply(env, buf, len, ret, true);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  status = ParseSrvReply(env, buf, len, ret, true);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  napi_get_array_length(env, ret, &old_count);
  type = ns_t_ptr;
  status = ParseGeneralReply(env, buf, len, &type, ret);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  uint32_t ptr_count = 0;
  napi_get_array_length(env, ret, &ptr_count);
  for (uint32_t i = old_count; i < ptr_count; ++i) {
    napi_value value = nullptr;
    if (napi_get_element(env, ret, i, &value) != napi_ok) return ARES_ENOMEM;
    napi_value obj = nullptr;
    napi_create_object(env, &obj);
    if (!SetNamedValue(env, obj, "value", value) ||
        !SetNamedString(env, obj, "type", "PTR") ||
        napi_set_element(env, ret, i, obj) != napi_ok) {
      return ARES_ENOMEM;
    }
  }

  status = ParseNaptrReply(env, buf, len, ret, true);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  napi_value soa_obj = nullptr;
  status = ParseSoaReplyAny(env, buf, len, &soa_obj);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;
  if (soa_obj != nullptr && !AppendArrayValue(env, ret, soa_obj)) return ARES_ENOMEM;

  status = ParseTlsaReply(env, buf, len, ret);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  status = ParseCaaReply(env, buf, len, ret, true);
  if (status != ARES_SUCCESS && status != ARES_ENODATA) return status;

  *out = ret;
  return ARES_SUCCESS;
}

int ParseAReply(napi_env env,
                const unsigned char* buf,
                int len,
                napi_value* out,
                napi_value* ttls_out) {
  ares_addrttl addrttls[256];
  int naddrttls = static_cast<int>(sizeof(addrttls) / sizeof(addrttls[0]));
  int type = ns_t_a;

  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseGeneralReply(env, buf, len, &type, ret, addrttls, &naddrttls);
  if (status != ARES_SUCCESS) return status;

  *out = ret;
  *ttls_out = AddrTTLToArray(env, addrttls, static_cast<size_t>(naddrttls));
  return ARES_SUCCESS;
}

int ParseAaaaReply(napi_env env,
                   const unsigned char* buf,
                   int len,
                   napi_value* out,
                   napi_value* ttls_out) {
  ares_addr6ttl addrttls[256];
  int naddrttls = static_cast<int>(sizeof(addrttls) / sizeof(addrttls[0]));
  int type = ns_t_aaaa;

  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseGeneralReply(env, buf, len, &type, ret, addrttls, &naddrttls);
  if (status != ARES_SUCCESS) return status;

  *out = ret;
  *ttls_out = AddrTTLToArray(env, addrttls, static_cast<size_t>(naddrttls));
  return ARES_SUCCESS;
}

int ParseCaaOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseCaaReply(env, buf, len, ret, false);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseCnameReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  int type = ns_t_cname;
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseGeneralReply(env, buf, len, &type, ret);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseMxOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseMxReply(env, buf, len, ret, false);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseNsReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  int type = ns_t_ns;
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseGeneralReply(env, buf, len, &type, ret);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseTlsaOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseTlsaReply(env, buf, len, ret);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseTxtOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseTxtReply(env, buf, len, ret, false);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseSrvOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseSrvReply(env, buf, len, ret, false);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParsePtrReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  int type = ns_t_ptr;
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseGeneralReply(env, buf, len, &type, ret);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseNaptrOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  int status = ParseNaptrReply(env, buf, len, ret, false);
  if (status != ARES_SUCCESS) return status;
  *out = ret;
  return ARES_SUCCESS;
}

int ParseSoaOnlyReply(napi_env env, const unsigned char* buf, int len, napi_value* out) {
  ares_soa_reply* soa_out = nullptr;
  int status = ares_parse_soa_reply(buf, len, &soa_out);
  if (status != ARES_SUCCESS) return status;
  AresDataPtr free_me(soa_out);

  napi_value obj = nullptr;
  napi_create_object(env, &obj);
  if (!SetNamedString(env, obj, "nsname", soa_out->nsname) ||
      !SetNamedString(env, obj, "hostmaster", soa_out->hostmaster) ||
      !SetNamedUInt(env, obj, "serial", soa_out->serial) ||
      !SetNamedInt(env, obj, "refresh", soa_out->refresh) ||
      !SetNamedInt(env, obj, "retry", soa_out->retry) ||
      !SetNamedInt(env, obj, "expire", soa_out->expire) ||
      !SetNamedUInt(env, obj, "minttl", soa_out->minttl)) {
    return ARES_ENOMEM;
  }

  *out = obj;
  return ARES_SUCCESS;
}

int ParseReverseHost(napi_env env, hostent* host, napi_value* out) {
  napi_value ret = nullptr;
  napi_create_array(env, &ret);
  std::unordered_set<std::string> seen;
  if (host != nullptr && host->h_name != nullptr && host->h_name[0] != '\0') {
    if (!AppendArrayValue(env, ret, MakeStringUtf8(env, host->h_name))) {
      return ARES_ENOMEM;
    }
    seen.insert(host->h_name);
  }
  if (host != nullptr && host->h_aliases != nullptr) {
    for (uint32_t i = 0; host->h_aliases[i] != nullptr; ++i) {
      if (host->h_aliases[i] == nullptr || host->h_aliases[i][0] == '\0') continue;
      if (seen.find(host->h_aliases[i]) != seen.end()) continue;
      if (!AppendArrayValue(env, ret, MakeStringUtf8(env, host->h_aliases[i]))) {
        return ARES_ENOMEM;
      }
      seen.insert(host->h_aliases[i]);
    }
  }
  *out = ret;
  return ARES_SUCCESS;
}

void CompleteQuery(CaresReqWrap* req,
                   int status,
                   const unsigned char* buf,
                   int len,
                   hostent* host,
                   bool from_host) {
  if (req == nullptr) return;

  ChannelWrap* channel = req->channel;
  if (channel != nullptr) {
    channel->query_last_ok = (status != ARES_ECONNREFUSED);
    channel->active_reqs.erase(req);
  }

  napi_env env = req->env;
  const bool env_cleaning = (env != nullptr && EnvCleanupInProgress(env));
  UntrackPendingReq(req);

  if (env == nullptr) {
    MarkReqComplete(req);
    MaybeDeleteReq(req);
    if (channel != nullptr) {
      ModifyActivityQueryCount(channel, -1);
    }
    return;
  }

  edge::HandleScope scope(env);
  if (!scope.is_open()) {
    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
    if (channel != nullptr) {
      ModifyActivityQueryCount(channel, -1);
    }
    return;
  }

  if (status != ARES_SUCCESS) {
    // Do not invoke JS callbacks once env teardown has started.
    if (!env_cleaning) {
      CallOnCompleteError(req, status);
    }
    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
  } else {
    napi_value result = nullptr;
    napi_value extra = nullptr;
    int parse_status = ARES_SUCCESS;

    switch (req->query_kind) {
      case QueryKind::kAny:
        parse_status = ParseAnyReply(env, buf, len, &result);
        break;
      case QueryKind::kA:
        parse_status = ParseAReply(env, buf, len, &result, &extra);
        break;
      case QueryKind::kAaaa:
        parse_status = ParseAaaaReply(env, buf, len, &result, &extra);
        break;
      case QueryKind::kCaa:
        parse_status = ParseCaaOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kCname:
        parse_status = ParseCnameReply(env, buf, len, &result);
        break;
      case QueryKind::kMx:
        parse_status = ParseMxOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kNs:
        parse_status = ParseNsReply(env, buf, len, &result);
        break;
      case QueryKind::kTlsa:
        parse_status = ParseTlsaOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kTxt:
        parse_status = ParseTxtOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kSrv:
        parse_status = ParseSrvOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kPtr:
        parse_status = ParsePtrReply(env, buf, len, &result);
        break;
      case QueryKind::kNaptr:
        parse_status = ParseNaptrOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kSoa:
        parse_status = ParseSoaOnlyReply(env, buf, len, &result);
        break;
      case QueryKind::kReverse:
        parse_status = from_host ? ParseReverseHost(env, host, &result) : ARES_EBADRESP;
        break;
    }

    if (!env_cleaning) {
      if (parse_status != ARES_SUCCESS) {
        CallOnCompleteError(req, parse_status);
      } else {
        CallOnCompleteSuccess(req, result, extra);
      }
    }

    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
  }

  if (channel != nullptr) {
    ModifyActivityQueryCount(channel, -1);
  }
}

void OnDnsQueryComplete(void* arg,
                        ares_status_t status,
                        size_t /*timeouts*/,
                        const ares_dns_record_t* dnsrec) {
  auto* req = static_cast<CaresReqWrap*>(arg);
  if (req == nullptr) return;

  unsigned char* wire = nullptr;
  size_t wire_len = 0;
  int parse_status = status;

  if (status == ARES_SUCCESS) {
    parse_status = ares_dns_write(dnsrec, &wire, &wire_len);
  }

  CompleteQuery(req,
                parse_status,
                wire,
                static_cast<int>(wire_len),
                nullptr,
                false);

  if (wire != nullptr) {
    ares_free_string(reinterpret_cast<char*>(wire));
  }
}

void OnReverseQueryComplete(void* arg,
                            int status,
                            int /*timeouts*/,
                            hostent* host) {
  auto* req = static_cast<CaresReqWrap*>(arg);
  if (req == nullptr) return;
  CompleteQuery(req, status, nullptr, 0, host, true);
}

int DispatchQuery(ChannelWrap* channel, CaresReqWrap* req, const QueryMethodData* method) {
  if (channel == nullptr || channel->channel == nullptr || req == nullptr || method == nullptr) {
    return UV_EINVAL;
  }

  EnsureServers(channel);

  req->used_query = true;
  req->channel = channel;
  req->query_kind = method->kind;
  req->binding_name = method->binding_name;
  req->hostname = ToAsciiHostname(req->hostname);

  ModifyActivityQueryCount(channel, 1);

  if (method->kind == QueryKind::kReverse) {
    char address_buffer[sizeof(in6_addr)] = {0};
    int length = 0;
    int family = 0;

    if (uv_inet_pton(AF_INET, req->hostname.c_str(), address_buffer) == 0) {
      length = sizeof(in_addr);
      family = AF_INET;
    } else if (uv_inet_pton(AF_INET6, req->hostname.c_str(), address_buffer) == 0) {
      length = sizeof(in6_addr);
      family = AF_INET6;
    } else {
      ModifyActivityQueryCount(channel, -1);
      return UV_EINVAL;
    }

    channel->active_reqs.insert(req);
    ares_gethostbyaddr(channel->channel,
                       address_buffer,
                       length,
                       family,
                       OnReverseQueryComplete,
                       req);
    StartChannelTimer(channel);
    return 0;
  }

  channel->active_reqs.insert(req);
  ares_query_dnsrec(channel->channel,
                    req->hostname.c_str(),
                    ARES_CLASS_IN,
                    method->rec_type,
                    OnDnsQueryComplete,
                    req,
                    nullptr);
  StartChannelTimer(channel);
  return 0;
}

void OnCaresEnvCleanup(void* arg) {
  napi_env env = static_cast<napi_env>(arg);
  CaresEnvState* state = GetCaresState(env);
  if (state == nullptr) return;
  state->cleanup_in_progress = true;

  std::vector<ChannelWrap*> channels;
  channels.reserve(state->channels.size());
  for (ChannelWrap* channel : state->channels) {
    channels.push_back(channel);
  }
  state->channels.clear();

  std::vector<ChannelWrap*> pinned_channels;
  pinned_channels.reserve(channels.size());
  for (ChannelWrap* channel : channels) {
    if (channel == nullptr || channel->env != env || channel->wrapper_ref == nullptr) continue;
    uint32_t ref_count = 0;
    if (napi_reference_ref(env, channel->wrapper_ref, &ref_count) == napi_ok) {
      pinned_channels.push_back(channel);
    }
  }

  for (ChannelWrap* channel : channels) {
    if (channel == nullptr || channel->channel == nullptr) continue;
    CancelChannelRequests(channel);
  }

  for (ChannelWrap* channel : pinned_channels) {
    if (channel == nullptr || channel->env != env || channel->wrapper_ref == nullptr) continue;
    uint32_t ref_count = 0;
    (void)napi_reference_unref(env, channel->wrapper_ref, &ref_count);
  }

  state->cleanup_in_progress = false;
}

void ReqFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* req = static_cast<CaresReqWrap*>(data);
  if (req == nullptr) return;

  req->finalized = true;
  UntrackPendingReq(req);
  QueueReqDestroyIfNeeded(req);
  if (req->pinned_ref && req->req_obj_ref != nullptr) {
    uint32_t ref_count = 0;
    (void)napi_reference_unref(env, req->req_obj_ref, &ref_count);
    req->pinned_ref = false;
  }
  if (req->req_obj_ref != nullptr) {
    napi_delete_reference(env, req->req_obj_ref);
    req->req_obj_ref = nullptr;
  }

  if (req->in_flight) {
    req->orphaned = true;
    return;
  }

  delete req;
}

void ChannelFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* channel = static_cast<ChannelWrap*>(data);
  if (channel == nullptr) return;

  channel->finalized = true;
  UnregisterChannelForEnv(channel);
  if (channel->wrapper_ref != nullptr) {
    napi_delete_reference(env, channel->wrapper_ref);
    channel->wrapper_ref = nullptr;
  }

  if (channel->active_query_count > 0) {
    return;
  }

  ScheduleFinalizedChannelDestroy(channel);
}

napi_value ReqCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, nullptr, &self, &data);

  auto* req = new CaresReqWrap();
  req->env = env;
  if (const char* wrap_name = static_cast<const char*>(data); wrap_name != nullptr) {
    req->async_wrap_name = wrap_name;
  }
  req->async_id = EdgeAsyncWrapNextId(env);
  napi_wrap(env, self, req, ReqFinalize, nullptr, &req->req_obj_ref);
  EdgeAsyncWrapEmitInitString(
      env, req->async_id, req->async_wrap_name.c_str(), EdgeAsyncWrapExecutionAsyncId(env), self);
  return self;
}

napi_value ChannelCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  auto* channel = new ChannelWrap();
  channel->env = env;

  if (argc >= 1 && argv[0] != nullptr) {
    (void)napi_get_value_int32(env, argv[0], &channel->timeout);
  }
  if (argc >= 2 && argv[1] != nullptr) {
    (void)napi_get_value_int32(env, argv[1], &channel->tries);
  }
  if (argc >= 3 && argv[2] != nullptr) {
    (void)napi_get_value_int32(env, argv[2], &channel->max_timeout);
  }

  const int setup_status = SetupChannelCares(channel);
  if (setup_status != ARES_SUCCESS) {
    delete channel;
    napi_throw_error(env, ToErrorCodeString(setup_status), ares_strerror(setup_status));
    return nullptr;
  }

  RegisterChannelForEnv(channel);

  napi_wrap(env, self, channel, ChannelFinalize, nullptr, &channel->wrapper_ref);
  return self;
}

void OnGetAddrInfo(uv_getaddrinfo_t* req, int status, addrinfo* res) {
  auto* wrap = static_cast<CaresReqWrap*>(req->data);
  if (wrap == nullptr) {
    if (res != nullptr) uv_freeaddrinfo(res);
    return;
  }

  napi_env env = wrap->env;
  UntrackPendingReq(wrap);

  if (env == nullptr) {
    MarkReqComplete(wrap);
    if (res != nullptr) uv_freeaddrinfo(res);
    MaybeDeleteReq(wrap);
    return;
  }
  edge::HandleScope scope(env);
  if (!scope.is_open()) {
    MarkReqComplete(wrap);
    if (res != nullptr) uv_freeaddrinfo(res);
    CleanupReqAfterAsync(wrap);
    return;
  }

  napi_value result = MakeNull(env);
  int result_status = status;

  if (status == 0) {
    napi_value arr = nullptr;
    napi_create_array(env, &arr);
    uint32_t n = 0;

    auto add = [&](bool want_ipv4, bool want_ipv6) {
      for (addrinfo* p = res; p != nullptr; p = p->ai_next) {
        if (p->ai_socktype != SOCK_STREAM) continue;

        const void* addr = nullptr;
        if (want_ipv4 && p->ai_family == AF_INET) {
          addr = &reinterpret_cast<sockaddr_in*>(p->ai_addr)->sin_addr;
        } else if (want_ipv6 && p->ai_family == AF_INET6) {
          addr = &reinterpret_cast<sockaddr_in6*>(p->ai_addr)->sin6_addr;
        } else {
          continue;
        }

        char ip[INET6_ADDRSTRLEN] = {0};
        if (uv_inet_ntop(p->ai_family, addr, ip, sizeof(ip)) != 0) continue;

        napi_set_element(env, arr, n++, MakeStringUtf8(env, ip));
      }
    };

    if (wrap->family == 4) {
      add(true, false);
    } else if (wrap->family == 6) {
      add(false, true);
    } else {
      switch (wrap->order) {
        case kDnsOrderIpv4First:
          add(true, false);
          add(false, true);
          break;
        case kDnsOrderIpv6First:
          add(false, true);
          add(true, false);
          break;
        default:
          add(true, true);
          break;
      }
    }

    if (n == 0) {
      result_status = UV_EAI_NODATA;
    }

    result = arr;
  }

  napi_value argv[2] = {MakeInt32(env, result_status), result};
  InvokeOnComplete(env, wrap, 2, argv);

  MarkReqComplete(wrap);
  if (res != nullptr) uv_freeaddrinfo(res);
  CleanupReqAfterAsync(wrap);
}

void OnGetNameInfo(uv_getnameinfo_t* req,
                   int status,
                   const char* hostname,
                   const char* service) {
  auto* wrap = static_cast<CaresReqWrap*>(req->data);
  if (wrap == nullptr) return;

  napi_env env = wrap->env;
  UntrackPendingReq(wrap);

  if (env == nullptr) {
    MarkReqComplete(wrap);
    MaybeDeleteReq(wrap);
    return;
  }
  edge::HandleScope scope(env);
  if (!scope.is_open()) {
    MarkReqComplete(wrap);
    CleanupReqAfterAsync(wrap);
    return;
  }

  napi_value argv[3] = {
      MakeInt32(env, status),
      MakeNull(env),
      MakeNull(env),
  };

  if (status == 0) {
    argv[1] = MakeStringUtf8(env, hostname);
    argv[2] = MakeStringUtf8(env, service);
  }

  InvokeOnComplete(env, wrap, 3, argv);
  MarkReqComplete(wrap);
  CleanupReqAfterAsync(wrap);
}

napi_value CaresGetAddrInfo(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 4) return MakeInt32(env, UV_EINVAL);

  CaresReqWrap* req = nullptr;
  napi_unwrap(env, argv[0], reinterpret_cast<void**>(&req));
  if (req == nullptr) return MakeInt32(env, UV_EINVAL);
  if (req->in_flight) return MakeInt32(env, UV_EBUSY);

  int32_t family = 0;
  int32_t hints_flags = 0;
  napi_get_value_int32(env, argv[2], &family);
  napi_get_value_int32(env, argv[3], &hints_flags);

  if (argc >= 5 && argv[4] != nullptr) {
    int32_t order = 0;
    if (napi_get_value_int32(env, argv[4], &order) == napi_ok) {
      req->order = static_cast<uint8_t>(order);
    }
  }

  req->used_ga = true;
  req->ga.data = req;
  req->env = env;
  req->in_flight = true;
  req->orphaned = false;
  req->family = family;
  req->hostname = ToAsciiHostname(ValueToUtf8(env, argv[1]));

  PinReqObject(req);
  TrackPendingReq(env, req);

  addrinfo hints{};
  hints.ai_socktype = SOCK_STREAM;
  hints.ai_family = (family == 4) ? AF_INET : ((family == 6) ? AF_INET6 : AF_UNSPEC);
  hints.ai_flags = hints_flags;

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  int rc = loop != nullptr
               ? uv_getaddrinfo(loop,
                                &req->ga,
                                OnGetAddrInfo,
                                req->hostname.c_str(),
                                nullptr,
                                &hints)
               : UV_EINVAL;
  if (rc != 0) {
    UntrackPendingReq(req);
    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
  }

  return MakeInt32(env, rc);
}

napi_value CaresGetNameInfo(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 3) return MakeInt32(env, UV_EINVAL);

  CaresReqWrap* req = nullptr;
  napi_unwrap(env, argv[0], reinterpret_cast<void**>(&req));
  if (req == nullptr) return MakeInt32(env, UV_EINVAL);
  if (req->in_flight) return MakeInt32(env, UV_EBUSY);

  req->used_gn = true;
  req->gn.data = req;
  req->env = env;
  req->in_flight = true;
  req->orphaned = false;
  PinReqObject(req);
  TrackPendingReq(env, req);

  const std::string host = ValueToUtf8(env, argv[1]);
  int32_t port = 0;
  napi_get_value_int32(env, argv[2], &port);

  sockaddr_storage storage{};
  int rc = uv_ip4_addr(host.c_str(), port, reinterpret_cast<sockaddr_in*>(&storage));
  if (rc != 0) {
    rc = uv_ip6_addr(host.c_str(), port, reinterpret_cast<sockaddr_in6*>(&storage));
  }

  if (rc == 0) {
    uv_loop_t* loop = EdgeGetEnvLoop(env);
    rc = loop != nullptr
             ? uv_getnameinfo(loop,
                              &req->gn,
                              OnGetNameInfo,
                              reinterpret_cast<const sockaddr*>(&storage),
                              NI_NAMEREQD)
             : UV_EINVAL;
  }

  if (rc != 0) {
    UntrackPendingReq(req);
    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
  }

  return MakeInt32(env, rc);
}

napi_value ConvertIpv6StringToBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1) return MakeNull(env);

  const std::string input = ValueToUtf8(env, argv[0]);
  uint8_t out[16] = {0};
  if (uv_inet_pton(AF_INET6, input.c_str(), out) != 0) {
    return MakeNull(env);
  }

  void* data = nullptr;
  napi_value ab = nullptr;
  if (napi_create_arraybuffer(env, 16, &data, &ab) != napi_ok || data == nullptr) {
    return MakeNull(env);
  }
  memcpy(data, out, 16);

  napi_value arr = nullptr;
  napi_create_typedarray(env, napi_uint8_array, 16, ab, 0, &arr);
  return arr;
}

napi_value CanonicalizeIP(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  const std::string input = argc > 0 ? ValueToUtf8(env, argv[0]) : std::string();
  in_addr addr4{};
  if (!input.empty() && uv_inet_pton(AF_INET, input.c_str(), &addr4) == 0) {
    char out4[INET_ADDRSTRLEN] = {0};
    uv_inet_ntop(AF_INET, &addr4, out4, sizeof(out4));
    return MakeStringUtf8(env, out4);
  }

  in6_addr addr6{};
  if (!input.empty() && uv_inet_pton(AF_INET6, input.c_str(), &addr6) == 0) {
    char out6[INET6_ADDRSTRLEN] = {0};
    uv_inet_ntop(AF_INET6, &addr6, out6, sizeof(out6));
    return MakeStringUtf8(env, out6);
  }

  return MakeStringUtf8(env, "");
}

napi_value CaresStrError(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t code = 0;
  if (argc > 0 && argv[0] != nullptr) {
    (void)napi_get_value_int32(env, argv[0], &code);
  }

  if (code == kDnsESetSrvPending) {
    return MakeStringUtf8(env, "There are pending queries.");
  }

  return MakeStringUtf8(env, ares_strerror(code));
}

void CancelChannelRequests(ChannelWrap* channel) {
  if (channel == nullptr || channel->channel == nullptr) return;

  std::vector<CaresReqWrap*> reqs;
  reqs.reserve(channel->active_reqs.size());
  for (CaresReqWrap* req : channel->active_reqs) {
    reqs.push_back(req);
  }

  ares_cancel(channel->channel);

  for (CaresReqWrap* req : reqs) {
    if (channel->active_reqs.find(req) == channel->active_reqs.end()) continue;
    CompleteQuery(req, ARES_ECANCELLED, nullptr, 0, nullptr, false);
  }
}

napi_value ChannelCancel(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);

  ChannelWrap* channel = nullptr;
  napi_unwrap(env, self, reinterpret_cast<void**>(&channel));
  CancelChannelRequests(channel);

  return MakeUndefined(env);
}

napi_value ChannelGetServers(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);

  ChannelWrap* channel = nullptr;
  napi_unwrap(env, self, reinterpret_cast<void**>(&channel));

  napi_value arr = nullptr;
  napi_create_array(env, &arr);
  if (channel == nullptr || channel->channel == nullptr) return arr;

  ares_addr_port_node* servers = nullptr;
  int rc = ares_get_servers_ports(channel->channel, &servers);
  if (rc != ARES_SUCCESS || servers == nullptr) return arr;

  AresDataPtr free_me(servers);

  uint32_t i = 0;
  for (ares_addr_port_node* cur = servers; cur != nullptr; cur = cur->next, ++i) {
    char ip[INET6_ADDRSTRLEN] = {0};
    const void* caddr = static_cast<const void*>(&cur->addr);
    if (uv_inet_ntop(cur->family, caddr, ip, sizeof(ip)) != 0) continue;

    napi_value pair = nullptr;
    napi_create_array_with_length(env, 2, &pair);
    napi_set_element(env, pair, 0, MakeStringUtf8(env, ip));
    napi_set_element(env, pair, 1, MakeInt32(env, cur->udp_port));
    napi_set_element(env, arr, i, pair);
  }

  return arr;
}

napi_value ChannelSetServers(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  ChannelWrap* channel = nullptr;
  napi_unwrap(env, self, reinterpret_cast<void**>(&channel));
  if (channel == nullptr || channel->channel == nullptr || argc < 1) {
    return MakeInt32(env, UV_EINVAL);
  }

  if (channel->active_query_count > 0) {
    return MakeInt32(env, kDnsESetSrvPending);
  }

  uint32_t len = 0;
  if (napi_get_array_length(env, argv[0], &len) != napi_ok) {
    return MakeInt32(env, ARES_EBADSTR);
  }

  if (len == 0) {
    int rv = ares_set_servers(channel->channel, nullptr);
    return MakeInt32(env, rv);
  }

  std::vector<ares_addr_port_node> servers(len);
  ares_addr_port_node* last = nullptr;
  int err = 0;

  for (uint32_t i = 0; i < len; ++i) {
    napi_value val = nullptr;
    if (napi_get_element(env, argv[0], i, &val) != napi_ok) {
      err = ARES_EBADSTR;
      break;
    }

    uint32_t elem_len = 0;
    if (napi_get_array_length(env, val, &elem_len) != napi_ok || elem_len < 3) {
      err = ARES_EBADSTR;
      break;
    }

    napi_value family_v = nullptr;
    napi_value ip_v = nullptr;
    napi_value port_v = nullptr;
    if (napi_get_element(env, val, 0, &family_v) != napi_ok ||
        napi_get_element(env, val, 1, &ip_v) != napi_ok ||
        napi_get_element(env, val, 2, &port_v) != napi_ok) {
      err = ARES_EBADSTR;
      break;
    }

    int32_t family = 0;
    int32_t port = 0;
    if (napi_get_value_int32(env, family_v, &family) != napi_ok ||
        napi_get_value_int32(env, port_v, &port) != napi_ok) {
      err = ARES_EBADSTR;
      break;
    }

    const std::string ip = ValueToUtf8(env, ip_v);

    ares_addr_port_node* cur = &servers[i];
    memset(cur, 0, sizeof(*cur));
    cur->tcp_port = port;
    cur->udp_port = port;

    switch (family) {
      case 4:
        cur->family = AF_INET;
        err = uv_inet_pton(AF_INET, ip.c_str(), &cur->addr);
        break;
      case 6:
        cur->family = AF_INET6;
        err = uv_inet_pton(AF_INET6, ip.c_str(), &cur->addr);
        break;
      default:
        err = ARES_EBADSTR;
        break;
    }

    if (err != 0) break;

    cur->next = nullptr;
    if (last != nullptr) last->next = cur;
    last = cur;
  }

  if (err == 0) {
    err = ares_set_servers_ports(channel->channel, servers.data());
  } else {
    err = ARES_EBADSTR;
  }

  if (err == ARES_SUCCESS) {
    channel->is_servers_default = false;
  }

  return MakeInt32(env, err);
}

napi_value ChannelSetLocalAddress(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  napi_value self = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, nullptr);

  ChannelWrap* channel = nullptr;
  napi_unwrap(env, self, reinterpret_cast<void**>(&channel));
  if (channel == nullptr || channel->channel == nullptr || argc < 1 || argv[0] == nullptr) {
    napi_throw_error(env, nullptr, "Invalid IP address.");
    return nullptr;
  }

  const std::string ip0 = ValueToUtf8(env, argv[0]);
  unsigned char addr0[sizeof(in6_addr)] = {0};
  unsigned char addr1[sizeof(in6_addr)] = {0};
  int type0 = 0;

  if (uv_inet_pton(AF_INET, ip0.c_str(), addr0) == 0) {
    ares_set_local_ip4(channel->channel, ReadUint32BE(addr0));
    type0 = 4;
  } else if (uv_inet_pton(AF_INET6, ip0.c_str(), addr0) == 0) {
    ares_set_local_ip6(channel->channel, addr0);
    type0 = 6;
  } else {
    napi_throw_error(env, nullptr, "Invalid IP address.");
    return nullptr;
  }

  napi_valuetype second_type = napi_undefined;
  if (argc > 1 && argv[1] != nullptr) {
    napi_typeof(env, argv[1], &second_type);
  }

  const bool has_second = argc > 1 && argv[1] != nullptr &&
                          second_type != napi_undefined && second_type != napi_null;

  if (has_second) {
    const std::string ip1 = ValueToUtf8(env, argv[1]);
    if (uv_inet_pton(AF_INET, ip1.c_str(), addr1) == 0) {
      if (type0 == 4) {
        napi_throw_error(env, nullptr, "Cannot specify two IPv4 addresses.");
        return nullptr;
      }
      ares_set_local_ip4(channel->channel, ReadUint32BE(addr1));
    } else if (uv_inet_pton(AF_INET6, ip1.c_str(), addr1) == 0) {
      if (type0 == 6) {
        napi_throw_error(env, nullptr, "Cannot specify two IPv6 addresses.");
        return nullptr;
      }
      ares_set_local_ip6(channel->channel, addr1);
    } else {
      napi_throw_error(env, nullptr, "Invalid IP address.");
      return nullptr;
    }
  } else {
    if (type0 == 4) {
      memset(addr1, 0, sizeof(addr1));
      ares_set_local_ip6(channel->channel, addr1);
    } else {
      ares_set_local_ip4(channel->channel, 0);
    }
  }

  return MakeUndefined(env);
}

napi_value ChannelQueryNative(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value self = nullptr;
  void* callback_data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, &self, &callback_data);

  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }

  ChannelWrap* channel = nullptr;
  napi_unwrap(env, self, reinterpret_cast<void**>(&channel));
  if (channel == nullptr || channel->channel == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }

  auto* req = static_cast<CaresReqWrap*>(nullptr);
  napi_unwrap(env, argv[0], reinterpret_cast<void**>(&req));
  if (req == nullptr) return MakeInt32(env, UV_EINVAL);
  if (req->in_flight) return MakeInt32(env, UV_EBUSY);

  const auto* method = static_cast<const QueryMethodData*>(callback_data);
  if (method == nullptr || method->binding_name == nullptr) return MakeInt32(env, UV_EINVAL);

  req->env = env;
  req->hostname = ValueToUtf8(env, argv[1]);
  req->in_flight = true;
  req->orphaned = false;

  napi_value ttl_value = nullptr;
  bool ttl = false;
  if (napi_get_named_property(env, argv[0], "ttl", &ttl_value) == napi_ok && ttl_value != nullptr) {
    (void)napi_get_value_bool(env, ttl_value, &ttl);
  }
  req->ttl = ttl;

  PinReqObject(req);
  TrackPendingReq(env, req);
  HoldChannelActiveRef(channel, self);

  const int rc = DispatchQuery(channel, req, method);
  if (rc != 0) {
    UntrackPendingReq(req);
    MarkReqComplete(req);
    CleanupReqAfterAsync(req);
  }

  return MakeInt32(env, rc);
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void SetNamedU32(napi_env env, napi_value obj, const char* name, uint32_t value) {
  napi_value v = nullptr;
  napi_create_uint32(env, value, &v);
  if (v != nullptr) napi_set_named_property(env, obj, name, v);
}

constexpr QueryMethodData kQueryAnyMethod{"queryAny", QueryKind::kAny, ARES_REC_TYPE_ANY};
constexpr QueryMethodData kQueryAMethod{"queryA", QueryKind::kA, ARES_REC_TYPE_A};
constexpr QueryMethodData kQueryAaaaMethod{"queryAaaa", QueryKind::kAaaa, ARES_REC_TYPE_AAAA};
constexpr QueryMethodData kQueryCaaMethod{"queryCaa", QueryKind::kCaa, ARES_REC_TYPE_CAA};
constexpr QueryMethodData kQueryCnameMethod{"queryCname", QueryKind::kCname, ARES_REC_TYPE_CNAME};
constexpr QueryMethodData kQueryMxMethod{"queryMx", QueryKind::kMx, ARES_REC_TYPE_MX};
constexpr QueryMethodData kQueryNsMethod{"queryNs", QueryKind::kNs, ARES_REC_TYPE_NS};
constexpr QueryMethodData kQueryTlsaMethod{"queryTlsa", QueryKind::kTlsa, ARES_REC_TYPE_TLSA};
constexpr QueryMethodData kQueryTxtMethod{"queryTxt", QueryKind::kTxt, ARES_REC_TYPE_TXT};
constexpr QueryMethodData kQuerySrvMethod{"querySrv", QueryKind::kSrv, ARES_REC_TYPE_SRV};
constexpr QueryMethodData kQueryPtrMethod{"queryPtr", QueryKind::kPtr, ARES_REC_TYPE_PTR};
constexpr QueryMethodData kQueryNaptrMethod{"queryNaptr", QueryKind::kNaptr, ARES_REC_TYPE_NAPTR};
constexpr QueryMethodData kQuerySoaMethod{"querySoa", QueryKind::kSoa, ARES_REC_TYPE_SOA};
constexpr QueryMethodData kGetHostByAddrMethod{"getHostByAddr", QueryKind::kReverse, ARES_REC_TYPE_PTR};

}  // namespace

void EdgeRunCaresWrapEnvCleanup(napi_env env) {
  if (env == nullptr) return;
  OnCaresEnvCleanup(env);
}

napi_value EdgeInstallCaresWrapBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_value req_ctor = nullptr;
  if (napi_define_class(env,
                        "QueryReqWrap",
                        NAPI_AUTO_LENGTH,
                        ReqCtor,
                        const_cast<char*>("QUERYWRAP"),
                        0,
                        nullptr,
                        &req_ctor) != napi_ok) {
    return nullptr;
  }
  napi_value req2_ctor = nullptr;
  if (napi_define_class(env,
                        "GetAddrInfoReqWrap",
                        NAPI_AUTO_LENGTH,
                        ReqCtor,
                        const_cast<char*>("GETADDRINFOREQWRAP"),
                        0,
                        nullptr,
                        &req2_ctor) !=
      napi_ok) {
    return nullptr;
  }
  napi_value req3_ctor = nullptr;
  if (napi_define_class(env,
                        "GetNameInfoReqWrap",
                        NAPI_AUTO_LENGTH,
                        ReqCtor,
                        const_cast<char*>("GETNAMEINFOREQWRAP"),
                        0,
                        nullptr,
                        &req3_ctor) !=
      napi_ok) {
    return nullptr;
  }

  napi_property_descriptor channel_props[] = {
      {"cancel", nullptr, ChannelCancel, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"getServers", nullptr, ChannelGetServers, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"setServers", nullptr, ChannelSetServers, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"setLocalAddress", nullptr, ChannelSetLocalAddress, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), nullptr},
      {"queryAny", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryAnyMethod)},
      {"queryA", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryAMethod)},
      {"queryAaaa", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryAaaaMethod)},
      {"queryCaa", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryCaaMethod)},
      {"queryCname", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryCnameMethod)},
      {"queryMx", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryMxMethod)},
      {"queryNs", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryNsMethod)},
      {"queryTlsa", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryTlsaMethod)},
      {"queryTxt", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryTxtMethod)},
      {"querySrv", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQuerySrvMethod)},
      {"queryPtr", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryPtrMethod)},
      {"queryNaptr", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQueryNaptrMethod)},
      {"querySoa", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kQuerySoaMethod)},
      {"getHostByAddr", nullptr, ChannelQueryNative, nullptr, nullptr, nullptr, static_cast<napi_property_attributes>(napi_writable | napi_configurable), const_cast<QueryMethodData*>(&kGetHostByAddrMethod)},
  };

  napi_value channel_ctor = nullptr;
  if (napi_define_class(env,
                        "ChannelWrap",
                        NAPI_AUTO_LENGTH,
                        ChannelCtor,
                        nullptr,
                        sizeof(channel_props) / sizeof(channel_props[0]),
                        channel_props,
                        &channel_ctor) != napi_ok) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "QueryReqWrap", req_ctor);
  napi_set_named_property(env, binding, "GetAddrInfoReqWrap", req2_ctor);
  napi_set_named_property(env, binding, "GetNameInfoReqWrap", req3_ctor);
  napi_set_named_property(env, binding, "ChannelWrap", channel_ctor);

  SetMethod(env, binding, "getaddrinfo", CaresGetAddrInfo);
  SetMethod(env, binding, "getnameinfo", CaresGetNameInfo);
  SetMethod(env, binding, "strerror", CaresStrError);
  SetMethod(env, binding, "convertIpv6StringToBuffer", ConvertIpv6StringToBuffer);
  SetMethod(env, binding, "canonicalizeIP", CanonicalizeIP);

  SetNamedU32(env, binding, "AF_INET", AF_INET);
  SetNamedU32(env, binding, "AF_INET6", AF_INET6);
  SetNamedU32(env, binding, "AF_UNSPEC", AF_UNSPEC);
  SetNamedU32(env, binding, "AI_ADDRCONFIG", AI_ADDRCONFIG);
  SetNamedU32(env, binding, "AI_ALL", AI_ALL);
  SetNamedU32(env, binding, "AI_V4MAPPED", AI_V4MAPPED);

  SetNamedU32(env, binding, "DNS_ORDER_VERBATIM", kDnsOrderVerbatim);
  SetNamedU32(env, binding, "DNS_ORDER_IPV4_FIRST", kDnsOrderIpv4First);
  SetNamedU32(env, binding, "DNS_ORDER_IPV6_FIRST", kDnsOrderIpv6First);

  return binding;
}
