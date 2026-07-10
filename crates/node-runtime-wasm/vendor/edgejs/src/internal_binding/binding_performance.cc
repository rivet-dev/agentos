#include "internal_binding/binding_initializers.h"
#include "internal_binding/binding_performance.h"

#include <chrono>
#include <cstdint>

#include "uv.h"
#include "edge_environment.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

enum PerformanceMilestone {
  kTimeOriginTimestamp = 0,
  kTimeOrigin = 1,
  kEnvironment = 2,
  kNodeStart = 3,
  kV8Start = 4,
  kLoopStart = 5,
  kLoopExit = 6,
  kBootstrapComplete = 7,
  kMilestoneInvalid = 8,
};

enum PerformanceEntryType {
  kEntryGc = 0,
  kEntryHttp = 1,
  kEntryHttp2 = 2,
  kEntryNet = 3,
  kEntryDns = 4,
  kEntryInvalid = 5,
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct PerformanceBindingState {
  explicit PerformanceBindingState(napi_env env_in) : env(env_in) {}
  ~PerformanceBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &milestones_ref);
    DeleteRefIfPresent(env, &observer_counts_ref);
    DeleteRefIfPresent(env, &observer_callback_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref milestones_ref = nullptr;
  napi_ref observer_counts_ref = nullptr;
  napi_ref observer_callback_ref = nullptr;
  uint64_t time_origin_ns = 0;
  double time_origin_timestamp_us = 0;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

PerformanceBindingState* GetPerformanceState(napi_env env) {
  return EdgeEnvironmentGetSlotData<PerformanceBindingState>(
      env, kEdgeEnvironmentSlotPerformanceBindingState);
}

PerformanceBindingState& EnsurePerformanceState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<PerformanceBindingState>(
      env, kEdgeEnvironmentSlotPerformanceBindingState);
}

double NowMicrosSinceEpoch() {
  using clock = std::chrono::system_clock;
  const auto now = clock::now().time_since_epoch();
  return static_cast<double>(
      std::chrono::duration_cast<std::chrono::microseconds>(now).count());
}

napi_value GetMilestonesArray(napi_env env) {
  auto* state = GetPerformanceState(env);
  if (state == nullptr || state->milestones_ref == nullptr) return nullptr;
  napi_value milestones = nullptr;
  if (napi_get_reference_value(env, state->milestones_ref, &milestones) != napi_ok ||
      milestones == nullptr) {
    return nullptr;
  }
  return milestones;
}

napi_value ReturnNumber(napi_env env, double value) {
  napi_value out = nullptr;
  if (napi_create_double(env, value, &out) != napi_ok || out == nullptr) return Undefined(env);
  return out;
}

napi_value ReturnBigIntZero(napi_env env) {
  napi_value out = nullptr;
  bool lossless = false;
  if (napi_create_bigint_uint64(env, 0, &out) != napi_ok || out == nullptr) {
    // Fallback for runtimes without bigint support.
    return ReturnNumber(env, 0);
  }
  (void)lossless;
  return out;
}

napi_value HistogramNoopCallback(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value HistogramCountCallback(napi_env env, napi_callback_info /*info*/) {
  return ReturnNumber(env, 0);
}

napi_value HistogramCountBigIntCallback(napi_env env, napi_callback_info /*info*/) {
  return ReturnBigIntZero(env);
}

napi_value HistogramMeanCallback(napi_env env, napi_callback_info /*info*/) {
  return ReturnNumber(env, 0);
}

napi_value HistogramPercentileCallback(napi_env env, napi_callback_info /*info*/) {
  return ReturnNumber(env, 0);
}

napi_value HistogramPercentileBigIntCallback(napi_env env, napi_callback_info /*info*/) {
  return ReturnBigIntZero(env);
}

napi_value HistogramPercentilesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  return argc >= 1 && argv[0] != nullptr ? argv[0] : Undefined(env);
}

bool DefineMethod(napi_env env, napi_value target, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  return napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
         fn != nullptr &&
         napi_set_named_property(env, target, name, fn) == napi_ok;
}

napi_value CreateHistogramHandle(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  if (!DefineMethod(env, out, "start", HistogramNoopCallback) ||
      !DefineMethod(env, out, "stop", HistogramNoopCallback) ||
      !DefineMethod(env, out, "reset", HistogramNoopCallback) ||
      !DefineMethod(env, out, "record", HistogramNoopCallback) ||
      !DefineMethod(env, out, "count", HistogramCountCallback) ||
      !DefineMethod(env, out, "countBigInt", HistogramCountBigIntCallback) ||
      !DefineMethod(env, out, "min", HistogramCountCallback) ||
      !DefineMethod(env, out, "minBigInt", HistogramCountBigIntCallback) ||
      !DefineMethod(env, out, "max", HistogramCountCallback) ||
      !DefineMethod(env, out, "maxBigInt", HistogramCountBigIntCallback) ||
      !DefineMethod(env, out, "mean", HistogramMeanCallback) ||
      !DefineMethod(env, out, "stddev", HistogramMeanCallback) ||
      !DefineMethod(env, out, "exceeds", HistogramCountCallback) ||
      !DefineMethod(env, out, "exceedsBigInt", HistogramCountBigIntCallback) ||
      !DefineMethod(env, out, "percentile", HistogramPercentileCallback) ||
      !DefineMethod(env, out, "percentileBigInt", HistogramPercentileBigIntCallback) ||
      !DefineMethod(env, out, "percentiles", HistogramPercentilesCallback) ||
      !DefineMethod(env, out, "percentilesBigInt", HistogramPercentilesCallback)) {
    return nullptr;
  }

  return out;
}

napi_value CreateELDHistogramCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = CreateHistogramHandle(env);
  return out != nullptr ? out : Undefined(env);
}

