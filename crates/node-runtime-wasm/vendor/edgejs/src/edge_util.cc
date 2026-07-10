#ifndef NAPI_EXPERIMENTAL
#define NAPI_EXPERIMENTAL
#endif

#include "edge_util.h"

#include "unofficial_napi.h"
#include "edge_environment.h"
#include "edge_module_loader.h"

#include <uv.h>

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <map>
#include <memory>
#include <unordered_set>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>
#include <vector>

#ifndef _WIN32
#include <sys/stat.h>
#endif

namespace {

constexpr int32_t kPromisePending = 0;
constexpr int32_t kPromiseFulfilled = 1;
constexpr int32_t kPromiseRejected = 2;
constexpr uint32_t kMaxCallSitesFrames = 200;
constexpr uint32_t kMaxRawCallSitesFrames = kMaxCallSitesFrames + 1;
constexpr uint32_t kEdgeInternalCallSiteFrames = 1;

constexpr int32_t kExitInfoKExiting = 0;
constexpr int32_t kExitInfoKExitCode = 1;
constexpr int32_t kExitInfoKHasExitCode = 2;

constexpr std::array<std::pair<const char*, const char*>, 20> kPrivateSymbolNames = {{
    {"arrow_message_private_symbol", "node:arrowMessage"},
    {"contextify_context_private_symbol", "node:contextify:context"},
    {"decorated_private_symbol", "node:decorated"},
    {"transfer_mode_private_symbol", "node:transfer_mode"},
    {"host_defined_option_symbol", "node:host_defined_option_symbol"},
    {"js_transferable_wrapper_private_symbol", "node:js_transferable_wrapper"},
    {"entry_point_module_private_symbol", "node:entry_point_module"},
    {"entry_point_promise_private_symbol", "node:entry_point_promise"},
    {"module_source_private_symbol", "node:module_source"},
    {"module_export_names_private_symbol", "node:module_export_names"},
    {"module_circular_visited_private_symbol", "node:module_circular_visited"},
    {"module_export_private_symbol", "node:module_export"},
    {"module_first_parent_private_symbol", "node:module_first_parent"},
    {"module_last_parent_private_symbol", "node:module_last_parent"},
    {"napi_type_tag", "node:napi:type_tag"},
    {"napi_wrapper", "node:napi:wrapper"},
    {"untransferable_object_private_symbol", "node:untransferableObject"},
    {"exit_info_private_symbol", "node:exit_info_private_symbol"},
    {"promise_trace_id", "node:promise_trace_id"},
    {"source_map_data_private_symbol", "node:source_map_data_private_symbol"},
}};

constexpr std::array<std::pair<const char*, const char*>, 21> kPerIsolateSymbolNames = {{
    {"fs_use_promises_symbol", "fs_use_promises_symbol"},
    {"async_id_symbol", "async_id_symbol"},
    {"constructor_key_symbol", "constructor_key_symbol"},
    {"handle_onclose_symbol", "handle_onclose"},
    {"no_message_symbol", "no_message_symbol"},
    {"messaging_deserialize_symbol", "messaging_deserialize_symbol"},
    {"imported_cjs_symbol", "imported_cjs_symbol"},
    {"messaging_transfer_symbol", "messaging_transfer_symbol"},
    {"messaging_clone_symbol", "messaging_clone_symbol"},
    {"messaging_transfer_list_symbol", "messaging_transfer_list_symbol"},
    {"oninit_symbol", "oninit"},
    {"owner_symbol", "owner_symbol"},
    {"onpskexchange_symbol", "onpskexchange"},
    {"resource_symbol", "resource_symbol"},
    {"trigger_async_id_symbol", "trigger_async_id_symbol"},
    {"source_text_module_default_hdo", "source_text_module_default_hdo"},
    {"vm_context_no_contextify", "vm_context_no_contextify"},
    {"vm_dynamic_import_default_internal", "vm_dynamic_import_default_internal"},
    {"vm_dynamic_import_main_context_default", "vm_dynamic_import_main_context_default"},
    {"vm_dynamic_import_missing_flag", "vm_dynamic_import_missing_flag"},
    {"vm_dynamic_import_no_callback", "vm_dynamic_import_no_callback"},
}};

struct LazyPropertyData {
  std::string module_id;
  std::string key;
  bool enumerable = true;
};

struct LazyPropertyStore {
  explicit LazyPropertyStore(napi_env) {}
  std::vector<std::unique_ptr<LazyPropertyData>> entries;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct TypesBindingState {
  explicit TypesBindingState(napi_env env_in) : env(env_in) {}
  ~TypesBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

LazyPropertyStore& GetLazyPropertyStore(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<LazyPropertyStore>(
      env,
      kEdgeEnvironmentSlotLazyPropertyStore);
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

napi_value Undefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value Null(napi_env env) {
  napi_value out = nullptr;
  napi_get_null(env, &out);
  return out;
}

bool SetNamedProperty(napi_env env, napi_value target, const char* key, napi_value value) {
  return target != nullptr && value != nullptr && napi_set_named_property(env, target, key, value) == napi_ok;
}

bool SetInt32(napi_env env, napi_value target, const char* key, int32_t value) {
  napi_value v = nullptr;
  return napi_create_int32(env, value, &v) == napi_ok && v != nullptr && SetNamedProperty(env, target, key, v);
}

bool SetBool(napi_env env, napi_value target, const char* key, bool value) {
  napi_value v = nullptr;
  return napi_get_boolean(env, value, &v) == napi_ok && v != nullptr && SetNamedProperty(env, target, key, v);
}

bool SetString(napi_env env, napi_value target, const char* key, const char* value) {
  napi_value v = nullptr;
  return napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr &&
         SetNamedProperty(env, target, key, v);
}

bool CreateNullPrototypeObject(napi_env env, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (napi_create_object(env, out) != napi_ok || *out == nullptr) return false;
  return node_api_set_prototype(env, *out, Null(env)) == napi_ok;
}

template <size_t N, typename CreateFn>
napi_value CreateSymbolObject(napi_env env,
                              const std::array<std::pair<const char*, const char*>, N>& entries,
                              CreateFn&& create_symbol) {
  napi_value out = nullptr;
  if (!CreateNullPrototypeObject(env, &out)) return nullptr;

  for (const auto& [key, description] : entries) {
    napi_value sym = nullptr;
    if (!create_symbol(description, &sym) || sym == nullptr || !SetNamedProperty(env, out, key, sym)) {
      return nullptr;
    }
  }

  return out;
}

napi_value GetNamedProperty(napi_env env, napi_value obj, const char* key) {
  if (obj == nullptr) return nullptr;
  bool has_prop = false;
  if (napi_has_named_property(env, obj, key, &has_prop) != napi_ok || !has_prop) return nullptr;
  napi_value out = nullptr;
  if (napi_get_named_property(env, obj, key, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

bool CreateLocationFromCallSite(napi_env env, napi_value callsite, napi_value* location_out) {
  if (callsite == nullptr || location_out == nullptr) return false;

  napi_value file = GetNamedProperty(env, callsite, "scriptNameOrSourceURL");
  if (file == nullptr) file = GetNamedProperty(env, callsite, "scriptName");
  napi_value line_v = GetNamedProperty(env, callsite, "lineNumber");
  napi_value column_v = GetNamedProperty(env, callsite, "columnNumber");
  if (file == nullptr || line_v == nullptr || column_v == nullptr) return false;

  napi_valuetype file_type = napi_undefined;
  size_t file_length = 0;
  if (napi_typeof(env, file, &file_type) != napi_ok || file_type != napi_string ||
      napi_get_value_string_utf8(env, file, nullptr, 0, &file_length) != napi_ok || file_length == 0) {
    return false;
  }

  uint32_t line = 0;
  uint32_t column = 0;
  if (napi_get_value_uint32(env, line_v, &line) != napi_ok ||
      napi_get_value_uint32(env, column_v, &column) != napi_ok) {
    return false;
  }

  napi_value location = nullptr;
  napi_value line_out = nullptr;
  napi_value column_out = nullptr;
  if (napi_create_array_with_length(env, 3, &location) != napi_ok || location == nullptr ||
      napi_create_uint32(env, line, &line_out) != napi_ok || line_out == nullptr ||
      napi_create_uint32(env, column, &column_out) != napi_ok || column_out == nullptr) {
    return false;
  }

  if (napi_set_element(env, location, 0, line_out) != napi_ok ||
      napi_set_element(env, location, 1, column_out) != napi_ok ||
      napi_set_element(env, location, 2, file) != napi_ok) {
    return false;
  }

  *location_out = location;
  return true;
}

uint32_t RawCallSiteFramesForEdge(uint32_t frames) {
  if (frames == 0) return 0;
  if (frames >= kMaxCallSitesFrames) return kMaxRawCallSitesFrames;
  return frames + kEdgeInternalCallSiteFrames;
}

bool CreateSkippedCallSitesArray(napi_env env,
                                 napi_value raw_callsites,
                                 uint32_t skip_frames,
                                 napi_value* out) {
  if (raw_callsites == nullptr || out == nullptr) return false;

  bool is_array = false;
  if (napi_is_array(env, raw_callsites, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, raw_callsites, &length) != napi_ok) return false;

  const uint32_t start = std::min(skip_frames, length);
  const uint32_t count = length - start;
  napi_value result = nullptr;
  if (napi_create_array_with_length(env, count, &result) != napi_ok || result == nullptr) {
    return false;
  }

  for (uint32_t i = 0; i < count; ++i) {
    napi_value callsite = nullptr;
    if (napi_get_element(env, raw_callsites, start + i, &callsite) != napi_ok ||
        napi_set_element(env, result, i, callsite) != napi_ok) {
      return false;
    }
  }

  *out = result;
  return true;
}

napi_value GetGlobal(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;
  return global;
}

napi_value GetGlobalNamed(napi_env env, const char* key) {
  return GetNamedProperty(env, GetGlobal(env), key);
}

bool IsFunction(napi_env env, napi_value value) {
  napi_valuetype t = napi_undefined;
  return value != nullptr && napi_typeof(env, value, &t) == napi_ok && t == napi_function;
}

bool IsObjectLike(napi_env env, napi_value value) {
  napi_valuetype t = napi_undefined;
  if (value == nullptr || napi_typeof(env, value, &t) != napi_ok) return false;
  return t == napi_object || t == napi_function;
}

std::string ToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return "";
  napi_value str = nullptr;
  if (napi_coerce_to_string(env, value, &str) != napi_ok || str == nullptr) return "";
  size_t length = 0;
  if (napi_get_value_string_utf8(env, str, nullptr, 0, &length) != napi_ok) return "";
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, str, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

bool ValueToUtf8IfString(napi_env env, napi_value value, std::string* out) {
  if (value == nullptr || out == nullptr) return false;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_string) return false;
  *out = ToUtf8(env, value);
  return true;
}

napi_value CallFunction(napi_env env,
                        napi_value this_arg,
                        napi_value fn,
                        size_t argc,
                        napi_value* argv) {
  if (this_arg == nullptr) this_arg = Undefined(env);
  if (!IsFunction(env, fn)) return nullptr;
  napi_value out = nullptr;
  if (napi_call_function(env, this_arg, fn, argc, argv, &out) != napi_ok) return nullptr;
  return out;
}

void ClearPendingException(napi_env env) {
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  (void)napi_get_and_clear_last_exception(env, &ignored);
}

bool TryCallFunction(napi_env env,
                     napi_value this_arg,
                     napi_value fn,
                     size_t argc,
                     napi_value* argv,
                     napi_value* result_out) {
  if (result_out != nullptr) *result_out = nullptr;
  if (this_arg == nullptr) this_arg = Undefined(env);
  if (!IsFunction(env, fn)) return false;

  napi_value out = nullptr;
  const napi_status status = napi_call_function(env, this_arg, fn, argc, argv, &out);
  if (status != napi_ok) {
    ClearPendingException(env);
    return false;
  }

  if (result_out != nullptr) *result_out = out;
  return true;
}

napi_value NewInstance(napi_env env, napi_value ctor, size_t argc, napi_value* argv) {
  if (!IsFunction(env, ctor)) return nullptr;
  napi_value out = nullptr;
  if (napi_new_instance(env, ctor, argc, argv, &out) != napi_ok) return nullptr;
  return out;
}

napi_value CreateMap(napi_env env) {
  napi_value map_ctor = GetGlobalNamed(env, "Map");
  if (!IsFunction(env, map_ctor)) return nullptr;
  return NewInstance(env, map_ctor, 0, nullptr);
}

bool MapSet(napi_env env, napi_value map, napi_value key, napi_value value) {
  napi_value set_fn = GetNamedProperty(env, map, "set");
  if (!IsFunction(env, set_fn)) return false;
  napi_value argv[2] = {key, value};
  return CallFunction(env, map, set_fn, 2, argv) != nullptr;
}

bool ValueToTagEquals(napi_env env, napi_value value, const char* expected) {
  napi_value object_ctor = GetGlobalNamed(env, "Object");
  napi_value object_prototype = GetNamedProperty(env, object_ctor, "prototype");
  napi_value to_string_fn = GetNamedProperty(env, object_prototype, "toString");
  if (!IsFunction(env, to_string_fn)) return false;
  napi_value out = CallFunction(env, value != nullptr ? value : Undefined(env), to_string_fn, 0, nullptr);
  if (out == nullptr) return false;
  return ToUtf8(env, out) == expected;
}

bool ValuePassesPrototypeMethodBrandCheck(napi_env env,
                                          napi_value value,
                                          const char* ctor_name,
                                          const char* method_name,
                                          napi_valuetype expected_type) {
  if (!IsObjectLike(env, value)) return false;

  napi_value ctor = GetGlobalNamed(env, ctor_name);
  napi_value prototype = GetNamedProperty(env, ctor, "prototype");
  napi_value method = GetNamedProperty(env, prototype, method_name);
  if (!IsFunction(env, method)) return false;

  napi_value result = nullptr;
  if (!TryCallFunction(env, value, method, 0, nullptr, &result) || result == nullptr) {
    return false;
  }

  napi_valuetype type = napi_undefined;
  return napi_typeof(env, result, &type) == napi_ok && type == expected_type;
}

bool ValuePassesPrototypeGetterBrandCheck(napi_env env,
                                          napi_value value,
                                          const char* ctor_name,
                                          const char* property_name,
                                          napi_valuetype expected_type) {
  if (!IsObjectLike(env, value)) return false;

  napi_value object_ctor = GetGlobalNamed(env, "Object");
  napi_value get_own_property_descriptor = GetNamedProperty(env, object_ctor, "getOwnPropertyDescriptor");
  napi_value ctor = GetGlobalNamed(env, ctor_name);
  napi_value prototype = GetNamedProperty(env, ctor, "prototype");
  if (!IsFunction(env, get_own_property_descriptor) || prototype == nullptr) return false;

  napi_value property_name_value = nullptr;
  if (napi_create_string_utf8(env, property_name, NAPI_AUTO_LENGTH, &property_name_value) != napi_ok ||
      property_name_value == nullptr) {
    return false;
  }

  napi_value descriptor_argv[2] = {prototype, property_name_value};
  napi_value descriptor = nullptr;
  if (!TryCallFunction(env,
                       object_ctor,
                       get_own_property_descriptor,
                       2,
                       descriptor_argv,
                       &descriptor) ||
      !IsObjectLike(env, descriptor)) {
    return false;
  }

  napi_value getter = GetNamedProperty(env, descriptor, "get");
  if (!IsFunction(env, getter)) return false;

  napi_value result = nullptr;
  if (!TryCallFunction(env, value, getter, 0, nullptr, &result) || result == nullptr) {
    return false;
  }

  napi_valuetype type = napi_undefined;
  return napi_typeof(env, result, &type) == napi_ok && type == expected_type;
}

bool IsInternalScriptName(std::string_view script_name) {
  if (script_name.empty()) return false;
  return script_name.rfind("node:", 0) == 0;
}

bool IsNodeModulesScriptName(std::string_view script_name) {
  return script_name.find("/node_modules/") != std::string::npos ||
         script_name.find("\\node_modules\\") != std::string::npos ||
         script_name.find("/node_modules\\") != std::string::npos ||
         script_name.find("\\node_modules/") != std::string::npos;
}

static uint32_t GetUVHandleTypeCode(uv_handle_type type) {
  switch (type) {
    case UV_TCP:
      return 0;
    case UV_TTY:
      return 1;
    case UV_UDP:
      return 2;
    case UV_FILE:
      return 3;
    case UV_NAMED_PIPE:
      return 4;
    case UV_UNKNOWN_HANDLE:
      return 5;
    default:
      return 5;
  }
}

napi_value GuessHandleType(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return Undefined(env);
  }
  int32_t fd = -1;
  if (napi_get_value_int32(env, argv[0], &fd) != napi_ok || fd < 0) {
    return Undefined(env);
  }
  const char* force_stdio_tty = std::getenv("EDGE_FORCE_STDIO_TTY");
  if (force_stdio_tty != nullptr && force_stdio_tty[0] == '1' && force_stdio_tty[1] == '\0' &&
      fd >= 0 && fd <= 2) {
    napi_value forced = nullptr;
    if (napi_create_uint32(env, GetUVHandleTypeCode(UV_TTY), &forced) == napi_ok && forced != nullptr) {
      return forced;
    }
    return Undefined(env);
  }
  uv_handle_type t = uv_guess_handle(static_cast<uv_file>(fd));
#ifndef _WIN32
  if (fd == 0 && t == UV_FILE) {
    struct stat fd_stat {};
    struct stat null_stat {};
    if (fstat(fd, &fd_stat) == 0 &&
        stat("/dev/null", &null_stat) == 0 &&
        S_ISCHR(fd_stat.st_mode) &&
        S_ISCHR(null_stat.st_mode) &&
        fd_stat.st_rdev == null_stat.st_rdev) {
      t = UV_NAMED_PIPE;
    }
  }
#endif
  napi_value result = nullptr;
  if (napi_create_uint32(env, GetUVHandleTypeCode(t), &result) != napi_ok || result == nullptr) {
    return Undefined(env);
  }
  return result;
}

napi_value IsInsideNodeModulesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t frame_limit = 10;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_number) {
      int32_t candidate = 0;
      if (napi_get_value_int32(env, argv[0], &candidate) == napi_ok) {
        frame_limit = std::max(frame_limit, candidate);
      }
    }
  }
  if (frame_limit < 1) frame_limit = 1;

  bool result = false;
  uint32_t frames = static_cast<uint32_t>(frame_limit);
  if (frames > kMaxCallSitesFrames) frames = kMaxCallSitesFrames;
  const uint32_t raw_frames = RawCallSiteFramesForEdge(frames);

  napi_value callsites = nullptr;
  if (unofficial_napi_get_call_sites(env, raw_frames, &callsites) == napi_ok && callsites != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, callsites, &is_array) == napi_ok && is_array) {
      uint32_t length = 0;
      if (napi_get_array_length(env, callsites, &length) == napi_ok) {
        for (uint32_t i = 0; i < length; ++i) {
          napi_value callsite = nullptr;
          if (napi_get_element(env, callsites, i, &callsite) != napi_ok || callsite == nullptr) continue;

          napi_value script_name = GetNamedProperty(env, callsite, "scriptName");
          std::string script_name_str;
          ValueToUtf8IfString(env, script_name, &script_name_str);
          if (script_name_str.empty()) {
            script_name = GetNamedProperty(env, callsite, "scriptNameOrSourceURL");
            script_name_str.clear();
            ValueToUtf8IfString(env, script_name, &script_name_str);
          }
          if (script_name_str.empty()) continue;
          if (IsInternalScriptName(script_name_str)) continue;

          if (IsNodeModulesScriptName(script_name_str)) {
            result = true;
          }
          break;
        }
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

void MaterializeLazyProperty(napi_env env, napi_value target, const LazyPropertyData& lazy, napi_value value) {
  if (!IsObjectLike(env, target)) return;
  if (value == nullptr) value = Undefined(env);

  napi_property_descriptor descriptor = {
      .utf8name = lazy.key.c_str(),
      .name = nullptr,
      .method = nullptr,
      .getter = nullptr,
      .setter = nullptr,
      .value = value,
      .attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable |
                                                          (lazy.enumerable ? napi_enumerable : napi_default)),
      .data = nullptr,
  };
  napi_define_properties(env, target, 1, &descriptor);
}

napi_value ResolveLazyPropertyValue(napi_env env, const LazyPropertyData& lazy) {
  napi_value require_fn = EdgeGetRequireFunction(env);
  if (!IsFunction(env, require_fn)) {
    require_fn = GetGlobalNamed(env, "require");
  }
  if (!IsFunction(env, require_fn)) return Undefined(env);

  napi_value id = nullptr;
  if (napi_create_string_utf8(env, lazy.module_id.c_str(), lazy.module_id.size(), &id) != napi_ok || id == nullptr) {
    return Undefined(env);
  }

  napi_value module = nullptr;
  napi_value argv_require[1] = {id};
  module = CallFunction(env, GetGlobal(env), require_fn, 1, argv_require);
  if (!IsObjectLike(env, module)) return Undefined(env);

  napi_value value = nullptr;
  if (napi_get_named_property(env, module, lazy.key.c_str(), &value) != napi_ok || value == nullptr) {
    value = Undefined(env);
  }

  return value;
}

napi_value LazyPropertyGetter(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  void* data = nullptr;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, &data) != napi_ok || data == nullptr) {
    return Undefined(env);
  }

