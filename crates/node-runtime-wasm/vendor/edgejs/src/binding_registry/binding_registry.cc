#include "binding_registry/binding_registry.h"

#include <algorithm>
#include <array>
#include <cstdio>
#include <string>
#include <unordered_map>

#include "binding_registry/binding_list.h"
#include "edge_buffer.h"
#include "edge_cares_wrap.h"
#include "edge_encoding.h"
#include "edge_environment.h"
#include "edge_errors_binding.h"
#include "edge_http_parser.h"
#include "edge_js_stream.h"
#include "edge_js_udp_wrap.h"
#include "edge_loader_bindings.h"
#include "edge_os.h"
#include "edge_pipe_wrap.h"
#include "edge_process.h"
#include "edge_process_wrap.h"
#include "edge_signal_wrap.h"
#include "edge_spawn_sync.h"
#include "edge_string_decoder.h"
#include "edge_stream_wrap.h"
#include "edge_task_queue.h"
#include "edge_tcp_wrap.h"
#include "edge_tls_wrap.h"
#include "edge_timers_host.h"
#include "edge_trace.h"
#include "edge_tty_wrap.h"
#include "edge_udp_wrap.h"
#include "edge_url.h"
#include "internal_binding/binding_initializers.h"
#include "edge_util.h"

namespace edge::binding_registry {
namespace {

struct BindingEntry {
  std::string_view name;
  BindingInit init;
};

#define EDGE_BINDING_REGISTRY_ENTRY(name, init) BindingEntry{#name, init},
constexpr std::array<BindingEntry, 63> kBindings = {{
    EDGE_BINDING_REGISTRY_LIST(EDGE_BINDING_REGISTRY_ENTRY)
}};
#undef EDGE_BINDING_REGISTRY_ENTRY

constexpr bool IsSortedAndUnique() {
  for (size_t i = 1; i < kBindings.size(); ++i) {
    if (!(kBindings[i - 1].name < kBindings[i].name)) {
      return false;
    }
  }
  return true;
}

static_assert(IsSortedAndUnique(), "binding registry manifest must be sorted and unique");

napi_value Undefined(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

bool IsUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_undefined;
}

bool HasPendingException(napi_env env) {
  bool has_pending_exception = false;
  return napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception;
}

const BindingEntry* FindEntry(std::string_view name) {
  auto it = std::lower_bound(
      kBindings.begin(),
      kBindings.end(),
      name,
      [](const BindingEntry& entry, std::string_view value) {
        return entry.name < value;
      });
  if (it == kBindings.end() || it->name != name) return nullptr;
  return &*it;
}

struct RegistryState {
  explicit RegistryState(napi_env env_in) : env(env_in) {}

  void Finalize() {
    if (finalized) return;
    finalized = true;
    for (auto& entry : cache) {
      if (entry.second != nullptr) {
        napi_delete_reference(env, entry.second);
        entry.second = nullptr;
      }
    }
    cache.clear();
  }

  napi_env env = nullptr;
  bool finalized = false;
  std::unordered_map<std::string, napi_ref> cache;
};

void DeleteRegistryState(void* data) {
  auto* state = static_cast<RegistryState*>(data);
  if (state == nullptr) return;
  state->Finalize();
  delete state;
}

RegistryState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<RegistryState>(env, kEdgeEnvironmentSlotBindingRegistryState);
}

RegistryState* GetOrCreateState(napi_env env) {
  if (env == nullptr) return nullptr;
  if (auto* existing = GetState(env); existing != nullptr) {
    return existing;
  }
  auto* created = new RegistryState(env);
  EdgeEnvironmentSetOpaqueSlot(env, kEdgeEnvironmentSlotBindingRegistryState, created, DeleteRegistryState);
  return created;
}

napi_value GetCached(RegistryState* state, napi_env env, std::string_view name) {
  if (state == nullptr || name.empty()) return nullptr;
  auto it = state->cache.find(std::string(name));
  if (it == state->cache.end() || it->second == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, it->second, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

void Cache(RegistryState* state, napi_env env, std::string_view name, napi_value value) {
  if (state == nullptr || name.empty() || value == nullptr || IsUndefined(env, value)) return;
  std::string key(name);
  auto it = state->cache.find(key);
  if (it != state->cache.end() && it->second != nullptr) {
    napi_delete_reference(env, it->second);
    it->second = nullptr;
  }
  napi_ref ref = nullptr;
  if (napi_create_reference(env, value, 1, &ref) != napi_ok || ref == nullptr) return;
  state->cache[std::move(key)] = ref;
}

void TraceRequest(std::string_view name) {
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_INTERNAL_BINDING")) {
    std::fprintf(stderr, "EDGE_TRACE_INTERNAL_BINDING request %.*s\n", static_cast<int>(name.size()), name.data());
  }
}

void TraceCacheHit(std::string_view name) {
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_INTERNAL_BINDING")) {
    std::fprintf(stderr, "EDGE_TRACE_INTERNAL_BINDING cache-hit %.*s\n", static_cast<int>(name.size()), name.data());
  }
}

void TraceResolved(napi_env env, std::string_view name, napi_value value) {
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_INTERNAL_BINDING")) {
    napi_valuetype resolved_type = napi_undefined;
    (void)napi_typeof(env, value, &resolved_type);
    std::fprintf(stderr,
                 "EDGE_TRACE_INTERNAL_BINDING resolved %.*s type=%d\n",
                 static_cast<int>(name.size()),
                 name.data(),
                 static_cast<int>(resolved_type));
  }
}

}  // namespace

bool Has(std::string_view name) {
  return FindEntry(name) != nullptr;
}

std::vector<std::string_view> Names() {
  std::vector<std::string_view> names;
  names.reserve(kBindings.size());
  for (const auto& entry : kBindings) {
    names.push_back(entry.name);
  }
  return names;
}

napi_value Get(napi_env env, std::string_view name) {
  if (env == nullptr) return nullptr;
  TraceRequest(name);

  RegistryState* state = GetOrCreateState(env);
  if (state == nullptr || state->finalized) return nullptr;

  if (name.empty() || !Has(name)) {
    return Undefined(env);
  }

  napi_value cached = GetCached(state, env, name);
  if (cached != nullptr) {
    TraceCacheHit(name);
    return cached;
  }

  const BindingEntry* entry = FindEntry(name);
  napi_value resolved = nullptr;
  if (entry != nullptr && entry->init != nullptr) {
    resolved = entry->init(env);
  } else {
    resolved = Undefined(env);
  }

  if (resolved == nullptr) {
    return HasPendingException(env) ? nullptr : Undefined(env);
  }

  TraceResolved(env, name, resolved);
  Cache(state, env, name, resolved);
  return resolved;
}

void FinalizeEnv(napi_env env) {
  RegistryState* state = GetState(env);
  if (state == nullptr) return;
  state->Finalize();
}

}  // namespace edge::binding_registry
