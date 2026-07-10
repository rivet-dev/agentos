#include "internal_binding/binding_initializers.h"

#include <filesystem>
#include <string>
#include <system_error>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"
#include "../edge_bytecode_cache.h"
#include "../edge_module_loader.h"
#include "../edge_path.h"

namespace internal_binding {

namespace {

void ResetRef(napi_env env, napi_ref* ref_ptr);

enum ModuleWrapStatus : int32_t {
  kUninstantiated = 0,
  kInstantiating = 1,
  kInstantiated = 2,
  kEvaluating = 3,
  kEvaluated = 4,
  kErrored = 5,
};

constexpr int32_t kSourcePhase = 1;
constexpr int32_t kEvaluationPhase = 2;

struct ModuleWrapInstance {
  napi_ref wrapper_ref = nullptr;
  napi_ref url_ref = nullptr;
  void* module_handle = nullptr;
  bool has_top_level_await = false;
};

struct ModuleWrapBindingState {
  explicit ModuleWrapBindingState(napi_env env_in) : env(env_in) {}
  ~ModuleWrapBindingState() {
    ResetRef(env, &binding_ref);
    ResetRef(env, &module_wrap_ctor_ref);
    ResetRef(env, &import_module_dynamically_ref);
    ResetRef(env, &initialize_import_meta_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref module_wrap_ctor_ref = nullptr;
  napi_ref import_module_dynamically_ref = nullptr;
  napi_ref initialize_import_meta_ref = nullptr;
};

ModuleWrapBindingState* GetBindingState(napi_env env) {
  return EdgeEnvironmentGetSlotData<ModuleWrapBindingState>(
      env, kEdgeEnvironmentSlotInternalModuleWrapBindingState);
}

ModuleWrapBindingState& EnsureBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<ModuleWrapBindingState>(
      env, kEdgeEnvironmentSlotInternalModuleWrapBindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

void ResetRef(napi_env env, napi_ref* ref_ptr) {
  if (ref_ptr == nullptr || *ref_ptr == nullptr) return;
  napi_delete_reference(env, *ref_ptr);
  *ref_ptr = nullptr;
}

void SetRef(napi_env env, napi_ref* ref_ptr, napi_value value, napi_valuetype required) {
  if (ref_ptr == nullptr) return;
  ResetRef(env, ref_ptr);
  if (value == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != required) return;
  napi_create_reference(env, value, 1, ref_ptr);
}

ModuleWrapInstance* UnwrapModuleWrap(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<ModuleWrapInstance*>(data);
}

void ModuleWrapFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* instance = static_cast<ModuleWrapInstance*>(data);
  if (instance == nullptr) return;
  if (instance->module_handle != nullptr) {
    (void)unofficial_napi_module_wrap_destroy(env, instance->module_handle);
    instance->module_handle = nullptr;
  }
  ResetRef(env, &instance->wrapper_ref);
  ResetRef(env, &instance->url_ref);
  delete instance;
}

void SetNamedInt(napi_env env, napi_value obj, const char* key, int32_t value) {
  napi_value out = nullptr;
  if (napi_create_int32(env, value, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, obj, key, out);
  }
}

void SetNamedBool(napi_env env, napi_value obj, const char* key, bool value) {
  napi_value out = nullptr;
  if (napi_get_boolean(env, value, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, obj, key, out);
  }
}

bool GetNamedProperty(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (env == nullptr || obj == nullptr || key == nullptr) return false;

  bool has_prop = false;
  if (napi_has_named_property(env, obj, key, &has_prop) != napi_ok || !has_prop) {
    return false;
  }

  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

bool IsFunctionValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool CallFunction(napi_env env,
                  napi_value recv,
                  napi_value fn,
                  size_t argc,
                  napi_value* argv,
                  napi_value* result) {
  napi_value local_result = nullptr;
  if (env == nullptr || recv == nullptr || fn == nullptr) return false;
  if (napi_call_function(env, recv, fn, argc, argv, &local_result) != napi_ok) return false;
  if (result != nullptr) *result = local_result;
  return true;
}

napi_value CreateFrozenNullProtoObject(napi_env env) {
  napi_value object_ctor = GetGlobalNamed(env, "Object");
  if (!IsFunctionValue(env, object_ctor)) return nullptr;

  napi_value create_fn = nullptr;
  if (!GetNamedProperty(env, object_ctor, "create", &create_fn) || !IsFunctionValue(env, create_fn)) {
    return nullptr;
  }
  napi_value freeze_fn = nullptr;
  if (!GetNamedProperty(env, object_ctor, "freeze", &freeze_fn) || !IsFunctionValue(env, freeze_fn)) {
    return nullptr;
  }

  napi_value null_value = nullptr;
  if (napi_get_null(env, &null_value) != napi_ok || null_value == nullptr) return nullptr;

  napi_value object = nullptr;
  napi_value create_argv[1] = {null_value};
  if (!CallFunction(env, object_ctor, create_fn, 1, create_argv, &object) || object == nullptr) {
    return nullptr;
  }

  napi_value freeze_argv[1] = {object};
  napi_value frozen = nullptr;
  if (!CallFunction(env, object_ctor, freeze_fn, 1, freeze_argv, &frozen) || frozen == nullptr) {
    return nullptr;
  }
  return frozen;
}

std::string ValueToUtf8(napi_env env, napi_value value);

napi_value GetSymbolsBindingProperty(napi_env env, const char* property_name) {
  if (env == nullptr || property_name == nullptr) return nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value internal_binding = nullptr;
  if (!GetNamedProperty(env, global, "internalBinding", &internal_binding) ||
      !IsFunctionValue(env, internal_binding)) {
    if (!GetNamedProperty(env, global, "getInternalBinding", &internal_binding) ||
        !IsFunctionValue(env, internal_binding)) {
      return nullptr;
    }
  }

  napi_value symbols_name = nullptr;
  if (napi_create_string_utf8(env, "symbols", NAPI_AUTO_LENGTH, &symbols_name) != napi_ok ||
      symbols_name == nullptr) {
    return nullptr;
  }

  napi_value symbols_binding = nullptr;
  if (!CallFunction(env, global, internal_binding, 1, &symbols_name, &symbols_binding) ||
      symbols_binding == nullptr) {
    return nullptr;
  }

  napi_value out = nullptr;
  if (!GetNamedProperty(env, symbols_binding, property_name, &out)) return nullptr;
  return out;
}

std::string GetFirstSourceLine(std::string source_text) {
  const size_t newline = source_text.find('\n');
  if (newline != std::string::npos) {
    source_text.resize(newline);
  }
  if (!source_text.empty() && source_text.back() == '\r') {
    source_text.pop_back();
  }
  return source_text;
}

void DecorateModuleCompileError(napi_env env,
                                napi_value err,
                                const std::string& resource_name,
                                const std::string& source_text) {
  if (env == nullptr || err == nullptr || resource_name.empty()) return;

  std::string stack_text;
  napi_value stack = nullptr;
  if (napi_get_named_property(env, err, "stack", &stack) == napi_ok && stack != nullptr) {
    stack_text = ValueToUtf8(env, stack);
  }
  if (stack_text.empty()) {
    stack_text = ValueToUtf8(env, err);
  }
  if (stack_text.empty()) return;

  std::string prefix = resource_name + ":1\n";
  const std::string first_line = GetFirstSourceLine(source_text);
  if (!first_line.empty()) {
    prefix += first_line;
    prefix += "\n";
    prefix.append(first_line.size(), ' ');
  }
  prefix += "\n\n";
  if (stack_text.rfind(prefix, 0) == 0) return;

  napi_value decorated_stack = nullptr;
  const std::string full_stack = prefix + stack_text;
  if (napi_create_string_utf8(env,
                              full_stack.c_str(),
                              full_stack.size(),
                              &decorated_stack) == napi_ok &&
      decorated_stack != nullptr) {
    napi_set_named_property(env, err, "stack", decorated_stack);
  }
}

void SetNamedValue(napi_env env, napi_value obj, const char* key, napi_value value) {
  if (obj == nullptr || key == nullptr || value == nullptr) return;
  napi_set_named_property(env, obj, key, value);
}

void SetNamedMethod(napi_env env, napi_value obj, const char* key, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, key, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, key, fn);
  }
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return {};
  napi_value str = nullptr;
  if (napi_coerce_to_string(env, value, &str) != napi_ok || str == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, str, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, str, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

bool ThrowCodeError(napi_env env, const char* code, const char* message) {
  if (env == nullptr || code == nullptr || message == nullptr) return false;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      message_value == nullptr ||
      napi_create_error(env, nullptr, message_value, &error_value) != napi_ok ||
      error_value == nullptr) {
    return false;
  }
  napi_value code_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok || code_value == nullptr) {
    return false;
  }
  if (napi_set_named_property(env, error_value, "code", code_value) != napi_ok) {
    return false;
  }
  return napi_throw(env, error_value) == napi_ok;
}

bool IsNullishValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return true;
  return type == napi_undefined || type == napi_null;
}

// Extracts the bytes of any ArrayBufferView (typed array or DataView) —
// Node's vm cachedData accepts every view flavor over the same buffer.
bool GetArrayBufferViewBytes(napi_env env, napi_value value, const uint8_t** data_out, size_t* len_out) {
  *data_out = nullptr;
  *len_out = 0;
  if (value == nullptr) return false;

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    if (napi_get_typedarray_info(env, value, &type, &length, &data, nullptr, nullptr) != napi_ok ||
        data == nullptr) {
      return false;
    }
    size_t element_size = 1;
    switch (type) {
      case napi_int8_array:
      case napi_uint8_array:
      case napi_uint8_clamped_array:
        element_size = 1;
        break;
      case napi_int16_array:
      case napi_uint16_array:
      case napi_float16_array:
        element_size = 2;
        break;
      case napi_int32_array:
      case napi_uint32_array:
      case napi_float32_array:
        element_size = 4;
        break;
      case napi_float64_array:
      case napi_bigint64_array:
      case napi_biguint64_array:
        element_size = 8;
        break;
      default:
        return false;
    }
    *data_out = static_cast<const uint8_t*>(data);
    *len_out = length * element_size;
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_length = 0;
    void* data = nullptr;
    if (napi_get_dataview_info(env, value, &byte_length, &data, nullptr, nullptr) != napi_ok ||
        data == nullptr) {
      return false;
    }
    *data_out = static_cast<const uint8_t*>(data);
    *len_out = byte_length;
    return true;
  }
  return false;
}

bool IsArrayBufferViewValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) return true;
  bool is_dataview = false;
  return napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview;
}