  auto* lazy = static_cast<LazyPropertyData*>(data);
  napi_value value = ResolveLazyPropertyValue(env, *lazy);
  MaterializeLazyProperty(env, this_arg, *lazy, value);
  return value;
}

napi_value LazyPropertySetter(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  void* data = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, &data) != napi_ok || data == nullptr) {
    return Undefined(env);
  }

  auto* lazy = static_cast<LazyPropertyData*>(data);
  napi_value value = (argc >= 1 && argv[0] != nullptr) ? argv[0] : Undefined(env);
  MaterializeLazyProperty(env, this_arg, *lazy, value);
  return Undefined(env);
}

void TrySetLazyGetterName(napi_env env, napi_value target, const std::string& key) {
  if (!IsObjectLike(env, target) || key.empty()) return;
  napi_value object_ctor = GetGlobalNamed(env, "Object");
  napi_value get_own_property_descriptor = GetNamedProperty(env, object_ctor, "getOwnPropertyDescriptor");
  napi_value define_property = GetNamedProperty(env, object_ctor, "defineProperty");
  if (!IsFunction(env, get_own_property_descriptor) || !IsFunction(env, define_property)) return;

  napi_value key_value = nullptr;
  if (napi_create_string_utf8(env, key.c_str(), key.size(), &key_value) != napi_ok || key_value == nullptr) return;

  napi_value get_desc_argv[2] = {target, key_value};
  napi_value desc = CallFunction(env, object_ctor, get_own_property_descriptor, 2, get_desc_argv);
  if (!IsObjectLike(env, desc)) return;

  napi_value getter = GetNamedProperty(env, desc, "get");
  if (!IsFunction(env, getter)) return;

  napi_value name_value = nullptr;
  if (napi_create_string_utf8(env, ("get " + key).c_str(), NAPI_AUTO_LENGTH, &name_value) != napi_ok ||
      name_value == nullptr) {
    return;
  }

  napi_value name_descriptor = nullptr;
  if (napi_create_object(env, &name_descriptor) != napi_ok || name_descriptor == nullptr) return;
  if (napi_set_named_property(env, name_descriptor, "value", name_value) != napi_ok) return;

  napi_value name_key = nullptr;
  if (napi_create_string_utf8(env, "name", NAPI_AUTO_LENGTH, &name_key) != napi_ok || name_key == nullptr) return;

  napi_value define_argv[3] = {getter, name_key, name_descriptor};
  if (CallFunction(env, object_ctor, define_property, 3, define_argv) == nullptr) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value ignored = nullptr;
      napi_get_and_clear_last_exception(env, &ignored);
    }
  }
}

