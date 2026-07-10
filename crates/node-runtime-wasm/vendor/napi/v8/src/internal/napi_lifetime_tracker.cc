#include "internal/napi_lifetime_tracker.h"

#include "internal/napi_ref.h"
#include "internal/napi_v8_env.h"
#include <napi_lifetime_tracker.h>

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <mutex>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace v8impl::detail {
namespace {

bool enabled() {
  return napi::lifetime::env_flag_enabled("EDGE_TRACE_NAPI_LIFETIME");
}

bool periodic_stats_enabled() {
  return napi::lifetime::env_flag_enabled_or("EDGE_TRACE_NAPI_LIFETIME_STATS",
                                             enabled());
}

int64_t monotonic_milliseconds() {
  return napi::lifetime::monotonic_milliseconds();
}

#ifdef NAPI_ENABLE_LIFETIME_TRACKER

using type_stats = napi::lifetime::type_stats;

#if defined(NAPI_ENABLE_LIFETIME_TAG_STATS) || \
    defined(NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP)
enum class value_tag : std::size_t {
  kBigInt,
  kSymbol,
  kString,
  kFunction,
  kObject,
  kExternal,
  kArray,
  kArrayBuffer,
  kTypedArray,
  kDataView,
  kPromise,
  kInt,
  kUint32,
  kBool,
  kNull,
  kUndefined,
  kNumber,
  kUnknown,
  kCount,
};

constexpr std::size_t k_tag_bucket_count =
    static_cast<std::size_t>(value_tag::kCount);

const char* tag_bucket_name(std::size_t index) {
  switch (static_cast<value_tag>(index)) {
    case value_tag::kBigInt:
      return "big_int";
    case value_tag::kSymbol:
      return "symbol";
    case value_tag::kString:
      return "string";
    case value_tag::kFunction:
      return "function";
    case value_tag::kObject:
      return "object";
    case value_tag::kExternal:
      return "external";
    case value_tag::kArray:
      return "array";
    case value_tag::kArrayBuffer:
      return "array_buffer";
    case value_tag::kTypedArray:
      return "typed_array";
    case value_tag::kDataView:
      return "data_view";
    case value_tag::kPromise:
      return "promise";
    case value_tag::kInt:
      return "int";
    case value_tag::kUint32:
      return "uint32";
    case value_tag::kBool:
      return "bool";
    case value_tag::kNull:
      return "null";
    case value_tag::kUndefined:
      return "undefined";
    case value_tag::kNumber:
      return "float64";
    default:
      return "unknown";
  }
}

struct tag_counters {
  std::array<std::size_t, k_tag_bucket_count> slots{};
};

void add_tag(tag_counters& counters, value_tag tag) {
  ++counters.slots[static_cast<std::size_t>(tag)];
}

void remove_tag(tag_counters& counters, value_tag tag) {
  std::size_t& slot = counters.slots[static_cast<std::size_t>(tag)];
  if (slot > 0) {
    --slot;
  }
}

bool has_tags(const tag_counters& counters) {
  return std::any_of(counters.slots.begin(), counters.slots.end(), [](auto count) {
    return count != 0;
  });
}

value_tag classify_value(v8::Local<v8::Value> value) {
  if (value.IsEmpty()) return value_tag::kUnknown;
  if (value->IsUndefined()) return value_tag::kUndefined;
  if (value->IsNull()) return value_tag::kNull;
  if (value->IsBoolean()) return value_tag::kBool;
  if (value->IsInt32()) return value_tag::kInt;
  if (value->IsUint32()) return value_tag::kUint32;
  if (value->IsNumber()) return value_tag::kNumber;
  if (value->IsBigInt()) return value_tag::kBigInt;
  if (value->IsSymbol()) return value_tag::kSymbol;
  if (value->IsString()) return value_tag::kString;
  if (value->IsFunction()) return value_tag::kFunction;
  if (value->IsExternal()) return value_tag::kExternal;
  if (value->IsArray()) return value_tag::kArray;
  if (value->IsArrayBuffer()) return value_tag::kArrayBuffer;
  if (value->IsTypedArray()) return value_tag::kTypedArray;
  if (value->IsDataView()) return value_tag::kDataView;
  if (value->IsPromise()) return value_tag::kPromise;
  if (value->IsObject()) return value_tag::kObject;
  return value_tag::kUnknown;
}
#endif

#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
std::string escaped_v8_string(v8::Isolate* isolate, v8::Local<v8::Value> value) {
  if (isolate == nullptr || value.IsEmpty() || !value->IsString()) {
    return {};
  }
  v8::Local<v8::String> string = value.As<v8::String>();
  int length = string->Utf8Length(isolate);
  if (length <= 0) return {};

  std::string utf8(static_cast<std::size_t>(length), '\0');
  int written = string->WriteUtf8(isolate,
                                  utf8.data(),
                                  length,
                                  nullptr,
                                  v8::String::NO_NULL_TERMINATION |
                                      v8::String::REPLACE_INVALID_UTF8);
  if (written <= 0) return {};
  utf8.resize(static_cast<std::size_t>(written));
  return napi::lifetime::escaped_value_fragment(
      utf8.data(), utf8.size());
}

std::string object_prototype_name(v8::Isolate* isolate, v8::Local<v8::Value> value) {
  if (value.IsEmpty() || !value->IsObject()) {
    return {};
  }

  v8::Local<v8::String> name =
      value.As<v8::Object>()->GetConstructorName();
  std::string result = escaped_v8_string(isolate, name);
  return result.empty() ? "<object>" : result;
}

#endif

struct value_snapshot {
  napi_env env = nullptr;
  const void* scope_key = nullptr;
  std::size_t scope_level = 0;
#if defined(NAPI_ENABLE_LIFETIME_TAG_STATS) || \
    defined(NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP)
  value_tag tag = value_tag::kUnknown;
#endif
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  bool has_string_symbol = false;
  std::string string_symbol_value;
  bool has_object_type = false;
  std::string object_type;
#endif
};

struct value_type_stats : type_stats {
#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
  tag_counters tags;
#endif
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  std::vector<napi::lifetime::string_symbol_entry<value_tag>> string_symbols;
  std::vector<napi::lifetime::object_type_entry> object_types;
#endif
};

struct live_record {
  napi_env env = nullptr;
  const char* type_name = "unknown";
  std::size_t order = 0;
  bool has_value_snapshot = false;
  value_snapshot snapshot;
};

struct lifetime_state {
  std::mutex mutex;
  std::unordered_map<void*, live_record> live;
  std::unordered_map<std::string, type_stats> types;
  value_type_stats values;
  value_type_stats refs;
  std::unordered_map<const void*, std::vector<value_snapshot>> scope_values;
  std::size_t next_order = 1;
  std::size_t scope_escape_calls = 0;
  std::size_t scope_escape_succeeded = 0;
  std::size_t scope_escape_failed = 0;
  napi::lifetime::counter_history counters;
};

lifetime_state g_lifetime;

bool is_napi_ref_type(const char* type_name) {
  return type_name != nullptr &&
         (std::strcmp(type_name, "napi_ref") == 0 ||
          std::strcmp(type_name, "napi_ref_with_data") == 0 ||
          std::strcmp(type_name, "napi_ref_with_finalizer") == 0);
}

const void* value_scope_key(napi_env env, std::size_t parent_scope_depth) {
  if (env == nullptr) {
    return nullptr;
  }
  if (env->open_handle_scope_stack.size() <= parent_scope_depth) {
    return env;
  }
  return env->open_handle_scope_stack[env->open_handle_scope_stack.size() - 1 -
                                      parent_scope_depth];
}

std::size_t value_scope_level(napi_env env, std::size_t parent_scope_depth) {
  if (env == nullptr || env->open_handle_scope_stack.size() <= parent_scope_depth) {
    return 0;
  }
  return env->open_handle_scope_stack.size() - parent_scope_depth;
}

value_snapshot capture_value_snapshot(napi_env env,
                                      v8::Local<v8::Value> local,
                                      std::size_t parent_scope_depth) {
  value_snapshot snapshot;
  snapshot.env = env;
  snapshot.scope_key = value_scope_key(env, parent_scope_depth);
  snapshot.scope_level = value_scope_level(env, parent_scope_depth);
#if defined(NAPI_ENABLE_LIFETIME_TAG_STATS) || \
    defined(NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP)
  if (env == nullptr || env->isolate == nullptr || local.IsEmpty()) {
    return snapshot;
  }
  snapshot.tag = classify_value(local);
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  if (snapshot.tag == value_tag::kString) {
    snapshot.string_symbol_value = escaped_v8_string(env->isolate, local);
    snapshot.has_string_symbol = !snapshot.string_symbol_value.empty();
  } else if (snapshot.tag == value_tag::kSymbol) {
    v8::Local<v8::Value> description =
        local.As<v8::Symbol>()->Description(env->isolate);
    if (!description.IsEmpty() && description->IsString()) {
      snapshot.string_symbol_value = escaped_v8_string(env->isolate, description);
      snapshot.has_string_symbol = !snapshot.string_symbol_value.empty();
    }
  }

  if (local->IsObject()) {
    snapshot.object_type = object_prototype_name(env->isolate, local);
    snapshot.has_object_type = !snapshot.object_type.empty();
  }
#endif
#endif
  return snapshot;
}

value_snapshot capture_ref_snapshot(napi_env env, void* value) {
  auto* ref = static_cast<napi_ref__*>(value);
  if (env == nullptr || env->isolate == nullptr || ref == nullptr) {
    value_snapshot snapshot;
    snapshot.env = env;
    return snapshot;
  }

  v8::HandleScope handle_scope(env->isolate);
  v8::Local<v8::Value> local = ref->Get();
  if (local.IsEmpty()) {
    value_snapshot snapshot;
    snapshot.env = env;
    return snapshot;
  }
  return capture_value_snapshot(env, local, 0);
}

void add_value_snapshot(value_type_stats& stats, const value_snapshot& snapshot) {
#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
  add_tag(stats.tags, snapshot.tag);
#endif
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  if (snapshot.has_string_symbol) {
    napi::lifetime::add_string_symbol_entry(stats.string_symbols,
                                            snapshot.tag,
                                            snapshot.string_symbol_value);
  }
  if (snapshot.has_object_type) {
    napi::lifetime::add_object_type_entry(stats.object_types,
                                          snapshot.object_type);
  }
#endif
}

void remove_value_snapshot(value_type_stats& stats,
                           const value_snapshot& snapshot) {
#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
  remove_tag(stats.tags, snapshot.tag);
#endif
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  if (snapshot.has_string_symbol) {
    napi::lifetime::remove_string_symbol_entry(stats.string_symbols,
                                               snapshot.tag,
                                               snapshot.string_symbol_value);
  }
  if (snapshot.has_object_type) {
    napi::lifetime::remove_object_type_entry(stats.object_types,
                                             snapshot.object_type);
  }
#endif
}

void record_ref_create_locked(const value_snapshot& snapshot) {
  napi::lifetime::record_create(g_lifetime.refs);
  add_value_snapshot(g_lifetime.refs, snapshot);
}

void record_ref_release_locked(const value_snapshot& snapshot) {
  remove_value_snapshot(g_lifetime.refs, snapshot);
  napi::lifetime::record_release(g_lifetime.refs);
}

void record_value_create_locked(napi_env env,
                                v8::Local<v8::Value> value,
                                std::size_t parent_scope_depth) {
  if (env == nullptr || value.IsEmpty()) {
    return;
  }

  value_snapshot snapshot =
      capture_value_snapshot(env, value, parent_scope_depth);
  napi::lifetime::record_create(g_lifetime.values);
  add_value_snapshot(g_lifetime.values, snapshot);
  g_lifetime.scope_values[snapshot.scope_key].push_back(std::move(snapshot));
}

void record_scope_values_release_locked(napi_env env, const void* scope) {
  const void* key = scope == nullptr ? static_cast<const void*>(env) : scope;
  auto it = g_lifetime.scope_values.find(key);
  if (it == g_lifetime.scope_values.end()) {
    return;
  }

  for (const auto& snapshot : it->second) {
    remove_value_snapshot(g_lifetime.values, snapshot);
    napi::lifetime::record_release(g_lifetime.values);
  }
  g_lifetime.scope_values.erase(it);
}

void record_create_locked(napi_env env, void* value, const char* type_name) {
  if (value == nullptr) return;

  const char* effective_type = type_name == nullptr ? "unknown" : type_name;
  auto existing = g_lifetime.live.find(value);
  if (existing != g_lifetime.live.end()) {
    auto& old_stats = g_lifetime.types[existing->second.type_name];
    if (old_stats.active > 0) {
      --old_stats.active;
    }
    if (existing->second.has_value_snapshot) {
      record_ref_release_locked(existing->second.snapshot);
    }
    g_lifetime.live.erase(existing);
  }

  auto& stats = g_lifetime.types[effective_type];
  napi::lifetime::record_create(stats);

  live_record record;
  record.env = env;
  record.type_name = effective_type;
  record.order = g_lifetime.next_order++;
  if (is_napi_ref_type(effective_type)) {
    record.has_value_snapshot = true;
    record.snapshot = capture_ref_snapshot(env, value);
    record_ref_create_locked(record.snapshot);
  }
  g_lifetime.live.emplace(value, std::move(record));
}

void record_release_locked(void* value, const char* type_name) {
  if (value == nullptr) return;

  auto live_it = g_lifetime.live.find(value);
  const char* effective_type = type_name == nullptr ? "unknown" : type_name;
  if (live_it != g_lifetime.live.end()) {
    effective_type = live_it->second.type_name;
    if (live_it->second.has_value_snapshot) {
      record_ref_release_locked(live_it->second.snapshot);
    }
    g_lifetime.live.erase(live_it);
  }

  auto& stats = g_lifetime.types[effective_type];
  napi::lifetime::record_release(stats);
}

type_stats type_stats_or_empty(const char* type_name) {
  auto it = g_lifetime.types.find(type_name);
  return it == g_lifetime.types.end() ? type_stats{} : it->second;
}

type_stats ref_type_stats() {
  type_stats stats;
  napi::lifetime::add_type_stats(stats, type_stats_or_empty("napi_ref"));
  napi::lifetime::add_type_stats(stats,
                                 type_stats_or_empty("napi_ref_with_data"));
  napi::lifetime::add_type_stats(
      stats, type_stats_or_empty("napi_ref_with_finalizer"));
  return stats;
}

napi::lifetime::env_slot_scan scan_env_slots(napi_env env) {
  napi::lifetime::env_slot_scan scan;
  if (env == nullptr) {
    return scan;
  }

  scan.value_slots_total = g_lifetime.values.active;
  scan.active_values = g_lifetime.values.active;
  std::unordered_map<std::size_t, std::size_t> active_values_by_scope_level;
  for (const auto& [_, snapshots] : g_lifetime.scope_values) {
    for (const auto& snapshot : snapshots) {
      if (snapshot.env == env) {
        active_values_by_scope_level[snapshot.scope_level]++;
      }
    }
  }
  scan.active_values_by_scope_level.reserve(active_values_by_scope_level.size());
  for (const auto& [level, count] : active_values_by_scope_level) {
    scan.active_values_by_scope_level.push_back({level, count});
  }
  std::sort(scan.active_values_by_scope_level.begin(),
            scan.active_values_by_scope_level.end(),
            [](const auto& left, const auto& right) {
              return left.first < right.first;
            });

  type_stats ref_stats = ref_type_stats();
  type_stats handle_scope_stats = type_stats_or_empty("napi_handle_scope");
  type_stats escapable_scope_stats =
      type_stats_or_empty("napi_escapable_handle_scope");
  scan.ref_slots_total = ref_stats.created;
  scan.active_refs = ref_stats.active;
  scan.scope_slots_total =
      handle_scope_stats.created + escapable_scope_stats.created;
  scan.active_scopes =
      handle_scope_stats.active + escapable_scope_stats.active;
  return scan;
}

#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
void dump_scope_value_tag_tables(napi_env env) {
  std::map<std::size_t, tag_counters> by_scope_level;
  for (const auto& [_, snapshots] : g_lifetime.scope_values) {
    for (const auto& snapshot : snapshots) {
      if (snapshot.env == env) {
        add_tag(by_scope_level[snapshot.scope_level], snapshot.tag);
      }
    }
  }

  for (const auto& [level, counters] : by_scope_level) {
    if (!has_tags(counters)) {
      continue;
    }
    std::fprintf(stderr,
                 "[napi-lifetime-scope-values] scope_level=%zu owner=napi_value\n",
                 level);
    std::fprintf(stderr,
                 "  %-16s %10s %10s %10s\n",
                 "tag",
                 "x[i-1]",
                 "speed",
                 "accel");
    for (std::size_t i = 0; i < k_tag_bucket_count; ++i) {
      std::size_t count = counters.slots[i];
      if (count == 0) {
        continue;
      }
      const char* name = tag_bucket_name(i);
      napi::lifetime::counter_trend trend = napi::lifetime::observe_counter(
          g_lifetime.counters,
          std::string("scope.tag.") + std::to_string(level) + "." + name,
          count);
      char current[32];
      char speed[32];
      char acceleration[32];
      napi::lifetime::format_center_value(current, sizeof(current), trend);
      napi::lifetime::format_delta_value(speed, sizeof(speed), trend, false);
      napi::lifetime::format_delta_value(acceleration,
                                         sizeof(acceleration),
                                         trend,
                                         true);
      std::fprintf(stderr,
                   "  %-16s %10s %10s %10s\n",
                   name,
                   current,
                   speed,
                   acceleration);
    }
  }
}
#endif

void dump_stats_locked(napi_env env, bool include_string_symbol_values) {
  napi::lifetime::dump_lifetime_header();
  napi::lifetime::env_slot_scan scan = scan_env_slots(env);
  napi::lifetime::dump_slots_table(g_lifetime.counters,
                                   scan,
                                   g_lifetime.values.active,
                                   g_lifetime.refs.active,
                                   g_lifetime.scope_escape_calls,
                                   g_lifetime.scope_escape_succeeded,
                                   g_lifetime.scope_escape_failed);
  napi::lifetime::dump_scope_level_table(g_lifetime.counters, scan);

  napi::lifetime::dump_type_table_header();
  napi::lifetime::dump_type_row(g_lifetime.counters,
                                "napi_value",
                                g_lifetime.values);

  std::vector<std::pair<std::string, type_stats>> rows(g_lifetime.types.begin(),
                                                       g_lifetime.types.end());
  std::sort(rows.begin(), rows.end(), [](const auto& lhs, const auto& rhs) {
    return lhs.first < rhs.first;
  });
  for (const auto& [type, stats] : rows) {
    napi::lifetime::dump_type_row(g_lifetime.counters, type.c_str(), stats);
  }

#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
  napi::lifetime::dump_tag_table(g_lifetime.counters,
                                 "napi_value",
                                 g_lifetime.values.tags,
                                 k_tag_bucket_count,
                                 tag_bucket_name);
  napi::lifetime::dump_tag_table(g_lifetime.counters,
                                 "napi_ref",
                                 g_lifetime.refs.tags,
                                 k_tag_bucket_count,
                                 tag_bucket_name);
  dump_scope_value_tag_tables(env);
#endif
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  if (include_string_symbol_values) {
    napi::lifetime::dump_string_entries(
        g_lifetime.counters,
        g_lifetime.values.string_symbols,
        [](value_tag tag) {
          return tag == value_tag::kString || tag == value_tag::kSymbol;
        },
        "napi_ref");
    napi::lifetime::dump_object_type_entries(g_lifetime.counters,
                                             g_lifetime.values.object_types,
                                             g_lifetime.refs.object_types);
  }
#else
  (void)include_string_symbol_values;
#endif
  std::fprintf(stderr, "\n");
}

void dump_summary_locked(napi_env env) {
  napi::lifetime::dump_summary_row(scan_env_slots(env));
}

void dump_lifetime(napi_env env,
                   const char* reason,
                   bool include_string_symbol_values) {
  if (reason != nullptr) {
    std::fprintf(stderr, "[napi-lifetime] dump env=%p reason=%s\n", env, reason);
  }

  std::lock_guard<std::mutex> lock{g_lifetime.mutex};
  dump_stats_locked(env, include_string_symbol_values);
}

void dump_lifetime_summary(napi_env env) {
  std::lock_guard<std::mutex> lock{g_lifetime.mutex};
  dump_summary_locked(env);
}

#ifdef NAPI_ENABLE_LIFETIME_PERIODIC_STATS
void maybe_dump_periodic_stats(napi_env env) {
  if (env == nullptr || !periodic_stats_enabled()) {
    return;
  }

  int64_t now = monotonic_milliseconds();
  bool should_dump_summary = env->should_dump_lifetime_stats(now);
  bool should_dump_full = false;
#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
  should_dump_full = env->should_dump_lifetime_string_symbol_values(now);
#endif

  if (should_dump_full) {
    dump_lifetime(env, nullptr, true);
    return;
  }

  if (should_dump_summary) {
    dump_lifetime_summary(env);
  }
}
#else
void maybe_dump_periodic_stats(napi_env env) {
  (void)env;
}
#endif

#endif  // NAPI_ENABLE_LIFETIME_TRACKER

}  // namespace

#ifdef NAPI_ENABLE_LIFETIME_TRACKER
void napi_lifetime_tracker__::record_create_raw(napi_env env,
                                                void* value,
                                                const char* type_name) {
  {
    std::lock_guard<std::mutex> lock{g_lifetime.mutex};
    record_create_locked(env, value, type_name);
  }
  maybe_dump_periodic_stats(env);
}

void napi_lifetime_tracker__::record_release_raw(napi_env env,
                                                 void* value,
                                                 const char* type_name) {
  napi_env owner = env;
  {
    std::lock_guard<std::mutex> lock{g_lifetime.mutex};
    auto live_it = g_lifetime.live.find(value);
    if (owner == nullptr && live_it != g_lifetime.live.end()) {
      owner = live_it->second.env;
    }
    record_release_locked(value, type_name);
  }
  maybe_dump_periodic_stats(owner);
}

void napi_lifetime_tracker__::record_scope_escape(napi_env env, bool succeeded) {
  {
    std::lock_guard<std::mutex> lock{g_lifetime.mutex};
    ++g_lifetime.scope_escape_calls;
    if (succeeded) {
      ++g_lifetime.scope_escape_succeeded;
    } else {
      ++g_lifetime.scope_escape_failed;
    }
  }
  maybe_dump_periodic_stats(env);
}

void napi_lifetime_tracker__::record_value(napi_env env,
                                           v8::Local<v8::Value> value,
                                           std::size_t parent_scope_depth) {
  {
    std::lock_guard<std::mutex> lock{g_lifetime.mutex};
    record_value_create_locked(env, value, parent_scope_depth);
  }
  maybe_dump_periodic_stats(env);
}

void napi_lifetime_tracker__::record_scope_values_release(napi_env env,
                                                          const void* scope) {
  {
    std::lock_guard<std::mutex> lock{g_lifetime.mutex};
    record_scope_values_release_locked(env, scope);
  }
  maybe_dump_periodic_stats(env);
}
#endif

void napi_lifetime_tracker__::dump(napi_env env, const char* reason) {
#ifdef NAPI_ENABLE_LIFETIME_TRACKER
  if (env == nullptr || !enabled()) return;
  dump_lifetime(env, reason, true);
#else
  (void)env;
  (void)reason;
#endif
}

}  // namespace v8impl::detail

extern "C" void napi_v8_lifetime_dump(napi_env env, const char* reason) {
  v8impl::detail::napi_lifetime_tracker__::dump(env, reason);
}