napi_value ModuleWrapCtor(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  auto* instance = new ModuleWrapInstance();

  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype url_type = napi_undefined;
    if (napi_typeof(env, argv[0], &url_type) == napi_ok && url_type == napi_string) {
      napi_create_reference(env, argv[0], 1, &instance->url_ref);
      SetNamedValue(env, this_arg, "url", argv[0]);
    }
  }

  const bool has_exports_array = argc >= 3 && argv[2] != nullptr;
  bool is_array = false;
  if (has_exports_array && napi_is_array(env, argv[2], &is_array) == napi_ok && is_array) {
    SetNamedBool(env, this_arg, "synthetic", true);
    if (argc >= 4 && argv[3] != nullptr) {
      if (unofficial_napi_module_wrap_create_synthetic(env,
                                                       this_arg,
                                                       argc >= 1 ? argv[0] : nullptr,
                                                       argc >= 2 ? argv[1] : nullptr,
                                                       argv[2],
                                                       argv[3],
                                                       &instance->module_handle) != napi_ok ||
          instance->module_handle == nullptr) {
        delete instance;
        return nullptr;
      }
    }
    if (argc >= 5 && argv[4] != nullptr) {
      napi_value imported_cjs_symbol = GetSymbolsBindingProperty(env, "imported_cjs_symbol");
      if (imported_cjs_symbol != nullptr) {
        (void)napi_set_property(env, this_arg, imported_cjs_symbol, argv[4]);
      }
    }
  } else if (has_exports_array) {
    SetNamedBool(env, this_arg, "synthetic", false);
    napi_valuetype source_type = napi_undefined;
    if (napi_typeof(env, argv[2], &source_type) == napi_ok && source_type == napi_string) {
      int32_t line_offset = 0;
      int32_t column_offset = 0;
      if (argc >= 4 && argv[3] != nullptr) (void)napi_get_value_int32(env, argv[3], &line_offset);
      if (argc >= 5 && argv[4] != nullptr) (void)napi_get_value_int32(env, argv[4], &column_offset);

      // arg5 is dual-purpose at the JS boundary: the ESM loader passes its
      // host-defined-option Symbol; vm.SourceTextModule passes user cachedData
      // (an ArrayBufferView).
      napi_value arg5 = argc >= 6 ? argv[5] : nullptr;
      const bool arg5_is_cached_data = IsArrayBufferViewValue(env, arg5);
      napi_value host_defined_option = arg5_is_cached_data ? nullptr : arg5;

      void* module_bytecode = nullptr;
      if (arg5_is_cached_data) {
        // vm.SourceTextModule cachedData: validate and consume. The provider
        // validates source/shape/integrity itself (V8 CachedData / QuickJS
        // QJSB header) and reports a mismatch via rejected_out. Node throws
        // ERR_VM_MODULE_CACHED_DATA_REJECTED from the constructor on reject.
        bool rejected = true;
        const uint8_t* data = nullptr;
        size_t length = 0;
        if (GetArrayBufferViewBytes(env, arg5, &data, &length) && length > 0) {
          bool deserialize_rejected = false;
          if (unofficial_napi_bytecode_deserialize(env,
                                                   data,
                                                   length,
                                                   argv[2],
                                                   argc >= 1 ? argv[0] : nullptr,
                                                   unofficial_napi_bytecode_shape_module,
                                                   nullptr,
                                                   nullptr,
                                                   &module_bytecode,
                                                   &deserialize_rejected) == napi_ok) {
            rejected = deserialize_rejected || module_bytecode == nullptr;
          }
        }
        SetNamedBool(env, this_arg, "cachedDataRejected", rejected);
        if (rejected) {
          ThrowCodeError(env, "ERR_VM_MODULE_CACHED_DATA_REJECTED",
                         "cachedData buffer was rejected");
          delete instance;
          return nullptr;
        }
      } else if (edge_bytecode_cache::Enabled() && (argc < 2 || IsNullishValue(env, argv[1])) &&
                 line_offset == 0 && column_offset == 0) {
        // Bytecode sidecar cache for file-backed modules loaded by the default
        // ESM loader (a vm context in argv[1] opts out). Deserialize
        // <path>.v8b/.qjsb when it matches the exact source, otherwise compile
        // to a fresh handle and persist it (write-on-first-run). Non-zero
        // line/column offsets are baked into the payload but absent from the
        // sidecar key, so they are excluded here — otherwise a
        // `new vm.SourceTextModule(src, {identifier:'file://…', lineOffset:N})`
        // would poison the real loader's sidecar with shifted positions.
        const std::string url_utf8 = argc >= 1 && argv[0] != nullptr ? ValueToUtf8(env, argv[0]) : "";
        if (url_utf8.rfind("file://", 0) == 0) {
          const std::string sidecar_source_path = edge_path::NormalizeFileURLOrPath(url_utf8);
          std::error_code ec;
          if (!sidecar_source_path.empty() &&
              std::filesystem::is_regular_file(std::filesystem::path(sidecar_source_path), ec) && !ec) {
            const std::string sidecar_source_utf8 = ValueToUtf8(env, argv[2]);
            edge_bytecode_cache::SidecarPayload payload;
            if (edge_bytecode_cache::ReadSidecar(sidecar_source_path, sidecar_source_utf8,
                                                 edge_bytecode_cache::kFlagEsmModuleV1, &payload)) {
              bool rejected = false;
              if (unofficial_napi_bytecode_deserialize(env,
                                                       payload.data(),
                                                       payload.payload_size,
                                                       argv[2],
                                                       argv[0],
                                                       unofficial_napi_bytecode_shape_module,
                                                       nullptr,
                                                       host_defined_option,
                                                       &module_bytecode,
                                                       &rejected) != napi_ok ||
                  rejected || module_bytecode == nullptr) {
                edge_bytecode_cache::RemoveSidecar(sidecar_source_path);
                module_bytecode = nullptr;
              }
            }
            if (module_bytecode == nullptr) {
              if (unofficial_napi_bytecode_compile(env,
                                                   argv[2],
                                                   argv[0],
                                                   unofficial_napi_bytecode_shape_module,
                                                   nullptr,
                                                   host_defined_option,
                                                   line_offset,
                                                   column_offset,
                                                   &module_bytecode,
                                                   nullptr) == napi_ok &&
                  module_bytecode != nullptr) {
                napi_value cache_buffer = nullptr;
                if (unofficial_napi_bytecode_serialize(env, module_bytecode, &cache_buffer) == napi_ok &&
                    cache_buffer != nullptr) {
                  napi_typedarray_type type = napi_uint8_array;
                  size_t length = 0;
                  void* data = nullptr;
                  if (napi_get_typedarray_info(env, cache_buffer, &type, &length, &data, nullptr, nullptr) ==
                          napi_ok &&
                      type == napi_uint8_array && data != nullptr && length > 0) {
                    edge_bytecode_cache::WriteSidecar(sidecar_source_path,
                                                      sidecar_source_utf8,
                                                      edge_bytecode_cache::kFlagEsmModuleV1,
                                                      static_cast<const uint8_t*>(data),
                                                      length);
                  }
                }
              } else {
                // Compile failed (syntax error): clear it and let the text
                // path below regenerate it with the existing error decoration.
                bool has_pending = false;
                napi_value ignored = nullptr;
                if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
                  (void)napi_get_and_clear_last_exception(env, &ignored);
                }
                module_bytecode = nullptr;
              }
            }
          }
        }
      }

      const unofficial_napi_js_source module_source{module_bytecode != nullptr ? nullptr : argv[2],
                                                    module_bytecode};
      const napi_status create_status =
          unofficial_napi_module_wrap_create_source_text(env,
                                                         this_arg,
                                                         argc >= 1 ? argv[0] : nullptr,
                                                         argc >= 2 ? argv[1] : nullptr,
                                                         &module_source,
                                                         line_offset,
                                                         column_offset,
                                                         host_defined_option,
                                                         &instance->module_handle);
      if (module_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, module_bytecode);
      if (create_status != napi_ok || instance->module_handle == nullptr) {
        napi_value err = nullptr;
        if (napi_get_and_clear_last_exception(env, &err) == napi_ok && err != nullptr) {
          const std::string resource_name =
              argc >= 1 && argv[0] != nullptr ? ValueToUtf8(env, argv[0]) : "";
          const std::string source_text = argc >= 3 && argv[2] != nullptr ? ValueToUtf8(env, argv[2]) : "";
          DecorateModuleCompileError(env, err, resource_name, source_text);
          napi_throw(env, err);
        }
        delete instance;
        return nullptr;
      }
      if (instance->module_handle != nullptr) {
        bool has_tla = false;
        if (unofficial_napi_module_wrap_has_top_level_await(env, instance->module_handle, &has_tla) == napi_ok) {
          instance->has_top_level_await = has_tla;
        }
      }
    }
  }

  SetNamedBool(env, this_arg, "hasTopLevelAwait", instance->has_top_level_await);

  napi_wrap(env, this_arg, instance, ModuleWrapFinalize, nullptr, &instance->wrapper_ref);
  return this_arg;
}