napi_value DefineLazyPropertiesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) {
    return Undefined(env);
  }

  napi_value target = argv[0];
  napi_value id = argv[1];
  napi_value keys = argv[2];

  bool is_array = false;
  napi_is_array(env, keys, &is_array);
  if (!IsObjectLike(env, target) || !is_array) {
    return Undefined(env);
  }

  bool enumerable = true;
  if (argc >= 4 && argv[3] != nullptr) {
    napi_get_value_bool(env, argv[3], &enumerable);
  }

  const std::string module_id = ToUtf8(env, id);
  LazyPropertyStore& store = GetLazyPropertyStore(env);

  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return Undefined(env);
  for (uint32_t i = 0; i < key_count; i++) {
    napi_value key_val = nullptr;
    if (napi_get_element(env, keys, i, &key_val) != napi_ok || key_val == nullptr) continue;
    const std::string key = ToUtf8(env, key_val);
    if (key.empty()) continue;

    auto data = std::make_unique<LazyPropertyData>();
    data->module_id = module_id;
    data->key = key;
    data->enumerable = enumerable;
    LazyPropertyData* raw_data = data.get();
    store.entries.emplace_back(std::move(data));

    napi_property_descriptor descriptor = {
        .utf8name = raw_data->key.c_str(),
        .name = nullptr,
        .method = nullptr,
        .getter = LazyPropertyGetter,
        .setter = LazyPropertySetter,
        .value = nullptr,
        .attributes = static_cast<napi_property_attributes>(napi_configurable |
                                                            (enumerable ? napi_enumerable : napi_default)),
        .data = raw_data,
    };
    napi_define_properties(env, target, 1, &descriptor);
    TrySetLazyGetterName(env, target, key);
  }

  return Undefined(env);
}