napi_value HistogramConstructorCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = CreateHistogramHandle(env);
  return out != nullptr ? out : Undefined(env);
}

napi_value SetupObserversCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return Undefined(env);
  }
  auto* state = GetPerformanceState(env);
  if (state == nullptr) return Undefined(env);
  DeleteRefIfPresent(env, &state->observer_callback_ref);
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      (void)napi_create_reference(env, argv[0], 1, &state->observer_callback_ref);
    }
  }
  return Undefined(env);
}

napi_value InstallGarbageCollectionTrackingCallback(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value RemoveGarbageCollectionTrackingCallback(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value NotifyCallback(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return Undefined(env);
  }
  auto* state = GetPerformanceState(env);
  if (state == nullptr || state->observer_callback_ref == nullptr) {
    return Undefined(env);
  }
  napi_value callback = nullptr;
  if (napi_get_reference_value(env, state->observer_callback_ref, &callback) != napi_ok ||
      callback == nullptr) {
    return Undefined(env);
  }
  napi_value global = GetGlobal(env);
  napi_value ignored = nullptr;
  (void)napi_call_function(env, global, callback, argc, argv, &ignored);
  return Undefined(env);
}

napi_value LoopIdleTimeCallback(napi_env env, napi_callback_info /*info*/) {
  uv_loop_t* loop = nullptr;
  if (napi_get_uv_event_loop(env, &loop) != napi_ok || loop == nullptr) {
    return ReturnNumber(env, 0);
  }
  return ReturnNumber(env, static_cast<double>(uv_metrics_idle_time(loop)) / 1e6);
}

napi_value UvMetricsInfoCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 3, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  napi_value zero = nullptr;
  if (napi_create_double(env, 0, &zero) != napi_ok || zero == nullptr) return out;
  napi_set_element(env, out, 0, zero);
  napi_set_element(env, out, 1, zero);
  napi_set_element(env, out, 2, zero);
  return out;
}

