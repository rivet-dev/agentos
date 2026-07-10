#include "napi_lifetime_tracker.h"

#include <chrono>
#include <cstdlib>

namespace napi::lifetime {

bool env_flag_enabled(const char* name) {
  const char* value = std::getenv(name);
  return value != nullptr && value[0] != '\0' && value[0] != '0';
}

bool env_flag_enabled_or(const char* name, bool fallback) {
  const char* value = std::getenv(name);
  if (value != nullptr && value[0] != '\0') {
    return value[0] != '0';
  }
  return fallback;
}

int64_t monotonic_milliseconds() {
  using clock = std::chrono::steady_clock;
  return std::chrono::duration_cast<std::chrono::milliseconds>(
             clock::now().time_since_epoch())
      .count();
}

counter_trend observe_counter(counter_history& counters,
                              const std::string& key,
                              std::size_t value) {
  counter_trend trend;
  std::size_t& history_size = counters.sizes[key];
  std::size_t& next = counters.next[key];
  auto& history = counters.values[key];

  history[next] = value;
  next = (next + 1) % history.size();
  if (history_size < history.size()) {
    ++history_size;
  }

  if (history_size == history.size()) {
    std::size_t i_minus_2_index = next;
    std::size_t i_minus_1_index = (next + 1) % history.size();
    std::size_t i_index = (next + 2) % history.size();
    long long x_i_minus_2 = static_cast<long long>(history[i_minus_2_index]);
    long long x_i_minus_1 = static_cast<long long>(history[i_minus_1_index]);
    long long x_i = static_cast<long long>(history[i_index]);
    trend.available = true;
    trend.current = history[i_minus_1_index];
    trend.speed = x_i - x_i_minus_2;
    trend.acceleration = x_i - 2 * x_i_minus_1 + x_i_minus_2;
  }
  return trend;
}

void format_center_value(char* buffer,
                         std::size_t buffer_size,
                         const counter_trend& trend) {
  if (!trend.available) {
    std::snprintf(buffer, buffer_size, "%s", "-");
    return;
  }
  std::snprintf(buffer, buffer_size, "%zu", trend.current);
}

void format_delta_value(char* buffer,
                        std::size_t buffer_size,
                        const counter_trend& trend,
                        bool acceleration) {
  if (!trend.available) {
    std::snprintf(buffer, buffer_size, "%s", "-");
    return;
  }
  std::snprintf(buffer,
                buffer_size,
                "%lld",
                acceleration ? trend.acceleration : trend.speed);
}

void format_trend_columns(const counter_trend& trend,
                          char* current,
                          std::size_t current_size,
                          char* speed,
                          std::size_t speed_size,
                          char* acceleration,
                          std::size_t acceleration_size) {
  format_center_value(current, current_size, trend);
  format_delta_value(speed, speed_size, trend, false);
  format_delta_value(acceleration, acceleration_size, trend, true);
}

void dump_metric_row(counter_history& history,
                     const char* metric,
                     std::size_t value,
                     const std::string& key) {
  counter_trend trend = observe_counter(history, key, value);
  char current[32];
  char speed[32];
  char acceleration[32];
  format_center_value(current, sizeof(current), trend);
  format_delta_value(speed, sizeof(speed), trend, false);
  format_delta_value(acceleration, sizeof(acceleration), trend, true);
  std::fprintf(stderr,
               "  %-36s %10s %10s %10s\n",
               metric,
               current,
               speed,
               acceleration);
}

void dump_counter_header(const char* title) {
  std::fprintf(stderr, "%s\n", title);
  std::fprintf(stderr,
               "  %-36s %10s %10s %10s\n",
               "metric",
               "x[i-1]",
               "speed",
               "accel");
}

void dump_lifetime_header() {
  std::fprintf(stderr, "NAPI LIFETIME TRACKER\n=====================\n");
}

void dump_scope_level_table(counter_history& history,
                            const env_slot_scan& scan) {
  if (scan.active_values_by_scope_level.empty()) {
    return;
  }

  std::fprintf(stderr, "[napi-lifetime-scopes]\n");
  std::fprintf(stderr,
               "  %-16s %10s %10s %10s\n",
               "level",
               "x[i-1]",
               "speed",
               "accel");
  for (const auto& [level, count] : scan.active_values_by_scope_level) {
    counter_trend trend = observe_counter(
        history, std::string("scope.level.") + std::to_string(level), count);
    char current[32];
    char speed[32];
    char acceleration[32];
    format_center_value(current, sizeof(current), trend);
    format_delta_value(speed, sizeof(speed), trend, false);
    format_delta_value(acceleration, sizeof(acceleration), trend, true);
    std::fprintf(stderr,
                 "  %-16zu %10s %10s %10s\n",
                 level,
                 current,
                 speed,
                 acceleration);
  }
}

void dump_slots_table(counter_history& history,
                      const env_slot_scan& scan,
                      std::size_t tracked_values,
                      std::size_t tracked_refs,
                      std::size_t scope_escape_calls,
                      std::size_t scope_escape_succeeded,
                      std::size_t scope_escape_failed) {
  dump_counter_header("[napi-lifetime-slots]");
  dump_metric_row(history,
                  "napi_value.slots_total",
                  scan.value_slots_total,
                  "slots.value.total");
  dump_metric_row(history,
                  "napi_value.active",
                  scan.active_values,
                  "slots.value.active");
  dump_metric_row(history,
                  "napi_value.tracked_active",
                  tracked_values,
                  "slots.value.tracked_active");
  dump_metric_row(history,
                  "napi_ref.slots_total",
                  scan.ref_slots_total,
                  "slots.ref.total");
  dump_metric_row(history,
                  "napi_ref.active",
                  scan.active_refs,
                  "slots.ref.active");
  dump_metric_row(history,
                  "napi_ref.tracked_active",
                  tracked_refs,
                  "slots.ref.tracked_active");
  dump_metric_row(history,
                  "napi_scope.slots_total",
                  scan.scope_slots_total,
                  "slots.scope.total");
  dump_metric_row(history,
                  "napi_scope.active",
                  scan.active_scopes,
                  "slots.scope.active");
  dump_metric_row(history,
                  "napi_scope.escape_value.calls",
                  scope_escape_calls,
                  "scope.escape.calls");
  dump_metric_row(history,
                  "napi_scope.escape_value.succeeded",
                  scope_escape_succeeded,
                  "scope.escape.succeeded");
  dump_metric_row(history,
                  "napi_scope.escape_value.failed",
                  scope_escape_failed,
                  "scope.escape.failed");
}

void dump_type_table_header() {
  std::fprintf(stderr, "[napi-lifetime-types]\n");
  std::fprintf(stderr,
               "  %-33s %10s %10s %10s %10s %10s %10s\n",
               "type",
               "created",
               "released",
               "x[i-1]",
               "peak",
               "speed",
               "accel");
}

void dump_type_row(counter_history& history,
                   const char* label,
                   const type_stats& stats) {
  counter_trend trend = observe_counter(
      history, std::string("type.") + label + ".active", stats.active);
  char current[32];
  char speed[32];
  char acceleration[32];
  format_center_value(current, sizeof(current), trend);
  format_delta_value(speed, sizeof(speed), trend, false);
  format_delta_value(acceleration, sizeof(acceleration), trend, true);
  std::fprintf(stderr,
               "  %-33s %10zu %10zu %10s %10zu %10s %10s\n",
               label,
               stats.created,
               stats.released,
               current,
               stats.peak,
               speed,
               acceleration);
}

void dump_summary_row(const env_slot_scan& scan) {
  std::fprintf(stderr,
               "[napi-lifetime-stats] napi_value slots_total=%zu active=%zu "
               "napi_ref slots_total=%zu active=%zu "
               "napi_scope slots_total=%zu active=%zu\n",
               scan.value_slots_total,
               scan.active_values,
               scan.ref_slots_total,
               scan.active_refs,
               scan.scope_slots_total,
               scan.active_scopes);
}

#ifdef NAPI_ENABLE_LIFETIME_STRING_SYMBOL_DUMP
void append_hex_escape(std::string& out, unsigned char c) {
  constexpr char hex[] = "0123456789abcdef";
  out.push_back('\\');
  out.push_back('x');
  out.push_back(hex[(c >> 4) & 0x0f]);
  out.push_back(hex[c & 0x0f]);
}

std::string escaped_value_fragment(const char* value, std::size_t value_length) {
  constexpr std::size_t k_value_dump_max_bytes = 240;
  std::size_t limit =
      value_length < k_value_dump_max_bytes ? value_length : k_value_dump_max_bytes;
  std::string out;
  out.reserve(limit + 3);
  for (std::size_t i = 0; i < limit; ++i) {
    unsigned char c = static_cast<unsigned char>(value[i]);
    switch (c) {
      case '\\':
        out += "\\\\";
        break;
      case '"':
        out += "\\\"";
        break;
      case '\n':
        out += "\\n";
        break;
      case '\r':
        out += "\\r";
        break;
      case '\t':
        out += "\\t";
        break;
      default:
        if (c >= 0x20 && c <= 0x7e) {
          out.push_back(static_cast<char>(c));
        } else {
          append_hex_escape(out, c);
        }
        break;
    }
  }
  if (value_length > limit) {
    out += "...";
  }
  return out;
}

void add_object_type_entry(std::vector<object_type_entry>& entries,
                           const std::string& prototype_name) {
  for (auto& entry : entries) {
    if (entry.prototype_name == prototype_name) {
      ++entry.count;
      return;
    }
  }
  entries.push_back({prototype_name, 1});
}

void remove_object_type_entry(std::vector<object_type_entry>& entries,
                              const std::string& prototype_name) {
  for (auto it = entries.begin(); it != entries.end(); ++it) {
    if (it->prototype_name == prototype_name) {
      if (it->count > 1) {
        --it->count;
      } else {
        entries.erase(it);
      }
      return;
    }
  }
}

std::unordered_map<std::string, std::size_t> object_type_counts(
    const std::vector<object_type_entry>& entries) {
  std::unordered_map<std::string, std::size_t> counts;
  for (const auto& entry : entries) {
    counts[entry.prototype_name] += entry.count;
  }
  return counts;
}

struct named_dual_count {
  std::string name;
  std::size_t values = 0;
  std::size_t refs = 0;
};

void dump_object_type_entries(counter_history& history,
                              const std::vector<object_type_entry>& value_entries,
                              const std::vector<object_type_entry>& ref_entries) {
  std::size_t singular_value_count = 0;
  std::size_t singular_ref_count = 0;
  std::unordered_map<std::string, std::size_t> values =
      object_type_counts(value_entries);
  std::unordered_map<std::string, std::size_t> refs =
      object_type_counts(ref_entries);
  std::unordered_map<std::string, named_dual_count> combined;

  for (const auto& [name, count] : values) {
    combined[name].name = name;
    combined[name].values = count;
  }
  for (const auto& [name, count] : refs) {
    combined[name].name = name;
    combined[name].refs = count;
  }

  std::vector<named_dual_count> sorted;
  sorted.reserve(combined.size());
  for (const auto& [_, count] : combined) {
    sorted.push_back(count);
  }

  std::sort(sorted.begin(), sorted.end(), [](const auto& left, const auto& right) {
    std::size_t left_total = left.values + left.refs;
    std::size_t right_total = right.values + right.refs;
    if (left_total != right_total) {
      return left_total > right_total;
    }
    if (left.values != right.values) {
      return left.values > right.values;
    }
    if (left.refs != right.refs) {
      return left.refs > right.refs;
    }
    return left.name < right.name;
  });

  if (sorted.empty()) {
    return;
  }

  std::fprintf(stderr, "[napi-lifetime-objects]\n");
  std::fprintf(stderr,
               "  %-28s  %10s %10s %10s  %10s %10s %10s\n",
               "type",
               "values:x",
               "speed",
               "accel",
               "refs:x",
               "speed",
               "accel");
  for (const auto& entry : sorted) {
    bool has_printable_values = entry.values >= 2;
    bool has_printable_refs = entry.refs >= 2;
    if (!has_printable_values && entry.values != 0) {
      ++singular_value_count;
    }
    if (!has_printable_refs && entry.refs != 0) {
      ++singular_ref_count;
    }
    if (!has_printable_values && !has_printable_refs) {
      continue;
    }

    counter_trend value_trend =
        observe_counter(history, std::string("object.napi_value.") + entry.name, entry.values);
    counter_trend ref_trend =
        observe_counter(history, std::string("object.napi_ref.") + entry.name, entry.refs);
    char value_current[32];
    char value_speed[32];
    char value_acceleration[32];
    char ref_current[32];
    char ref_speed[32];
    char ref_acceleration[32];
    if (has_printable_values) {
      format_trend_columns(value_trend,
                           value_current,
                           sizeof(value_current),
                           value_speed,
                           sizeof(value_speed),
                           value_acceleration,
                           sizeof(value_acceleration));
    } else {
      std::snprintf(value_current, sizeof(value_current), "%s", "-");
      std::snprintf(value_speed, sizeof(value_speed), "%s", "-");
      std::snprintf(value_acceleration, sizeof(value_acceleration), "%s", "-");
    }

    if (has_printable_refs) {
      format_trend_columns(ref_trend,
                           ref_current,
                           sizeof(ref_current),
                           ref_speed,
                           sizeof(ref_speed),
                           ref_acceleration,
                           sizeof(ref_acceleration));
    } else {
      std::snprintf(ref_current, sizeof(ref_current), "%s", "-");
      std::snprintf(ref_speed, sizeof(ref_speed), "%s", "-");
      std::snprintf(ref_acceleration, sizeof(ref_acceleration), "%s", "-");
    }

    std::fprintf(stderr,
                 "  %-28s  %10s %10s %10s  %10s %10s %10s\n",
                 entry.name.c_str(),
                 value_current,
                 value_speed,
                 value_acceleration,
                 ref_current,
                 ref_speed,
                 ref_acceleration);
  }

  if (singular_value_count != 0 || singular_ref_count != 0) {
    counter_trend value_trend =
        observe_counter(history, "object.napi_value.count_eq_1", singular_value_count);
    counter_trend ref_trend =
        observe_counter(history, "object.napi_ref.count_eq_1", singular_ref_count);
    char value_current[32];
    char value_speed[32];
    char value_acceleration[32];
    char ref_current[32];
    char ref_speed[32];
    char ref_acceleration[32];
    format_trend_columns(value_trend,
                         value_current,
                         sizeof(value_current),
                         value_speed,
                         sizeof(value_speed),
                         value_acceleration,
                         sizeof(value_acceleration));
    format_trend_columns(ref_trend,
                         ref_current,
                         sizeof(ref_current),
                         ref_speed,
                         sizeof(ref_speed),
                         ref_acceleration,
                         sizeof(ref_acceleration));
    std::fprintf(stderr,
                 "  %-28s  %10s %10s %10s  %10s %10s %10s\n",
                 "count == 1",
                 value_current,
                 value_speed,
                 value_acceleration,
                 ref_current,
                 ref_speed,
                 ref_acceleration);
  }
}
#endif

}  // namespace napi::lifetime