napi_value ConstructSharedArrayBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return Undefined(env);
  }

  napi_value num = nullptr;
  if (napi_coerce_to_number(env, argv[0], &num) != napi_ok || num == nullptr) {
    return Undefined(env);
  }

  int64_t length = 0;
  if (napi_get_value_int64(env, num, &length) != napi_ok) {
    return Undefined(env);
  }
  if (length < 0) {
    napi_throw_range_error(env, nullptr, "Invalid array buffer length");
    return nullptr;
  }
  void* data = nullptr;
  napi_value out = nullptr;
  if (node_api_create_sharedarraybuffer(env, static_cast<size_t>(length), &data, &out) != napi_ok ||
      out == nullptr) {
    napi_throw_range_error(env, nullptr, "Array buffer allocation failed");
    return nullptr;
  }
  return out;
}

napi_value GetOwnNonIndexPropertiesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) {
    return Undefined(env);
  }

  napi_value source = argv[0];
  uint32_t filter_bits = 0;
  napi_get_value_uint32(env, argv[1], &filter_bits);
  napi_value global = GetGlobal(env);
  bool source_is_global = false;
  if (global != nullptr) {
    (void)napi_strict_equals(env, source, global, &source_is_global);
  }

  napi_value keys = nullptr;
  if (unofficial_napi_get_own_non_index_properties(env, source, filter_bits, &keys) != napi_ok ||
      keys == nullptr) {
    return Undefined(env);
  }

  uint32_t key_count = 0;
  napi_get_array_length(env, keys, &key_count);

  napi_value out = nullptr;
  if (napi_create_array(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  uint32_t out_idx = 0;
  for (uint32_t i = 0; i < key_count; i++) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, key, &type) != napi_ok) continue;
    if (type != napi_string) {
      napi_set_element(env, out, out_idx++, key);
      continue;
    }

    const std::string text = ToUtf8(env, key);
    // internalBinding() is bootstrap-only in Node and should not leak into
    // REPL/global completion surfaces.
    if (source_is_global &&
        (text == "internalBinding" || text == "getInternalBinding" || text == "getLinkedBinding")) {
      continue;
    }
    napi_set_element(env, out, out_idx++, key);
  }

  return out;
}

