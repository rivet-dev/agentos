#include "internal_binding/binding_initializers.h"

#include <cstring>
#include <string>

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"

namespace internal_binding {

namespace {

constexpr size_t kHeapStatisticsPropertiesCount = 14;
constexpr size_t kHeapSpaceStatisticsPropertiesCount = 4;
constexpr size_t kHeapCodeStatisticsPropertiesCount = 4;

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct V8BindingState {
  explicit V8BindingState(napi_env env_in) : env(env_in) {}
  ~V8BindingState() {
    DeleteRefIfPresent(env, &heap_statistics_buffer_ref);
    DeleteRefIfPresent(env, &heap_space_statistics_buffer_ref);
    DeleteRefIfPresent(env, &heap_code_statistics_buffer_ref);
  }

  napi_env env = nullptr;
  napi_ref heap_statistics_buffer_ref = nullptr;
  napi_ref heap_space_statistics_buffer_ref = nullptr;
  napi_ref heap_code_statistics_buffer_ref = nullptr;
};

napi_value MakeUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void SetNamedInt(napi_env env, napi_value obj, const char* key, int32_t value) {
  napi_value v = nullptr;
  if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, key, v);
  }
}

void SetNamedMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

void StoreBufferRef(napi_env env, napi_ref* slot, napi_value value) {
  if (slot == nullptr) return;
  DeleteRefIfPresent(env, slot);
  if (value != nullptr) {
    napi_create_reference(env, value, 1, slot);
  }
}

V8BindingState* GetV8BindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<V8BindingState>(env, kEdgeEnvironmentSlotV8BindingState);
}

V8BindingState& EnsureV8BindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<V8BindingState>(
      env, kEdgeEnvironmentSlotV8BindingState);
}

double* GetFloat64ArrayData(napi_env env, napi_ref ref, size_t minimum_length) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value typed_array = nullptr;
  if (napi_get_reference_value(env, ref, &typed_array) != napi_ok || typed_array == nullptr) {
    return nullptr;
  }
  bool is_typed_array = false;
  if (napi_is_typedarray(env, typed_array, &is_typed_array) != napi_ok || !is_typed_array) {
    return nullptr;
  }

  napi_typedarray_type type = napi_int8_array;
  size_t length = 0;
  void* data = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(
          env, typed_array, &type, &length, &data, &arraybuffer, &byte_offset) != napi_ok ||
      type != napi_float64_array ||
      length < minimum_length ||
      data == nullptr) {
    return nullptr;
  }

  return static_cast<double*>(data);
}

napi_value CreateFloat64Array(napi_env env, size_t length) {
  napi_value ab = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, length * sizeof(double), &data, &ab) != napi_ok || ab == nullptr) {
    return nullptr;
  }
  if (data != nullptr) {
    std::memset(data, 0, length * sizeof(double));
  }
  napi_value arr = nullptr;
  if (napi_create_typedarray(env, napi_float64_array, length, ab, 0, &arr) != napi_ok) return nullptr;
  return arr;
}

napi_value V8CachedDataVersionTag(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : MakeUndefined(env);
}

napi_value V8SetFlagsFromString(napi_env env, napi_callback_info /*info*/) {
  return MakeUndefined(env);
}

napi_value V8StartCpuProfile(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : MakeUndefined(env);
}

napi_value V8StopCpuProfile(napi_env env, napi_callback_info /*info*/) {
  return MakeUndefined(env);
}

napi_value V8IsStringOneByteRepresentation(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool is_one_byte = true;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, argv[0], &t) != napi_ok || t != napi_string) {
      is_one_byte = false;
    } else {
      size_t len = 0;
      if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &len) == napi_ok) {
        std::string bytes(len + 1, '\0');
        size_t written = 0;
        if (napi_get_value_string_utf8(env, argv[0], bytes.data(), bytes.size(), &written) == napi_ok) {
          for (size_t i = 0; i < written; ++i) {
            if (static_cast<unsigned char>(bytes[i]) > 0x7F) {
              is_one_byte = false;
              break;
            }
          }
        } else {
          is_one_byte = false;
        }
      } else {
        is_one_byte = false;
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, is_one_byte, &out);
  return out != nullptr ? out : MakeUndefined(env);
}