napi_value PerformanceNowCallback(napi_env env, napi_callback_info /*info*/) {
  auto* state = GetPerformanceState(env);
  if (state == nullptr) return ReturnNumber(env, 0);
  const uint64_t now_ns = uv_hrtime();
  double now_ms = 0;
  if (now_ns >= state->time_origin_ns) {
    now_ms = static_cast<double>(now_ns - state->time_origin_ns) / 1e6;
  }
  return ReturnNumber(env, now_ms);
}

napi_value MarkBootstrapCompleteCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value milestones = GetMilestonesArray(env);
  if (milestones == nullptr) return Undefined(env);

  napi_value value = nullptr;
  if (napi_create_double(env, static_cast<double>(uv_hrtime()), &value) == napi_ok &&
      value != nullptr) {
    napi_set_element(env, milestones, kBootstrapComplete, value);
  }
  return Undefined(env);
}

napi_value GetCachedPerformance(napi_env env) {
  auto* state = GetPerformanceState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

void SetMilestoneValue(napi_env env, napi_value milestones, uint32_t index, double value) {
  napi_value n = nullptr;
  if (napi_create_double(env, value, &n) == napi_ok && n != nullptr) {
    napi_set_element(env, milestones, index, n);
  }
}

}  // namespace

bool PerformanceHasObserver(napi_env env, uint32_t type_index) {
  auto* state = GetPerformanceState(env);
  if (state == nullptr || state->observer_counts_ref == nullptr) return false;
  napi_value observer_counts = nullptr;
  if (napi_get_reference_value(env, state->observer_counts_ref, &observer_counts) != napi_ok ||
      observer_counts == nullptr) {
    return false;
  }
  napi_value count_value = nullptr;
  if (napi_get_element(env, observer_counts, type_index, &count_value) != napi_ok || count_value == nullptr) {
    return false;
  }
  double count = 0;
  if (napi_get_value_double(env, count_value, &count) != napi_ok) return false;
  return count > 0;
}

void PerformanceEmitEntry(napi_env env,
                          const char* name,
                          const char* entry_type,
                          double start_time,
                          double duration,
                          napi_value details) {
  auto* state = GetPerformanceState(env);
  if (state == nullptr || state->observer_callback_ref == nullptr) return;
  napi_value callback = nullptr;
  if (napi_get_reference_value(env, state->observer_callback_ref, &callback) != napi_ok ||
      callback == nullptr) {
    return;
  }

  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, details != nullptr ? details : Undefined(env)};
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &argv[0]) != napi_ok ||
      napi_create_string_utf8(env, entry_type, NAPI_AUTO_LENGTH, &argv[1]) != napi_ok ||
      napi_create_double(env, start_time, &argv[2]) != napi_ok ||
      napi_create_double(env, duration, &argv[3]) != napi_ok) {
    return;
  }

  napi_value global = GetGlobal(env);
  napi_value ignored = nullptr;
  (void)napi_call_function(env, global, callback, 5, argv, &ignored);
}