napi_value GetConstructorNameCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) {
    napi_value empty = nullptr;
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty);
    return empty != nullptr ? empty : Undefined(env);
  }

  napi_value out = nullptr;
  if (unofficial_napi_get_constructor_name(env, argv[0], &out) == napi_ok && out != nullptr) {
    return out;
  }

  napi_value empty = nullptr;
  napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty);
  return empty != nullptr ? empty : Undefined(env);
}

napi_value GetExternalValueCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  uint64_t value = 0;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, argv[0], &t) == napi_ok && t == napi_external) {
      void* ptr = nullptr;
      if (napi_get_value_external(env, argv[0], &ptr) == napi_ok) {
        value = static_cast<uint64_t>(reinterpret_cast<uintptr_t>(ptr));
      }
    }
  }

  napi_value out = nullptr;
  if (napi_create_bigint_uint64(env, value, &out) != napi_ok || out == nullptr) return Undefined(env);
  return out;
}

napi_value GetPromiseDetailsCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  bool is_promise = false;
  if (napi_is_promise(env, argv[0], &is_promise) != napi_ok || !is_promise) return Undefined(env);

  int32_t state = 0;
  bool has_result = false;
  napi_value result = nullptr;
  if (unofficial_napi_get_promise_details(env, argv[0], &state, &result, &has_result) != napi_ok) {
    return Undefined(env);
  }

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, has_result ? 2 : 1, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  napi_value state_v = nullptr;
  napi_create_int32(env, state, &state_v);
  napi_set_element(env, out, 0, state_v);
  if (has_result && result != nullptr) {
    napi_set_element(env, out, 1, result);
  }
  return out;
}

napi_value GetProxyDetailsCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  napi_value target = nullptr;
  napi_value handler = nullptr;
  if (unofficial_napi_get_proxy_details(env, argv[0], &target, &handler) != napi_ok || target == nullptr ||
      handler == nullptr) {
    return Undefined(env);
  }

  bool full = true;
  if (argc >= 2 && argv[1] != nullptr) {
    napi_get_value_bool(env, argv[1], &full);
  }

  if (!full) return target;

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 2, &out) != napi_ok || out == nullptr) return Undefined(env);
  napi_set_element(env, out, 0, target);
  napi_set_element(env, out, 1, handler);
  return out;
}

napi_value GetCallerLocationCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value callsites = nullptr;
  if (unofficial_napi_get_call_sites(env, RawCallSiteFramesForEdge(64), &callsites) != napi_ok ||
      callsites == nullptr) {
    return Undefined(env);
  }

  bool is_array = false;
  if (napi_is_array(env, callsites, &is_array) != napi_ok || !is_array) return Undefined(env);

  uint32_t length = 0;
  if (napi_get_array_length(env, callsites, &length) != napi_ok || length == 0) return Undefined(env);

  for (uint32_t i = std::min(kEdgeInternalCallSiteFrames, length); i < length; ++i) {
    napi_value callsite = nullptr;
    if (napi_get_element(env, callsites, i, &callsite) != napi_ok || callsite == nullptr) continue;
    napi_value location = nullptr;
    if (CreateLocationFromCallSite(env, callsite, &location) && location != nullptr) {
      return location;
    }
  }

  return Undefined(env);
}

napi_value PreviewEntriesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  napi_value entries = nullptr;
  bool is_key_value = false;
  if (unofficial_napi_preview_entries(env, argv[0], &entries, &is_key_value) != napi_ok || entries == nullptr) {
    return Undefined(env);
  }

  if (argc < 2) return entries;

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 2, &out) != napi_ok || out == nullptr) return Undefined(env);
  napi_set_element(env, out, 0, entries);
  napi_value is_key_value_v = nullptr;
  napi_get_boolean(env, is_key_value, &is_key_value_v);
  napi_set_element(env, out, 1, is_key_value_v);
  return out;
}

napi_value SleepCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  uint32_t msec = 0;
  if (napi_get_value_uint32(env, argv[0], &msec) != napi_ok) return Undefined(env);
  uv_sleep(msec);
  return Undefined(env);
}

std::string_view TrimSpaces(std::string_view input) {
  if (input.empty()) return "";
  size_t start = input.find_first_not_of(" \t\n");
  if (start == std::string_view::npos) return "";
  size_t end = input.find_last_not_of(" \t\n");
  if (end == std::string_view::npos) return input.substr(start);
  return input.substr(start, end - start + 1);
}

