#ifndef NAPI_LIFETIME_TRACKER_H_
#define NAPI_LIFETIME_TRACKER_H_

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace napi::lifetime {

struct type_stats {
  std::size_t created = 0;
  std::size_t released = 0;
  std::size_t active = 0;
  std::size_t peak = 0;
};

template <typename Env>
struct basic_snapshot {
  Env env = nullptr;
};

template <typename Snapshot>
struct tracked_type_stats : type_stats {
  std::unordered_map<const void*, Snapshot> live;
};

struct env_slot_scan {
  std::size_t value_slots_total = 0;
  std::size_t active_values = 0;
  std::size_t ref_slots_total = 0;
  std::size_t active_refs = 0;
  std::size_t scope_slots_total = 0;
  std::size_t active_scopes = 0;
  std::vector<std::pair<std::size_t, std::size_t>> active_values_by_scope_level;
};

struct counter_trend {
  bool available = false;
  std::size_t current = 0;
  long long speed = 0;
  long long acceleration = 0;
};

struct counter_history {
  std::unordered_map<std::string, std::array<std::size_t, 3>> values;
  std::unordered_map<std::string, std::size_t> sizes;
  std::unordered_map<std::string, std::size_t> next;
};

bool env_flag_enabled(const char* name);
bool env_flag_enabled_or(const char* name, bool fallback);
int64_t monotonic_milliseconds();

inline void record_create(type_stats& stats) {
  ++stats.created;
  ++stats.active;
  stats.peak = std::max(stats.peak, stats.active);
}

inline void record_release(type_stats& stats) {
  ++stats.released;
  if (stats.active > 0) {
    --stats.active;
  }
}

inline void add_type_stats(type_stats& total, const type_stats& stats) {
  total.created += stats.created;
  total.released += stats.released;
  total.active += stats.active;
  total.peak += stats.peak;
}

counter_trend observe_counter(counter_history& history,
                              const std::string& key,
                              std::size_t value);
void format_center_value(char* buffer,
                         std::size_t buffer_size,
                         const counter_trend& trend);
void format_delta_value(char* buffer,
                        std::size_t buffer_size,
                        const counter_trend& trend,
                        bool acceleration);
void format_trend_columns(const counter_trend& trend,
                          char* current,
                          std::size_t current_size,
                          char* speed,
                          std::size_t speed_size,
                          char* acceleration,
                          std::size_t acceleration_size);
void dump_metric_row(counter_history& history,
                     const char* metric,
                     std::size_t value,
                     const std::string& key);
void dump_counter_header(const char* title);
void dump_lifetime_header();
void dump_scope_level_table(counter_history& history,
                            const env_slot_scan& scan);
void dump_slots_table(counter_history& history,
                      const env_slot_scan& scan,
                      std::size_t tracked_values,
                      std::size_t tracked_refs,
                      std::size_t scope_escape_calls,
                      std::size_t scope_escape_succeeded,
                      std::size_t scope_escape_failed);
void dump_type_table_header();
void dump_type_row(counter_history& history,
                   const char* label,
                   const type_stats& stats);
void dump_summary_row(const env_slot_scan& scan);

template <typename Snapshot>
void add_basic_snapshot(tracked_type_stats<Snapshot>&, const Snapshot&) {}

template <typename Snapshot>
void remove_basic_snapshot(tracked_type_stats<Snapshot>&, const Snapshot&) {}

template <typename Stats, typename Snapshot, typename AddSnapshot>
void record_tracked_create(Stats& stats,
                           const void* handle,
                           Snapshot snapshot,
                           AddSnapshot add_snapshot) {
  auto existing = stats.live.find(handle);
  if (existing != stats.live.end()) {
    if (stats.active > 0) {
      --stats.active;
    }
    stats.live.erase(existing);
  }

  record_create(stats);
  add_snapshot(stats, snapshot);
  stats.live.emplace(handle, std::move(snapshot));
}

template <typename Env, typename Stats, typename RemoveSnapshot>
Env record_tracked_release(Stats& stats,
                           const void* handle,
                           RemoveSnapshot remove_snapshot) {
  auto existing = stats.live.find(handle);
  if (existing == stats.live.end()) {
    return nullptr;
  }

  Env env = existing->second.env;
  remove_snapshot(stats, existing->second);
  stats.live.erase(existing);
  record_release(stats);
  return env;
}

#ifdef NAPI_ENABLE_LIFETIME_TAG_STATS
template <typename TagCounters, typename TagName>
void dump_tag_table(counter_history& history,
                    const char* label,
                    const TagCounters& counters,
                    std::size_t tag_count,
                    TagName tag_name) {
  bool has_any = false;
  for (std::size_t i = 0; i < tag_count; ++i) {
    if (counters.slots[i] != 0) {
      has_any = true;
      break;
    }
  }
  if (!has_any) {
    return;
  }

  std::fprintf(stderr, "[napi-lifetime-tags] owner=%s\n", label);
  std::fprintf(stderr,
               "  %-16s %10s %10s %10s\n",
               "tag",
               "x[i-1]",
               "speed",
               "accel");
  for (std::size_t i = 0; i < tag_count; ++i) {
    std::size_t count = counters.slots[i];
    if (count == 0) {
      continue;
    }
    const char* name = tag_name(i);
    counter_trend trend =
        observe_counter(history, std::string("tag.") + label + "." + name, count);
    char current[32];
    char speed[32];
    char acceleration[32];
    format_center_value(current, sizeof(current), trend);
    format_delta_value(speed, sizeof(speed), trend, false);
    format_delta_value(acceleration, sizeof(acceleration), trend, true);
    std::fprintf(stderr,
                 "  %-16s %10s %10s %10s\n",
                 name,
                 current,
                 speed,
                 acceleration);
  }
}
#endif

#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
template <typename Tag>
struct string_symbol_entry {
  Tag tag{};
  std::string value;
  std::size_t count = 0;
};

struct object_type_entry {
  std::string prototype_name;
  std::size_t count = 0;
};

void append_hex_escape(std::string& out, unsigned char c);
std::string escaped_value_fragment(const char* value, std::size_t value_length);
void add_object_type_entry(std::vector<object_type_entry>& entries,
                           const std::string& prototype_name);
void remove_object_type_entry(std::vector<object_type_entry>& entries,
                              const std::string& prototype_name);

template <typename Tag>
void add_string_symbol_entry(std::vector<string_symbol_entry<Tag>>& entries,
                             Tag tag,
                             const std::string& value) {
  for (auto& entry : entries) {
    if (entry.tag == tag && entry.value == value) {
      ++entry.count;
      return;
    }
  }

  entries.push_back({tag, value, 1});
}

template <typename Tag>
void remove_string_symbol_entry(std::vector<string_symbol_entry<Tag>>& entries,
                                Tag tag,
                                const std::string& value) {
  for (auto it = entries.begin(); it != entries.end(); ++it) {
    if (it->tag == tag && it->value == value) {
      if (it->count > 1) {
        --it->count;
      } else {
        entries.erase(it);
      }
      return;
    }
  }
}

template <typename Entry, typename IsStringLike>
void dump_string_entries(counter_history& history,
                         const std::vector<Entry>& entries,
                         IsStringLike is_string_like,
                         const char* counter_owner) {
  std::size_t singular_count = 0;
  std::unordered_map<std::string, std::size_t> counts;
  for (const auto& entry : entries) {
    if (!is_string_like(entry.tag)) {
      continue;
    }
    counts[entry.value] += entry.count;
  }

  std::vector<std::pair<std::string, std::size_t>> sorted;
  sorted.reserve(counts.size());
  for (const auto& [value, count] : counts) {
    sorted.push_back({value, count});
  }

  std::sort(sorted.begin(), sorted.end(), [](const auto& left, const auto& right) {
    if (left.second != right.second) {
      return left.second > right.second;
    }
    return left.first < right.first;
  });

  if (sorted.empty()) {
    return;
  }

  std::fprintf(stderr, "[napi-lifetime-strings]\n");
  std::fprintf(stderr,
               "  %-36s %10s %10s %10s\n",
               "string",
               "x[i-1]",
               "speed",
               "accel");
  for (const auto& entry : sorted) {
    if (entry.second < 10) { // we go with at least 10, otherwise too many strings
      ++singular_count;
      continue;
    }

    counter_trend trend =
        observe_counter(history,
                        std::string("string.") + counter_owner + "." + entry.first,
                        entry.second);
    char current[32];
    char speed[32];
    char acceleration[32];
    format_trend_columns(trend,
                         current,
                         sizeof(current),
                         speed,
                         sizeof(speed),
                         acceleration,
                         sizeof(acceleration));
    std::fprintf(stderr,
                 "  %-36s %10s %10s %10s\n",
                 entry.first.c_str(),
                 current,
                 speed,
                 acceleration);
  }
  if (singular_count != 0) {
    dump_metric_row(history,
                    "count == 1",
                    singular_count,
                    std::string("string.") + counter_owner + ".count_eq_1");
  }
}

void dump_object_type_entries(counter_history& history,
                              const std::vector<object_type_entry>& value_entries,
                              const std::vector<object_type_entry>& ref_entries);
#endif

}  // namespace napi::lifetime

#ifdef NAPI_ENABLE_LIFETIME_TRACKER
#define NAPI_LIFETIME_DUMP(TRACKER, env, reason) TRACKER::dump(env, reason)
#else
#define NAPI_LIFETIME_DUMP(TRACKER, env, reason) ((void)0)
#endif

#endif  // NAPI_LIFETIME_TRACKER_H_