napi_value ModuleWrapLink(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr) return Undefined(env);
  if (instance->module_handle == nullptr) return Undefined(env);

  bool is_array = false;
  if (argc < 1 || argv[0] == nullptr || napi_is_array(env, argv[0], &is_array) != napi_ok || !is_array) {
    napi_throw_error(env, "ERR_INVALID_ARG_TYPE", "link() expects an array");
    return nullptr;
  }
  uint32_t length = 0;
  napi_get_array_length(env, argv[0], &length);
  std::vector<void*> linked_handles(length, nullptr);
  for (uint32_t i = 0; i < length; ++i) {
    napi_value module_value = nullptr;
    if (napi_get_element(env, argv[0], i, &module_value) != napi_ok || module_value == nullptr) {
      napi_throw_error(env, "ERR_VM_MODULE_LINK_FAILURE", "linked module missing");
      return nullptr;
    }
    ModuleWrapInstance* linked = UnwrapModuleWrap(env, module_value);
    linked_handles[i] = linked != nullptr ? linked->module_handle : nullptr;
  }
  if (unofficial_napi_module_wrap_link(env, instance->module_handle, length, linked_handles.data()) != napi_ok) {
    return nullptr;
  }
  return Undefined(env);
}

napi_value ModuleWrapGetModuleRequests(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance != nullptr && instance->module_handle != nullptr) {
    napi_value out = nullptr;
    if (unofficial_napi_module_wrap_get_module_requests(env, instance->module_handle, &out) == napi_ok &&
        out != nullptr) {
      return out;
    }
    return Undefined(env);
  }
  napi_value out = nullptr;
  napi_create_array_with_length(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapInstantiate(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr || instance->module_handle == nullptr) return Undefined(env);
  if (unofficial_napi_module_wrap_instantiate(env, instance->module_handle) != napi_ok) {
    return nullptr;
  }
  return Undefined(env);
}