std::map<std::string, std::string> ParseDotenvContent(const std::string& input) {
  std::map<std::string, std::string> store;

  std::string lines = input;
  lines.erase(std::remove(lines.begin(), lines.end(), '\r'), lines.end());
  std::string_view content = TrimSpaces(lines);

  while (!content.empty()) {
    if (content.front() == '\n' || content.front() == '#') {
      size_t newline = content.find('\n');
      if (newline == std::string_view::npos) {
        content = {};
      } else {
        content.remove_prefix(newline + 1);
      }
      continue;
    }

    size_t equal_or_newline = content.find_first_of("=\n");
    if (equal_or_newline == std::string_view::npos || content[equal_or_newline] == '\n') {
      if (equal_or_newline == std::string_view::npos) break;
      content.remove_prefix(equal_or_newline + 1);
      content = TrimSpaces(content);
      continue;
    }

    std::string_view key = TrimSpaces(content.substr(0, equal_or_newline));
    content.remove_prefix(equal_or_newline + 1);

    if (key.starts_with("export ")) {
      key.remove_prefix(7);
      key = TrimSpaces(key);
    }

    if (key.empty()) {
      size_t newline = content.find('\n');
      if (newline == std::string_view::npos) break;
      content.remove_prefix(newline + 1);
      content = TrimSpaces(content);
      continue;
    }

    if (content.empty() || content.front() == '\n') {
      store[std::string(key)] = "";
      if (!content.empty()) content.remove_prefix(1);
      continue;
    }

    content = TrimSpaces(content);
    if (content.empty()) {
      store[std::string(key)] = "";
      break;
    }

    if (content.front() == '"') {
      size_t closing = content.find('"', 1);
      if (closing != std::string_view::npos) {
        std::string value(content.substr(1, closing - 1));
        size_t pos = 0;
        while ((pos = value.find("\\n", pos)) != std::string::npos) {
          value.replace(pos, 2, "\n");
          pos += 1;
        }
        store[std::string(key)] = value;
        size_t newline = content.find('\n', closing + 1);
        if (newline == std::string_view::npos) {
          content = {};
        } else {
          content.remove_prefix(newline + 1);
        }
        content = TrimSpaces(content);
        continue;
      }
    }

    if (content.front() == '\'' || content.front() == '"' || content.front() == '`') {
      char quote = content.front();
      size_t closing = content.find(quote, 1);
      if (closing == std::string_view::npos) {
        size_t newline = content.find('\n');
        std::string value(newline == std::string_view::npos ? content : content.substr(0, newline));
        store[std::string(key)] = value;
        if (newline == std::string_view::npos) {
          content = {};
        } else {
          content.remove_prefix(newline + 1);
        }
      } else {
        store[std::string(key)] = std::string(content.substr(1, closing - 1));
        size_t newline = content.find('\n', closing + 1);
        if (newline == std::string_view::npos) {
          content = {};
        } else {
          content.remove_prefix(newline + 1);
        }
      }
      content = TrimSpaces(content);
      continue;
    }

    size_t newline = content.find('\n');
    std::string_view value = (newline == std::string_view::npos) ? content : content.substr(0, newline);
    size_t hash = value.find('#');
    if (hash != std::string_view::npos) value = value.substr(0, hash);
    store[std::string(key)] = std::string(TrimSpaces(value));
    if (newline == std::string_view::npos) {
      content = {};
    } else {
      content.remove_prefix(newline + 1);
      content = TrimSpaces(content);
    }
  }

  return store;
}

napi_value ParseEnvCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  const std::string content = ToUtf8(env, argv[0]);
  const std::map<std::string, std::string> parsed = ParseDotenvContent(content);

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  for (const auto& [key, value] : parsed) {
    napi_value value_v = nullptr;
    if (napi_create_string_utf8(env, value.c_str(), value.size(), &value_v) == napi_ok && value_v != nullptr) {
      napi_set_named_property(env, out, key.c_str(), value_v);
    }
  }

  return out;
}

napi_value ArrayBufferViewHasBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  bool result = false;
  if (argc >= 1 && argv[0] != nullptr) {
    unofficial_napi_arraybuffer_view_has_buffer(env, argv[0], &result);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value GetCallSitesCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  uint32_t frames = 0;
  if (napi_get_value_uint32(env, argv[0], &frames) != napi_ok) return Undefined(env);
  if (frames == 0 || frames > kMaxCallSitesFrames) return Undefined(env);

  const uint32_t raw_frames = RawCallSiteFramesForEdge(frames);
  napi_value raw_out = nullptr;
  if (unofficial_napi_get_call_sites(env, raw_frames, &raw_out) != napi_ok || raw_out == nullptr) {
    return Undefined(env);
  }

  napi_value out = nullptr;
  if (!CreateSkippedCallSitesArray(env, raw_out, kEdgeInternalCallSiteFrames, &out) || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

bool DefineMethod(napi_env env, napi_value target, const char* name, napi_callback cb, void* data = nullptr) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, data, &fn) != napi_ok || fn == nullptr) return false;
  return SetNamedProperty(env, target, name, fn);
}

napi_value CreatePrivateSymbolsObject(napi_env env) {
  return CreateSymbolObject(
      env,
      kPrivateSymbolNames,
      [&](const char* description, napi_value* out) {
        return unofficial_napi_create_private_symbol(env, description, NAPI_AUTO_LENGTH, out) == napi_ok;
      });
}

napi_value CreatePerIsolateSymbolsObject(napi_env env) {
  return CreateSymbolObject(
      env,
      kPerIsolateSymbolNames,
      [&](const char* description, napi_value* out) {
        napi_value desc = nullptr;
        return napi_create_string_utf8(env, description, NAPI_AUTO_LENGTH, &desc) == napi_ok &&
               desc != nullptr &&
               napi_create_symbol(env, desc, out) == napi_ok;
      });
}

bool InstallPrivateSymbols(napi_env env, napi_value binding) {
  napi_value private_symbols = EdgeGetPrivateSymbols(env);
  if (private_symbols == nullptr) {
    private_symbols = CreatePrivateSymbolsObject(env);
    if (private_symbols == nullptr) return false;
    EdgeSetPrivateSymbols(env, private_symbols);
  }

  return SetNamedProperty(env, binding, "privateSymbols", private_symbols);
}