napi_value V8UpdateHeapStatisticsBuffer(napi_env env, napi_callback_info /*info*/) {
  V8BindingState* state = GetV8BindingState(env);
  if (state == nullptr) return MakeUndefined(env);

  double* buffer =
      GetFloat64ArrayData(env, state->heap_statistics_buffer_ref, kHeapStatisticsPropertiesCount);
  if (buffer == nullptr) return MakeUndefined(env);

  unofficial_napi_heap_statistics stats{};
  if (unofficial_napi_get_heap_statistics(env, &stats) != napi_ok) {
    return MakeUndefined(env);
  }

  buffer[0] = static_cast<double>(stats.total_heap_size);
  buffer[1] = static_cast<double>(stats.total_heap_size_executable);
  buffer[2] = static_cast<double>(stats.total_physical_size);
  buffer[3] = static_cast<double>(stats.total_available_size);
  buffer[4] = static_cast<double>(stats.used_heap_size);
  buffer[5] = static_cast<double>(stats.heap_size_limit);
  buffer[6] = static_cast<double>(stats.does_zap_garbage);
  buffer[7] = static_cast<double>(stats.malloced_memory);
  buffer[8] = static_cast<double>(stats.peak_malloced_memory);
  buffer[9] = static_cast<double>(stats.number_of_native_contexts);
  buffer[10] = static_cast<double>(stats.number_of_detached_contexts);
  buffer[11] = static_cast<double>(stats.total_global_handles_size);
  buffer[12] = static_cast<double>(stats.used_global_handles_size);
  buffer[13] = static_cast<double>(stats.external_memory);
  return MakeUndefined(env);
}

napi_value V8UpdateHeapSpaceStatisticsBuffer(napi_env env, napi_callback_info info) {
  V8BindingState* state = GetV8BindingState(env);
  if (state == nullptr) return MakeUndefined(env);

  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  uint32_t space_index = 0;
  if (argc < 1 || argv[0] == nullptr || napi_get_value_uint32(env, argv[0], &space_index) != napi_ok) {
    return MakeUndefined(env);
  }

  double* buffer = GetFloat64ArrayData(
      env, state->heap_space_statistics_buffer_ref, kHeapSpaceStatisticsPropertiesCount);
  if (buffer == nullptr) return MakeUndefined(env);

  unofficial_napi_heap_space_statistics stats{};
  if (unofficial_napi_get_heap_space_statistics(env, space_index, &stats) != napi_ok) {
    return MakeUndefined(env);
  }

  buffer[0] = static_cast<double>(stats.space_size);
  buffer[1] = static_cast<double>(stats.space_used_size);
  buffer[2] = static_cast<double>(stats.space_available_size);
  buffer[3] = static_cast<double>(stats.physical_space_size);
  return MakeUndefined(env);
}

napi_value V8UpdateHeapCodeStatisticsBuffer(napi_env env, napi_callback_info /*info*/) {
  V8BindingState* state = GetV8BindingState(env);
  if (state == nullptr) return MakeUndefined(env);

  double* buffer =
      GetFloat64ArrayData(env, state->heap_code_statistics_buffer_ref, kHeapCodeStatisticsPropertiesCount);
  if (buffer == nullptr) return MakeUndefined(env);

  unofficial_napi_heap_code_statistics stats{};
  if (unofficial_napi_get_heap_code_statistics(env, &stats) != napi_ok) {
    return MakeUndefined(env);
  }

  buffer[0] = static_cast<double>(stats.code_and_metadata_size);
  buffer[1] = static_cast<double>(stats.bytecode_and_metadata_size);
  buffer[2] = static_cast<double>(stats.external_script_source_size);
  buffer[3] = static_cast<double>(stats.cpu_profiler_metadata_size);
  return MakeUndefined(env);
}

napi_value V8Noop(napi_env env, napi_callback_info /*info*/) {
  return MakeUndefined(env);
}

napi_value V8GetCppHeapStatistics(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_object(env, &out);
  return out != nullptr ? out : MakeUndefined(env);
}

napi_value V8GetHashSeed(napi_env env, napi_callback_info /*info*/) {
  uint64_t hash_seed = 0;
  if (unofficial_napi_get_hash_seed(env, &hash_seed) != napi_ok) {
    return MakeUndefined(env);
  }
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(hash_seed), &out);
  return out != nullptr ? out : MakeUndefined(env);
}

}  // namespace