napi_value InitPerformance(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedPerformance(env);
  if (cached != nullptr) return cached;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  auto& state = EnsurePerformanceState(env);
  state.time_origin_ns = uv_hrtime();
  state.time_origin_timestamp_us = NowMicrosSinceEpoch();

  napi_value milestones = nullptr;
  if (napi_create_array_with_length(env, kMilestoneInvalid, &milestones) != napi_ok ||
      milestones == nullptr) {
    return undefined;
  }
  for (uint32_t i = 0; i < kMilestoneInvalid; i++) {
    SetMilestoneValue(env, milestones, i, -1);
  }
  const double now_ns = static_cast<double>(state.time_origin_ns);
  SetMilestoneValue(env, milestones, kTimeOriginTimestamp, state.time_origin_timestamp_us);
  SetMilestoneValue(env, milestones, kTimeOrigin, now_ns);
  SetMilestoneValue(env, milestones, kEnvironment, now_ns);
  SetMilestoneValue(env, milestones, kNodeStart, now_ns);
  SetMilestoneValue(env, milestones, kV8Start, now_ns);
  SetMilestoneValue(env, milestones, kLoopStart, now_ns);

  DeleteRefIfPresent(env, &state.milestones_ref);
  napi_create_reference(env, milestones, 1, &state.milestones_ref);
  napi_set_named_property(env, out, "milestones", milestones);

  napi_value observer_counts = nullptr;
  if (napi_create_array_with_length(env, kEntryInvalid, &observer_counts) == napi_ok &&
      observer_counts != nullptr) {
    for (uint32_t i = 0; i < kEntryInvalid; i++) {
      SetMilestoneValue(env, observer_counts, i, 0);
    }
    DeleteRefIfPresent(env, &state.observer_counts_ref);
    napi_create_reference(env, observer_counts, 1, &state.observer_counts_ref);
    napi_set_named_property(env, out, "observerCounts", observer_counts);
  }

  napi_value constants = nullptr;
  if (napi_create_object(env, &constants) == napi_ok && constants != nullptr) {
    SetInt32(env, constants, "NODE_PERFORMANCE_ENTRY_TYPE_GC", kEntryGc);
    SetInt32(env, constants, "NODE_PERFORMANCE_ENTRY_TYPE_HTTP", kEntryHttp);
    SetInt32(env, constants, "NODE_PERFORMANCE_ENTRY_TYPE_HTTP2", kEntryHttp2);
    SetInt32(env, constants, "NODE_PERFORMANCE_ENTRY_TYPE_NET", kEntryNet);
    SetInt32(env, constants, "NODE_PERFORMANCE_ENTRY_TYPE_DNS", kEntryDns);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_TIME_ORIGIN_TIMESTAMP", kTimeOriginTimestamp);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_TIME_ORIGIN", kTimeOrigin);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_ENVIRONMENT", kEnvironment);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_NODE_START", kNodeStart);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_V8_START", kV8Start);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_LOOP_START", kLoopStart);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_LOOP_EXIT", kLoopExit);
    SetInt32(env, constants, "NODE_PERFORMANCE_MILESTONE_BOOTSTRAP_COMPLETE", kBootstrapComplete);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_MAJOR", 1);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_MINOR", 2);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_INCREMENTAL", 4);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_WEAKCB", 8);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_NO", 0);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_CONSTRUCT_RETAINED", 2);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_FORCED", 4);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_SYNCHRONOUS_PHANTOM_PROCESSING", 8);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_ALL_AVAILABLE_GARBAGE", 16);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_ALL_EXTERNAL_MEMORY", 32);
    SetInt32(env, constants, "NODE_PERFORMANCE_GC_FLAGS_SCHEDULE_IDLE", 64);
    napi_set_named_property(env, out, "constants", constants);
  }

  DefineMethod(env, out, "setupObservers", SetupObserversCallback);
  DefineMethod(env, out, "installGarbageCollectionTracking", InstallGarbageCollectionTrackingCallback);
  DefineMethod(env, out, "removeGarbageCollectionTracking", RemoveGarbageCollectionTrackingCallback);
  DefineMethod(env, out, "notify", NotifyCallback);
  DefineMethod(env, out, "loopIdleTime", LoopIdleTimeCallback);
  DefineMethod(env, out, "createELDHistogram", CreateELDHistogramCallback);
  DefineMethod(env, out, "markBootstrapComplete", MarkBootstrapCompleteCallback);
  DefineMethod(env, out, "uvMetricsInfo", UvMetricsInfoCallback);
  DefineMethod(env, out, "now", PerformanceNowCallback);

  napi_value histogram_ctor = nullptr;
  if (napi_create_function(env,
                           "Histogram",
                           NAPI_AUTO_LENGTH,
                           HistogramConstructorCallback,
                           nullptr,
                           &histogram_ctor) == napi_ok &&
      histogram_ctor != nullptr) {
    napi_set_named_property(env, out, "Histogram", histogram_ctor);
  }

  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, out, 1, &state.binding_ref);
  return out;
}

}  // namespace internal_binding