bool InstallConstants(napi_env env, napi_value binding) {
  napi_value constants = nullptr;
  if (napi_create_object(env, &constants) != napi_ok || constants == nullptr) return false;

  if (!SetInt32(env, constants, "kPending", kPromisePending) ||
      !SetInt32(env, constants, "kFulfilled", kPromiseFulfilled) ||
      !SetInt32(env, constants, "kRejected", kPromiseRejected) ||
      !SetInt32(env, constants, "kExiting", kExitInfoKExiting) ||
      !SetInt32(env, constants, "kExitCode", kExitInfoKExitCode) ||
      !SetInt32(env, constants, "kHasExitCode", kExitInfoKHasExitCode) ||
      !SetInt32(env, constants, "ALL_PROPERTIES", 0) ||
      !SetInt32(env, constants, "ONLY_WRITABLE", 1) ||
      !SetInt32(env, constants, "ONLY_ENUMERABLE", 2) ||
      !SetInt32(env, constants, "ONLY_CONFIGURABLE", 4) ||
      !SetInt32(env, constants, "SKIP_STRINGS", 8) ||
      !SetInt32(env, constants, "SKIP_SYMBOLS", 16) ||
      !SetInt32(env, constants, "kDisallowCloneAndTransfer", 0) ||
      !SetInt32(env, constants, "kTransferable", 1) ||
      !SetInt32(env, constants, "kCloneable", 2)) {
    return false;
  }

  return SetNamedProperty(env, binding, "constants", constants);
}

bool InstallShouldAbortToggle(napi_env env, napi_value binding) {
  napi_value ab = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, sizeof(uint32_t), &data, &ab) != napi_ok || ab == nullptr || data == nullptr) {
    return false;
  }
  static_cast<uint32_t*>(data)[0] = 1;

  napi_value out = nullptr;
  if (napi_create_typedarray(env, napi_uint32_array, 1, ab, 0, &out) != napi_ok || out == nullptr) return false;
  return SetNamedProperty(env, binding, "shouldAbortOnUncaughtToggle", out);
}

enum class TypeCheckKind : uintptr_t {
  kExternal,
  kDate,
  kArgumentsObject,
  kBooleanObject,
  kNumberObject,
  kStringObject,
  kSymbolObject,
  kBigIntObject,
  kNativeError,
  kRegExp,
  kAsyncFunction,
  kGeneratorFunction,
  kGeneratorObject,
  kPromise,
  kMap,
  kSet,
  kMapIterator,
  kSetIterator,
  kWeakMap,
  kWeakSet,
  kArrayBuffer,
  kDataView,
  kSharedArrayBuffer,
  kProxy,
  kModuleNamespaceObject,
  kAnyArrayBuffer,
  kBoxedPrimitive,
};

bool RunTypeCheck(napi_env env, TypeCheckKind kind, napi_value value) {
  switch (kind) {
    case TypeCheckKind::kExternal: {
      napi_valuetype t = napi_undefined;
      return napi_typeof(env, value, &t) == napi_ok && t == napi_external;
    }
    case TypeCheckKind::kDate: {
      bool out = false;
      return napi_is_date(env, value, &out) == napi_ok && out;
    }
    case TypeCheckKind::kArgumentsObject:
      return ValueToTagEquals(env, value, "[object Arguments]");
    case TypeCheckKind::kBooleanObject:
      return ValuePassesPrototypeMethodBrandCheck(env, value, "Boolean", "valueOf", napi_boolean);
    case TypeCheckKind::kNumberObject:
      return ValuePassesPrototypeMethodBrandCheck(env, value, "Number", "valueOf", napi_number);
    case TypeCheckKind::kStringObject:
      return ValuePassesPrototypeMethodBrandCheck(env, value, "String", "valueOf", napi_string);
    case TypeCheckKind::kSymbolObject:
      return ValuePassesPrototypeMethodBrandCheck(env, value, "Symbol", "valueOf", napi_symbol);
    case TypeCheckKind::kBigIntObject:
      return ValuePassesPrototypeMethodBrandCheck(env, value, "BigInt", "valueOf", napi_bigint);
    case TypeCheckKind::kNativeError: {
      bool out = false;
      return napi_is_error(env, value, &out) == napi_ok && out;
    }
    case TypeCheckKind::kRegExp:
      return ValuePassesPrototypeGetterBrandCheck(env, value, "RegExp", "source", napi_string);
    case TypeCheckKind::kAsyncFunction:
      return ValueToTagEquals(env, value, "[object AsyncFunction]");
    case TypeCheckKind::kGeneratorFunction:
      return ValueToTagEquals(env, value, "[object GeneratorFunction]") ||
             ValueToTagEquals(env, value, "[object AsyncGeneratorFunction]");
    case TypeCheckKind::kGeneratorObject:
      return ValueToTagEquals(env, value, "[object Generator]");
    case TypeCheckKind::kPromise: {
      bool out = false;
      return napi_is_promise(env, value, &out) == napi_ok && out;
    }
    case TypeCheckKind::kMap:
      return ValueToTagEquals(env, value, "[object Map]");
    case TypeCheckKind::kSet:
      return ValueToTagEquals(env, value, "[object Set]");
    case TypeCheckKind::kMapIterator:
      return ValueToTagEquals(env, value, "[object Map Iterator]");
    case TypeCheckKind::kSetIterator:
      return ValueToTagEquals(env, value, "[object Set Iterator]");
    case TypeCheckKind::kWeakMap:
      return ValueToTagEquals(env, value, "[object WeakMap]");
    case TypeCheckKind::kWeakSet:
      return ValueToTagEquals(env, value, "[object WeakSet]");
    case TypeCheckKind::kArrayBuffer: {
      bool out = false;
      return napi_is_arraybuffer(env, value, &out) == napi_ok && out;
    }
    case TypeCheckKind::kDataView: {
      bool out = false;
      return napi_is_dataview(env, value, &out) == napi_ok && out;
    }
    case TypeCheckKind::kSharedArrayBuffer:
      return ValueToTagEquals(env, value, "[object SharedArrayBuffer]");
    case TypeCheckKind::kProxy: {
      napi_value target = nullptr;
      napi_value handler = nullptr;
      return unofficial_napi_get_proxy_details(env, value, &target, &handler) == napi_ok;
    }
    case TypeCheckKind::kModuleNamespaceObject:
      return ValueToTagEquals(env, value, "[object Module]");
    case TypeCheckKind::kAnyArrayBuffer:
      return RunTypeCheck(env, TypeCheckKind::kArrayBuffer, value) ||
             RunTypeCheck(env, TypeCheckKind::kSharedArrayBuffer, value);
    case TypeCheckKind::kBoxedPrimitive:
      return RunTypeCheck(env, TypeCheckKind::kNumberObject, value) ||
             RunTypeCheck(env, TypeCheckKind::kStringObject, value) ||
             RunTypeCheck(env, TypeCheckKind::kBooleanObject, value) ||
             RunTypeCheck(env, TypeCheckKind::kBigIntObject, value) ||
             RunTypeCheck(env, TypeCheckKind::kSymbolObject, value);
  }

  return false;
}