napi_value ModuleWrapInstantiateSync(napi_env env, napi_callback_info info) {
  return ModuleWrapInstantiate(env, info);
}

napi_value ModuleWrapEvaluateSync(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr) return Undefined(env);
  if (instance->module_handle == nullptr) return Undefined(env);

  napi_value out = nullptr;
  if (unofficial_napi_module_wrap_evaluate_sync(
          env,
          instance->module_handle,
          argc >= 1 ? argv[0] : nullptr,
          argc >= 2 ? argv[1] : nullptr,
          &out) != napi_ok) {
    return nullptr;
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapEvaluate(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr) return Undefined(env);
  if (instance->module_handle == nullptr) return Undefined(env);

  int64_t timeout = -1;
  bool break_on_sigint = false;
  if (argc >= 1 && argv[0] != nullptr) (void)napi_get_value_int64(env, argv[0], &timeout);
  if (argc >= 2 && argv[1] != nullptr) (void)napi_get_value_bool(env, argv[1], &break_on_sigint);
  napi_value out = nullptr;
  if (unofficial_napi_module_wrap_evaluate(env, instance->module_handle, timeout, break_on_sigint, &out) != napi_ok) {
    return nullptr;
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapSetExport(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr || argc < 1 || argv[0] == nullptr) return Undefined(env);
  if (instance->module_handle == nullptr) return Undefined(env);
  if (unofficial_napi_module_wrap_set_export(
          env, instance->module_handle, argv[0], argc >= 2 ? argv[1] : Undefined(env)) != napi_ok) {
    return nullptr;
  }
  return Undefined(env);
}

napi_value ModuleWrapSetModuleSourceObject(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr || instance->module_handle == nullptr) return Undefined(env);
  if (unofficial_napi_module_wrap_set_module_source_object(
          env, instance->module_handle, argc >= 1 ? argv[0] : nullptr) != napi_ok) {
    return nullptr;
  }
  return Undefined(env);
}