napi_value InitV8(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  SetNamedMethod(env, out, "cachedDataVersionTag", V8CachedDataVersionTag);
  SetNamedMethod(env, out, "setFlagsFromString", V8SetFlagsFromString);
  SetNamedMethod(env, out, "startCpuProfile", V8StartCpuProfile);
  SetNamedMethod(env, out, "stopCpuProfile", V8StopCpuProfile);
  SetNamedMethod(env, out, "isStringOneByteRepresentation", V8IsStringOneByteRepresentation);
  SetNamedMethod(env, out, "getHashSeed", V8GetHashSeed);
  SetNamedMethod(env, out, "updateHeapStatisticsBuffer", V8UpdateHeapStatisticsBuffer);
  SetNamedMethod(env, out, "updateHeapSpaceStatisticsBuffer", V8UpdateHeapSpaceStatisticsBuffer);
  SetNamedMethod(env, out, "updateHeapCodeStatisticsBuffer", V8UpdateHeapCodeStatisticsBuffer);
  SetNamedMethod(env, out, "setHeapSnapshotNearHeapLimit", V8Noop);
  SetNamedMethod(env, out, "getCppHeapStatistics", V8GetCppHeapStatistics);

  SetNamedInt(env, out, "kTotalHeapSizeIndex", 0);
  SetNamedInt(env, out, "kTotalHeapSizeExecutableIndex", 1);
  SetNamedInt(env, out, "kTotalPhysicalSizeIndex", 2);
  SetNamedInt(env, out, "kTotalAvailableSize", 3);
  SetNamedInt(env, out, "kUsedHeapSizeIndex", 4);
  SetNamedInt(env, out, "kHeapSizeLimitIndex", 5);
  SetNamedInt(env, out, "kDoesZapGarbageIndex", 6);
  SetNamedInt(env, out, "kMallocedMemoryIndex", 7);
  SetNamedInt(env, out, "kPeakMallocedMemoryIndex", 8);
  SetNamedInt(env, out, "kNumberOfNativeContextsIndex", 9);
  SetNamedInt(env, out, "kNumberOfDetachedContextsIndex", 10);
  SetNamedInt(env, out, "kTotalGlobalHandlesSizeIndex", 11);
  SetNamedInt(env, out, "kUsedGlobalHandlesSizeIndex", 12);
  SetNamedInt(env, out, "kExternalMemoryIndex", 13);

  SetNamedInt(env, out, "kSpaceSizeIndex", 0);
  SetNamedInt(env, out, "kSpaceUsedSizeIndex", 1);
  SetNamedInt(env, out, "kSpaceAvailableSizeIndex", 2);
  SetNamedInt(env, out, "kPhysicalSpaceSizeIndex", 3);

  SetNamedInt(env, out, "kCodeAndMetadataSizeIndex", 0);
  SetNamedInt(env, out, "kBytecodeAndMetadataSizeIndex", 1);
  SetNamedInt(env, out, "kExternalScriptSourceSizeIndex", 2);
  SetNamedInt(env, out, "kCPUProfilerMetaDataSizeIndex", 3);

  uint32_t heap_space_count = 0;
  napi_value heap_spaces = nullptr;
  napi_create_array_with_length(env, heap_space_count, &heap_spaces);
  if (unofficial_napi_get_heap_space_count(env, &heap_space_count) == napi_ok &&
      napi_create_array_with_length(env, heap_space_count, &heap_spaces) == napi_ok &&
      heap_spaces != nullptr) {
    for (uint32_t i = 0; i < heap_space_count; ++i) {
      unofficial_napi_heap_space_statistics stats{};
      if (unofficial_napi_get_heap_space_statistics(env, i, &stats) != napi_ok) continue;
      napi_value name = nullptr;
      if (napi_create_string_utf8(env, stats.space_name, NAPI_AUTO_LENGTH, &name) == napi_ok &&
          name != nullptr) {
        napi_set_element(env, heap_spaces, i, name);
      }
    }
  }
  if (heap_spaces != nullptr) napi_set_named_property(env, out, "kHeapSpaces", heap_spaces);

  napi_value heap_stats = CreateFloat64Array(env, kHeapStatisticsPropertiesCount);
  if (heap_stats != nullptr) napi_set_named_property(env, out, "heapStatisticsBuffer", heap_stats);
  napi_value heap_code_stats = CreateFloat64Array(env, kHeapCodeStatisticsPropertiesCount);
  if (heap_code_stats != nullptr) napi_set_named_property(env, out, "heapCodeStatisticsBuffer", heap_code_stats);
  napi_value heap_space_stats = CreateFloat64Array(env, kHeapSpaceStatisticsPropertiesCount);
  if (heap_space_stats != nullptr) napi_set_named_property(env, out, "heapSpaceStatisticsBuffer", heap_space_stats);

  V8BindingState& state = EnsureV8BindingState(env);
  StoreBufferRef(env, &state.heap_statistics_buffer_ref, heap_stats);
  StoreBufferRef(env, &state.heap_code_statistics_buffer_ref, heap_code_stats);
  StoreBufferRef(env, &state.heap_space_statistics_buffer_ref, heap_space_stats);

  napi_value detail_level = nullptr;
  if (napi_create_object(env, &detail_level) == napi_ok && detail_level != nullptr) {
    SetNamedInt(env, detail_level, "DETAILED", 0);
    SetNamedInt(env, detail_level, "BRIEF", 1);
    napi_set_named_property(env, out, "detailLevel", detail_level);
  }

  return out;
}

}  // namespace internal_binding