napi_value TypeCheckCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, nullptr, &data);

  bool result = false;
  if (data != nullptr && argc >= 1 && argv[0] != nullptr) {
    result = RunTypeCheck(env, static_cast<TypeCheckKind>(reinterpret_cast<uintptr_t>(data)), argv[0]);
  }

  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

bool DefineTypePredicate(napi_env env, napi_value target, const char* name, TypeCheckKind kind) {
  return DefineMethod(env,
                      target,
                      name,
                      TypeCheckCallback,
                      reinterpret_cast<void*>(static_cast<uintptr_t>(kind)));
}

bool InstallTypesBinding(napi_env env) {
  auto& state =
      EdgeEnvironmentGetOrCreateSlotData<TypesBindingState>(env, kEdgeEnvironmentSlotTypesBindingState);
  napi_value types = nullptr;
  if (napi_create_object(env, &types) != napi_ok || types == nullptr) return false;

  if (!DefineTypePredicate(env, types, "isExternal", TypeCheckKind::kExternal) ||
      !DefineTypePredicate(env, types, "isDate", TypeCheckKind::kDate) ||
      !DefineTypePredicate(env, types, "isArgumentsObject", TypeCheckKind::kArgumentsObject) ||
      !DefineTypePredicate(env, types, "isBooleanObject", TypeCheckKind::kBooleanObject) ||
      !DefineTypePredicate(env, types, "isNumberObject", TypeCheckKind::kNumberObject) ||
      !DefineTypePredicate(env, types, "isStringObject", TypeCheckKind::kStringObject) ||
      !DefineTypePredicate(env, types, "isSymbolObject", TypeCheckKind::kSymbolObject) ||
      !DefineTypePredicate(env, types, "isBigIntObject", TypeCheckKind::kBigIntObject) ||
      !DefineTypePredicate(env, types, "isNativeError", TypeCheckKind::kNativeError) ||
      !DefineTypePredicate(env, types, "isRegExp", TypeCheckKind::kRegExp) ||
      !DefineTypePredicate(env, types, "isAsyncFunction", TypeCheckKind::kAsyncFunction) ||
      !DefineTypePredicate(env, types, "isGeneratorFunction", TypeCheckKind::kGeneratorFunction) ||
      !DefineTypePredicate(env, types, "isGeneratorObject", TypeCheckKind::kGeneratorObject) ||
      !DefineTypePredicate(env, types, "isPromise", TypeCheckKind::kPromise) ||
      !DefineTypePredicate(env, types, "isMap", TypeCheckKind::kMap) ||
      !DefineTypePredicate(env, types, "isSet", TypeCheckKind::kSet) ||
      !DefineTypePredicate(env, types, "isMapIterator", TypeCheckKind::kMapIterator) ||
      !DefineTypePredicate(env, types, "isSetIterator", TypeCheckKind::kSetIterator) ||
      !DefineTypePredicate(env, types, "isWeakMap", TypeCheckKind::kWeakMap) ||
      !DefineTypePredicate(env, types, "isWeakSet", TypeCheckKind::kWeakSet) ||
      !DefineTypePredicate(env, types, "isArrayBuffer", TypeCheckKind::kArrayBuffer) ||
      !DefineTypePredicate(env, types, "isDataView", TypeCheckKind::kDataView) ||
      !DefineTypePredicate(env, types, "isSharedArrayBuffer", TypeCheckKind::kSharedArrayBuffer) ||
      !DefineTypePredicate(env, types, "isProxy", TypeCheckKind::kProxy) ||
      !DefineTypePredicate(env, types, "isModuleNamespaceObject", TypeCheckKind::kModuleNamespaceObject) ||
      !DefineTypePredicate(env, types, "isAnyArrayBuffer", TypeCheckKind::kAnyArrayBuffer) ||
      !DefineTypePredicate(env, types, "isBoxedPrimitive", TypeCheckKind::kBoxedPrimitive)) {
    return false;
  }

  DeleteRefIfPresent(env, &state.binding_ref);
  napi_ref ref = nullptr;
  if (napi_create_reference(env, types, 1, &ref) != napi_ok || ref == nullptr) return false;
  state.binding_ref = ref;
  return true;
}

}  // namespace

napi_value EdgeCreatePrivateSymbolsObject(napi_env env) {
  return CreatePrivateSymbolsObject(env);
}

napi_value EdgeCreatePerIsolateSymbolsObject(napi_env env) {
  return CreatePerIsolateSymbolsObject(env);
}

napi_value EdgeInstallUtilBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  if (!InstallPrivateSymbols(env, binding) || !InstallConstants(env, binding) ||
      !InstallShouldAbortToggle(env, binding)) {
    return nullptr;
  }

  if (!DefineMethod(env, binding, "isInsideNodeModules", IsInsideNodeModulesCallback) ||
      !DefineMethod(env, binding, "defineLazyProperties", DefineLazyPropertiesCallback) ||
      !DefineMethod(env, binding, "getPromiseDetails", GetPromiseDetailsCallback) ||
      !DefineMethod(env, binding, "getProxyDetails", GetProxyDetailsCallback) ||
      !DefineMethod(env, binding, "getCallerLocation", GetCallerLocationCallback) ||
      !DefineMethod(env, binding, "previewEntries", PreviewEntriesCallback) ||
      !DefineMethod(env, binding, "getOwnNonIndexProperties", GetOwnNonIndexPropertiesCallback) ||
      !DefineMethod(env, binding, "getConstructorName", GetConstructorNameCallback) ||
      !DefineMethod(env, binding, "getExternalValue", GetExternalValueCallback) ||
      !DefineMethod(env, binding, "getCallSites", GetCallSitesCallback) ||
      !DefineMethod(env, binding, "sleep", SleepCallback) ||
      !DefineMethod(env, binding, "parseEnv", ParseEnvCallback) ||
      !DefineMethod(env, binding, "arrayBufferViewHasBuffer", ArrayBufferViewHasBufferCallback) ||
      !DefineMethod(env, binding, "constructSharedArrayBuffer", ConstructSharedArrayBufferCallback) ||
      !DefineMethod(env, binding, "guessHandleType", GuessHandleType)) {
    return nullptr;
  }

  if (!InstallTypesBinding(env)) return nullptr;
  return binding;
}

napi_value EdgeGetTypesBinding(napi_env env) {
  auto* state = EdgeEnvironmentGetSlotData<TypesBindingState>(env, kEdgeEnvironmentSlotTypesBindingState);
  return state != nullptr ? GetRefValue(env, state->binding_ref) : nullptr;
}