napi_value ModuleWrapGetModuleSourceObject(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr || instance->module_handle == nullptr) return Undefined(env);
  napi_value out = nullptr;
  if (unofficial_napi_module_wrap_get_module_source_object(env, instance->module_handle, &out) == napi_ok &&
      out != nullptr) {
    return out;
  }
  const std::string message =
      "Source phase import object is not defined for module '" +
      ValueToUtf8(env, GetRefValue(env, instance->url_ref)) + "'";
  ThrowCodeError(env, "ERR_SOURCE_PHASE_NOT_DEFINED", message.c_str());
  return nullptr;
}

napi_value ModuleWrapCreateCachedData(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance != nullptr && instance->module_handle != nullptr) {
    napi_value out = nullptr;
    if (unofficial_napi_module_wrap_create_cached_data(env, instance->module_handle, &out) == napi_ok &&
        out != nullptr) {
      // The provider's bytes self-validate (V8 CachedData / QuickJS QJSB
      // header, which now carries the module source hash); just hand back a
      // node Buffer. vm.SourceTextModule#createCachedData() promises a Buffer;
      // the QuickJS provider returns a plain Uint8Array.
      napi_value global = nullptr;
      napi_value buffer_ctor = nullptr;
      napi_value from_fn = nullptr;
      napi_value wrapped = nullptr;
      bool is_buffer = false;
      if (napi_get_global(env, &global) == napi_ok && global != nullptr &&
          napi_get_named_property(env, global, "Buffer", &buffer_ctor) == napi_ok &&
          buffer_ctor != nullptr &&
          napi_instanceof(env, out, buffer_ctor, &is_buffer) == napi_ok && !is_buffer &&
          napi_get_named_property(env, buffer_ctor, "from", &from_fn) == napi_ok &&
          from_fn != nullptr &&
          napi_call_function(env, buffer_ctor, from_fn, 1, &out, &wrapped) == napi_ok &&
          wrapped != nullptr) {
        return wrapped;
      }
      return out;
    }
  }
  napi_value arraybuffer = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, 0, &data, &arraybuffer) != napi_ok || arraybuffer == nullptr) {
    return Undefined(env);
  }
  napi_value typed_array = nullptr;
  if (napi_create_typedarray(env, napi_uint8_array, 0, arraybuffer, 0, &typed_array) != napi_ok ||
      typed_array == nullptr) {
    return Undefined(env);
  }
  return typed_array;
}

napi_value ModuleWrapGetNamespace(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr) return Undefined(env);
  if (instance->module_handle != nullptr) {
    napi_value out = nullptr;
    if (unofficial_napi_module_wrap_get_namespace(env, instance->module_handle, &out) == napi_ok && out != nullptr) {
      return out;
    }
  }
  return Undefined(env);
}

napi_value ModuleWrapGetStatus(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  int32_t status = kUninstantiated;
  if (instance != nullptr && instance->module_handle != nullptr) {
    (void)unofficial_napi_module_wrap_get_status(env, instance->module_handle, &status);
  }
  napi_value out = nullptr;
  napi_create_int32(env, status, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapGetError(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  if (instance == nullptr) return Undefined(env);
  if (instance->module_handle != nullptr) {
    napi_value out = nullptr;
    if (unofficial_napi_module_wrap_get_error(env, instance->module_handle, &out) == napi_ok && out != nullptr) {
      return out;
    }
  }
  return Undefined(env);
}

napi_value ModuleWrapHasTopLevelAwait(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  bool value = instance != nullptr && instance->has_top_level_await;
  if (instance != nullptr && instance->module_handle != nullptr) {
    (void)unofficial_napi_module_wrap_has_top_level_await(env, instance->module_handle, &value);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapHasAsyncGraph(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, this_arg);
  bool value = false;
  if (instance != nullptr && instance->module_handle != nullptr) {
    if (unofficial_napi_module_wrap_has_async_graph(env, instance->module_handle, &value) != napi_ok) {
      return nullptr;
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapSetImportModuleDynamicallyCallback(napi_env env, napi_callback_info info) {
  auto* state = GetBindingState(env);
  if (state == nullptr) return Undefined(env);
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  SetRef(env, &state->import_module_dynamically_ref, argc >= 1 ? argv[0] : nullptr, napi_function);
  (void)unofficial_napi_module_wrap_set_import_module_dynamically_callback(env, argc >= 1 ? argv[0] : nullptr);
  return Undefined(env);
}

napi_value ModuleWrapSetInitializeImportMetaObjectCallback(napi_env env, napi_callback_info info) {
  auto* state = GetBindingState(env);
  if (state == nullptr) return Undefined(env);
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  SetRef(env, &state->initialize_import_meta_ref, argc >= 1 ? argv[0] : nullptr, napi_function);
  (void)unofficial_napi_module_wrap_set_initialize_import_meta_object_callback(env, argc >= 1 ? argv[0] : nullptr);
  return Undefined(env);
}

napi_value ModuleWrapImportModuleDynamically(napi_env env, napi_callback_info info) {
  auto* state = GetBindingState(env);
  if (state == nullptr) return Undefined(env);
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);

  napi_value callback = GetRefValue(env, state->import_module_dynamically_ref);
  if (!IsFunctionValue(env, callback)) return Undefined(env);

  napi_value global = GetGlobal(env);
  if (global == nullptr) return Undefined(env);

  napi_value result = nullptr;
  if (argc >= 5) {
    if (!CallFunction(env, global, callback, 5, argv, &result)) return nullptr;
    return result != nullptr ? result : Undefined(env);
  }

  napi_value referrer_symbol = GetSymbolsBindingProperty(env, "vm_dynamic_import_default_internal");
  if (referrer_symbol == nullptr) return nullptr;
  napi_value phase = nullptr;
  if (napi_create_int32(env, kEvaluationPhase, &phase) != napi_ok || phase == nullptr) return nullptr;
  napi_value attrs = CreateFrozenNullProtoObject(env);
  if (attrs == nullptr) return nullptr;
  napi_value referrer_name = argc >= 2 ? argv[1] : nullptr;
  napi_value call_argv[5] = {referrer_symbol, argv[0], phase, attrs, referrer_name};
  if (!CallFunction(env, global, callback, 5, call_argv, &result)) return nullptr;
  return result != nullptr ? result : Undefined(env);
}

napi_value ModuleWrapCreateRequiredModuleFacade(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  ModuleWrapInstance* instance = UnwrapModuleWrap(env, argv[0]);
  if (instance == nullptr || instance->module_handle == nullptr) return argv[0];
  napi_value out = nullptr;
  if (unofficial_napi_module_wrap_create_required_module_facade(env, instance->module_handle, &out) != napi_ok) {
    return nullptr;
  }
  return out != nullptr ? out : Undefined(env);
}

napi_value ModuleWrapThrowIfPromiseRejected(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return Undefined(env);
  }

  int32_t state = 0;
  napi_value result = nullptr;
  bool has_result = false;
  if (unofficial_napi_get_promise_details(env, argv[0], &state, &result, &has_result) != napi_ok) {
    return nullptr;
  }
  // Mirror Node's native helper: only rejected promises are rethrown.
  if (state == 2 && has_result && result != nullptr) {
    if (unofficial_napi_mark_promise_as_handled(env, argv[0]) != napi_ok) {
      return nullptr;
    }
    napi_throw(env, result);
    return nullptr;
  }
  return Undefined(env);
}

}  // namespace

napi_value InitModuleWrap(napi_env env) {
  auto& state = EnsureBindingState(env);
  if (state.binding_ref != nullptr) {
    napi_value existing = GetRefValue(env, state.binding_ref);
    if (existing != nullptr) return existing;
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  SetNamedInt(env, binding, "kUninstantiated", kUninstantiated);
  SetNamedInt(env, binding, "kInstantiating", kInstantiating);
  SetNamedInt(env, binding, "kInstantiated", kInstantiated);
  SetNamedInt(env, binding, "kEvaluating", kEvaluating);
  SetNamedInt(env, binding, "kEvaluated", kEvaluated);
  SetNamedInt(env, binding, "kErrored", kErrored);
  SetNamedInt(env, binding, "kSourcePhase", kSourcePhase);
  SetNamedInt(env, binding, "kEvaluationPhase", kEvaluationPhase);

  napi_property_descriptor proto[] = {
      {"link", nullptr, ModuleWrapLink, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getModuleRequests", nullptr, ModuleWrapGetModuleRequests, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"instantiate", nullptr, ModuleWrapInstantiate, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"evaluate", nullptr, ModuleWrapEvaluate, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"evaluateSync", nullptr, ModuleWrapEvaluateSync, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"setExport", nullptr, ModuleWrapSetExport, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"setModuleSourceObject", nullptr, ModuleWrapSetModuleSourceObject, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"getModuleSourceObject", nullptr, ModuleWrapGetModuleSourceObject, nullptr, nullptr, nullptr, napi_default,
       nullptr},
      {"createCachedData", nullptr, ModuleWrapCreateCachedData, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getNamespace", nullptr, ModuleWrapGetNamespace, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getStatus", nullptr, ModuleWrapGetStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getError", nullptr, ModuleWrapGetError, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"hasAsyncGraph", nullptr, nullptr, ModuleWrapHasAsyncGraph, nullptr, nullptr, napi_default, nullptr},
  };

  napi_value module_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "ModuleWrap",
                        NAPI_AUTO_LENGTH,
                        ModuleWrapCtor,
                        nullptr,
                        sizeof(proto) / sizeof(proto[0]),
                        proto,
                        &module_wrap_ctor) != napi_ok ||
      module_wrap_ctor == nullptr) {
    return Undefined(env);
  }
  napi_set_named_property(env, binding, "ModuleWrap", module_wrap_ctor);
  napi_create_reference(env, module_wrap_ctor, 1, &state.module_wrap_ctor_ref);

  SetNamedMethod(env, binding, "setImportModuleDynamicallyCallback", ModuleWrapSetImportModuleDynamicallyCallback);
  SetNamedMethod(
      env, binding, "setInitializeImportMetaObjectCallback", ModuleWrapSetInitializeImportMetaObjectCallback);
  SetNamedMethod(env, binding, "importModuleDynamically", ModuleWrapImportModuleDynamically);
  SetNamedMethod(env, binding, "createRequiredModuleFacade", ModuleWrapCreateRequiredModuleFacade);
  SetNamedMethod(env, binding, "throwIfPromiseRejected", ModuleWrapThrowIfPromiseRejected);

  napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}

}  // namespace internal_binding
