#include "webassembly/edge_wasm.h"

#include "edge_environment.h"
#include "internal_binding/helpers.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <vector>

#if defined(EDGE_QUICKJS_WEBASSEMBLY_IMPORTS) && !defined(WASM_API_EXTERN)
#define WASM_API_EXTERN __attribute__((import_module("wasm_c_api_v0")))
#endif

#include <wasm.h>

namespace {

constexpr uint32_t kInternalCreateMagic = 0x45574153; // EWAS

enum class WasmObjectKind : uint32_t {
  kModule = 1,
  kInstance,
  kMemory,
  kTable,
  kGlobal,
  kFunction,
};

struct WasmState;

struct WasmObjectBase {
  WasmObjectKind kind;
  WasmState *state;
};

struct WasmModuleObject {
  WasmObjectBase base;
  wasm_module_t *module = nullptr;
};

struct WasmInstanceObject {
  WasmObjectBase base;
  wasm_instance_t *instance = nullptr;
  napi_ref exports_ref = nullptr;
};

struct WasmMemoryObject {
  WasmObjectBase base;
  wasm_memory_t *memory = nullptr;
};

struct WasmTableObject {
  WasmObjectBase base;
  wasm_table_t *table = nullptr;
  bool local_only = false;
  uint32_t local_size = 0;
  uint32_t local_max = wasm_limits_max_default;
  wasm_valkind_t local_element_kind = WASM_FUNCREF;
};

struct WasmGlobalObject {
  WasmObjectBase base;
  wasm_global_t *global = nullptr;
};

struct WasmFunctionObject {
  WasmObjectBase base;
  wasm_func_t *func = nullptr;
};

struct InternalCreate {
  uint32_t magic = kInternalCreateMagic;
  WasmObjectKind kind = WasmObjectKind::kModule;
  void *ptr = nullptr;
};

void DeleteRefIfPresent(napi_env env, napi_ref *ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr)
    return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

struct WasmState {
  explicit WasmState(napi_env env_in) : env(env_in) {}

  ~WasmState() {
    DeleteRefIfPresent(env, &pending_import_exception_ref);
    DeleteRefIfPresent(env, &webassembly_ref);
    DeleteRefIfPresent(env, &module_ctor_ref);
    DeleteRefIfPresent(env, &instance_ctor_ref);
    DeleteRefIfPresent(env, &memory_ctor_ref);
    DeleteRefIfPresent(env, &table_ctor_ref);
    DeleteRefIfPresent(env, &global_ctor_ref);
    if (store != nullptr)
      wasm_store_delete(store);
    if (engine != nullptr)
      wasm_engine_delete(engine);
  }

  bool Initialize(std::string *error_out) {
    if (engine != nullptr && store != nullptr)
      return true;
    engine = wasm_engine_new();
    if (engine == nullptr) {
      if (error_out != nullptr)
        *error_out = "wasm_engine_new failed";
      return false;
    }
    store = wasm_store_new(engine);
    if (store == nullptr) {
      if (error_out != nullptr)
        *error_out = "wasm_store_new failed";
      return false;
    }
    return true;
  }

  napi_env env = nullptr;
  wasm_engine_t *engine = nullptr;
  wasm_store_t *store = nullptr;
  napi_ref webassembly_ref = nullptr;
  napi_ref module_ctor_ref = nullptr;
  napi_ref instance_ctor_ref = nullptr;
  napi_ref memory_ctor_ref = nullptr;
  napi_ref table_ctor_ref = nullptr;
  napi_ref global_ctor_ref = nullptr;
  napi_ref pending_import_exception_ref = nullptr;
};

struct ImportFuncData {
  WasmState *state = nullptr;
  napi_env env = nullptr;
  napi_ref function_ref = nullptr;
  std::vector<wasm_valkind_t> result_kinds;
};

napi_value Undefined(napi_env env) { return internal_binding::Undefined(env); }

napi_value Null(napi_env env) {
  napi_value value = nullptr;
  napi_get_null(env, &value);
  return value;
}

bool IsUndefined(napi_env env, napi_value value) {
  return value == nullptr || internal_binding::IsUndefined(env, value);
}

bool IsNullOrUndefined(napi_env env, napi_value value) {
  if (IsUndefined(env, value))
    return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_null;
}

std::string GetString(napi_env env, napi_value value) {
  if (value == nullptr)
    return std::string();
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
    return std::string();
  }
  std::vector<char> buffer(length + 1);
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, buffer.data(), buffer.size(),
                                 &copied) != napi_ok) {
    return std::string();
  }
  return std::string(buffer.data(), copied);
}

std::string NameToString(const wasm_name_t *name) {
  if (name == nullptr || name->data == nullptr || name->size == 0)
    return std::string();
  size_t length = name->size;
  if (length > 0 && name->data[length - 1] == '\0')
    --length;
  return std::string(name->data, name->data + length);
}

napi_value MakeString(napi_env env, const std::string &value) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value.data(), value.size(), &out) !=
      napi_ok) {
    return Undefined(env);
  }
  return out;
}

napi_value MakeString(napi_env env, const char *value) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &out) != napi_ok) {
    return Undefined(env);
  }
  return out;
}

bool GetNamed(napi_env env, napi_value object, const char *name,
              napi_value *out) {
  if (out == nullptr)
    return false;
  *out = nullptr;
  if (object == nullptr || name == nullptr)
    return false;
  bool has = false;
  if (napi_has_named_property(env, object, name, &has) != napi_ok || !has) {
    *out = Undefined(env);
    return true;
  }
  return napi_get_named_property(env, object, name, out) == napi_ok &&
         *out != nullptr;
}

bool SetNamed(napi_env env, napi_value object, const char *name,
              napi_value value) {
  return object != nullptr && name != nullptr && value != nullptr &&
         napi_set_named_property(env, object, name, value) == napi_ok;
}

bool SetNamedString(napi_env env, napi_value object, const char *name,
                    const char *value) {
  return SetNamed(env, object, name, MakeString(env, value));
}

bool SetNamedString(napi_env env, napi_value object, const char *name,
                    const std::string &value) {
  return SetNamed(env, object, name, MakeString(env, value));
}

bool SetNamedUint32(napi_env env, napi_value object, const char *name,
                    uint32_t value) {
  napi_value js_value = nullptr;
  if (napi_create_uint32(env, value, &js_value) != napi_ok ||
      js_value == nullptr)
    return false;
  return SetNamed(env, object, name, js_value);
}

bool GetRequiredUint32(napi_env env, napi_value object, const char *name,
                       uint32_t *out) {
  napi_value value = nullptr;
  if (!GetNamed(env, object, name, &value) || IsUndefined(env, value))
    return false;
  return napi_get_value_uint32(env, value, out) == napi_ok;
}

bool GetOptionalUint32(napi_env env, napi_value object, const char *name,
                       uint32_t fallback, uint32_t *out) {
  napi_value value = nullptr;
  if (!GetNamed(env, object, name, &value) || IsUndefined(env, value)) {
    *out = fallback;
    return true;
  }
  return napi_get_value_uint32(env, value, out) == napi_ok;
}

bool GetOptionalBool(napi_env env, napi_value object, const char *name,
                     bool fallback, bool *out) {
  napi_value value = nullptr;
  if (!GetNamed(env, object, name, &value) || IsUndefined(env, value)) {
    *out = fallback;
    return true;
  }
  return napi_get_value_bool(env, value, out) == napi_ok;
}

napi_value CreateErrorObject(napi_env env, const char *name,
                             const std::string &message) {
  napi_value message_value = MakeString(env, message);
  napi_value error = nullptr;
  napi_value global = nullptr;
  napi_value webassembly = nullptr;
  napi_value ctor = nullptr;
  bool used_wasm_ctor = false;
  if (napi_get_global(env, &global) == napi_ok &&
      GetNamed(env, global, "WebAssembly", &webassembly) &&
      !IsUndefined(env, webassembly) &&
      GetNamed(env, webassembly, name, &ctor) && !IsUndefined(env, ctor)) {
    napi_valuetype ctor_type = napi_undefined;
    if (napi_typeof(env, ctor, &ctor_type) == napi_ok &&
        ctor_type == napi_function) {
      napi_value argv[1] = {message_value};
      used_wasm_ctor =
          napi_new_instance(env, ctor, 1, argv, &error) == napi_ok &&
          error != nullptr;
    }
  }
  if (!used_wasm_ctor &&
      napi_create_error(env, nullptr, message_value, &error) != napi_ok) {
    return nullptr;
  }
  if (error != nullptr) {
    SetNamedString(env, error, "name", name);
  }
  return error;
}

void ThrowWasmError(napi_env env, const char *name,
                    const std::string &message) {
  napi_value error = CreateErrorObject(env, name, message);
  if (error != nullptr) {
    napi_throw(env, error);
  } else {
    napi_throw_error(env, nullptr, message.c_str());
  }
}

void RejectDeferredWithWasmError(napi_env env, napi_deferred deferred,
                                 const char *name, const std::string &message) {
  napi_value error = CreateErrorObject(env, name, message);
  if (error == nullptr) {
    napi_create_error(env, nullptr, MakeString(env, message), &error);
  }
  napi_reject_deferred(env, deferred, error);
}

bool ReadBufferSource(napi_env env, napi_value value, const wasm_byte_t **data,
                      size_t *length) {
  if (data == nullptr || length == nullptr)
    return false;
  *data = nullptr;
  *length = 0;

  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) == napi_ok &&
      is_typed_array) {
    napi_typedarray_type type = napi_uint8_array;
    size_t element_count = 0;
    void *raw = nullptr;
    if (napi_get_typedarray_info(env, value, &type, &element_count, &raw,
                                 nullptr, nullptr) != napi_ok) {
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
    }
    *data = static_cast<const wasm_byte_t *>(raw);
    *length = element_count * element_size;
    return true;
  }

  bool is_data_view = false;
  if (napi_is_dataview(env, value, &is_data_view) == napi_ok && is_data_view) {
    void *raw = nullptr;
    if (napi_get_dataview_info(env, value, length, &raw, nullptr, nullptr) !=
        napi_ok) {
      return false;
    }
    *data = static_cast<const wasm_byte_t *>(raw);
    return true;
  }

  bool is_array_buffer = false;
  if (napi_is_arraybuffer(env, value, &is_array_buffer) == napi_ok &&
      is_array_buffer) {
    void *raw = nullptr;
    if (napi_get_arraybuffer_info(env, value, &raw, length) != napi_ok) {
      return false;
    }
    *data = static_cast<const wasm_byte_t *>(raw);
    return true;
  }

  return false;
}

wasm_byte_vec_t BorrowedByteVec(const wasm_byte_t *data, size_t length) {
  wasm_byte_vec_t bytes;
  bytes.size = length;
  bytes.data = const_cast<wasm_byte_t *>(data);
  return bytes;
}

WasmState *GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<WasmState>(
      env, kEdgeEnvironmentSlotQuickJsWebAssemblyState);
}

WasmState *EnsureState(napi_env env, std::string *error_out) {
  if (auto *existing = GetState(env); existing != nullptr)
    return existing;
  auto *state = new WasmState(env);
  if (!state->Initialize(error_out)) {
    delete state;
    return nullptr;
  }
  EdgeEnvironmentSetOpaqueSlot(
      env, kEdgeEnvironmentSlotQuickJsWebAssemblyState, state,
      [](void *data) { delete static_cast<WasmState *>(data); });
  return state;
}

template <typename T>
T *Unwrap(napi_env env, napi_value value, WasmObjectKind kind) {
  if (value == nullptr)
    return nullptr;
  void *data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok || data == nullptr)
    return nullptr;
  auto *base = static_cast<WasmObjectBase *>(data);
  if (base->kind != kind)
    return nullptr;
  return static_cast<T *>(data);
}

bool GetCallback(napi_env env, napi_callback_info info, size_t *argc,
                 napi_value *argv, napi_value *this_arg, void **data) {
  return napi_get_cb_info(env, info, argc, argv, this_arg, data) == napi_ok;
}

bool TryConsumeInternalCreate(napi_env env, napi_value value,
                              WasmObjectKind expected_kind, void **out) {
  if (out == nullptr)
    return false;
  *out = nullptr;
  void *external = nullptr;
  if (value == nullptr ||
      napi_get_value_external(env, value, &external) != napi_ok ||
      external == nullptr) {
    return false;
  }
  auto *init = static_cast<InternalCreate *>(external);
  if (init->magic != kInternalCreateMagic || init->kind != expected_kind ||
      init->ptr == nullptr) {
    return false;
  }
  *out = init->ptr;
  init->ptr = nullptr;
  return true;
}

napi_value GetConstructor(WasmState *state, WasmObjectKind kind) {
  napi_ref ref = nullptr;
  switch (kind) {
  case WasmObjectKind::kModule:
    ref = state->module_ctor_ref;
    break;
  case WasmObjectKind::kInstance:
    ref = state->instance_ctor_ref;
    break;
  case WasmObjectKind::kMemory:
    ref = state->memory_ctor_ref;
    break;
  case WasmObjectKind::kTable:
    ref = state->table_ctor_ref;
    break;
  case WasmObjectKind::kGlobal:
    ref = state->global_ctor_ref;
    break;
  case WasmObjectKind::kFunction:
    break;
  }
  napi_value ctor = nullptr;
  if (ref != nullptr)
    napi_get_reference_value(state->env, ref, &ctor);
  return ctor;
}

void DeleteOwnedByKind(WasmObjectKind kind, void *ptr) {
  if (ptr == nullptr)
    return;
  switch (kind) {
  case WasmObjectKind::kModule:
    wasm_module_delete(static_cast<wasm_module_t *>(ptr));
    break;
  case WasmObjectKind::kInstance:
    wasm_instance_delete(static_cast<wasm_instance_t *>(ptr));
    break;
  case WasmObjectKind::kMemory:
    wasm_memory_delete(static_cast<wasm_memory_t *>(ptr));
    break;
  case WasmObjectKind::kTable:
    wasm_table_delete(static_cast<wasm_table_t *>(ptr));
    break;
  case WasmObjectKind::kGlobal:
    wasm_global_delete(static_cast<wasm_global_t *>(ptr));
    break;
  case WasmObjectKind::kFunction:
    wasm_func_delete(static_cast<wasm_func_t *>(ptr));
    break;
  }
}

napi_value CreateObjectFromOwned(WasmState *state, WasmObjectKind kind,
                                 void *ptr) {
  if (state == nullptr || ptr == nullptr)
    return nullptr;
  napi_env env = state->env;
  napi_value ctor = GetConstructor(state, kind);
  if (ctor == nullptr)
    return nullptr;

  InternalCreate init;
  init.kind = kind;
  init.ptr = ptr;

  napi_value external = nullptr;
  if (napi_create_external(env, &init, nullptr, nullptr, &external) !=
          napi_ok ||
      external == nullptr) {
    return nullptr;
  }
  napi_value argv[1] = {external};
  napi_value object = nullptr;
  napi_status status = napi_new_instance(env, ctor, 1, argv, &object);
  if (init.ptr != nullptr) {
    DeleteOwnedByKind(kind, init.ptr);
    init.ptr = nullptr;
  }
  return status == napi_ok ? object : nullptr;
}

const char *ExternKindName(wasm_externkind_t kind) {
  switch (kind) {
  case WASM_EXTERN_FUNC:
    return "function";
  case WASM_EXTERN_GLOBAL:
    return "global";
  case WASM_EXTERN_TABLE:
    return "table";
  case WASM_EXTERN_MEMORY:
    return "memory";
  default:
    return "unknown";
  }
}

bool ParseValueKind(napi_env env, napi_value value, wasm_valkind_t *out) {
  if (out == nullptr || value == nullptr)
    return false;
  std::string kind = GetString(env, value);
  if (kind == "i32") {
    *out = WASM_I32;
    return true;
  }
  if (kind == "i64") {
    *out = WASM_I64;
    return true;
  }
  if (kind == "f32") {
    *out = WASM_F32;
    return true;
  }
  if (kind == "f64") {
    *out = WASM_F64;
    return true;
  }
  if (kind == "externref") {
    *out = WASM_EXTERNREF;
    return true;
  }
  if (kind == "funcref" || kind == "anyfunc") {
    *out = WASM_FUNCREF;
    return true;
  }
  return false;
}

bool JsToWasmVal(napi_env env, napi_value value, wasm_valkind_t kind,
                 wasm_val_t *out) {
  if (out == nullptr)
    return false;
  out->kind = kind;
  switch (kind) {
  case WASM_I32: {
    int32_t number = 0;
    if (napi_get_value_int32(env, value, &number) != napi_ok)
      return false;
    out->of.i32 = number;
    return true;
  }
  case WASM_I64: {
    int64_t number = 0;
    bool lossless = false;
    if (napi_get_value_bigint_int64(env, value, &number, &lossless) !=
        napi_ok) {
      if (napi_get_value_int64(env, value, &number) != napi_ok)
        return false;
    }
    out->of.i64 = number;
    return true;
  }
  case WASM_F32:
  case WASM_F64: {
    double number = 0;
    if (napi_get_value_double(env, value, &number) != napi_ok)
      return false;
    if (kind == WASM_F32) {
      out->of.f32 = static_cast<float>(number);
    } else {
      out->of.f64 = number;
    }
    return true;
  }
  case WASM_EXTERNREF:
  case WASM_FUNCREF:
    if (!IsNullOrUndefined(env, value))
      return false;
    out->of.ref = nullptr;
    return true;
  default:
    return false;
  }
}

napi_value WasmValToJs(napi_env env, const wasm_val_t *value) {
  if (value == nullptr)
    return Undefined(env);
  napi_value out = nullptr;
  switch (value->kind) {
  case WASM_I32:
    napi_create_int32(env, value->of.i32, &out);
    break;
  case WASM_I64:
    napi_create_bigint_int64(env, value->of.i64, &out);
    break;
  case WASM_F32:
    napi_create_double(env, static_cast<double>(value->of.f32), &out);
    break;
  case WASM_F64:
    napi_create_double(env, value->of.f64, &out);
    break;
  case WASM_EXTERNREF:
  case WASM_FUNCREF:
    out = Null(env);
    break;
  default:
    out = Undefined(env);
    break;
  }
  return out == nullptr ? Undefined(env) : out;
}

wasm_trap_t *MakeTrap(WasmState *state, const char *message) {
  if (state == nullptr || state->store == nullptr)
    return nullptr;
  wasm_message_t wasm_message;
  wasm_name_new_from_string_nt(
      &wasm_message, message == nullptr ? "WebAssembly trap" : message);
  wasm_trap_t *trap = wasm_trap_new(state->store, &wasm_message);
  wasm_name_delete(&wasm_message);
  return trap;
}

std::string TrapMessage(wasm_trap_t *trap) {
  if (trap == nullptr)
    return "WebAssembly trap";
  wasm_message_t message;
  wasm_trap_message(trap, &message);
  std::string out = NameToString(&message);
  wasm_name_delete(&message);
  if (out.empty())
    return "WebAssembly trap";
  return out;
}

bool TakePendingImportException(WasmState *state, napi_value *out) {
  if (state == nullptr || out == nullptr ||
      state->pending_import_exception_ref == nullptr)
    return false;
  napi_value exception = nullptr;
  if (napi_get_reference_value(state->env, state->pending_import_exception_ref,
                               &exception) != napi_ok ||
      exception == nullptr) {
    DeleteRefIfPresent(state->env, &state->pending_import_exception_ref);
    return false;
  }
  DeleteRefIfPresent(state->env, &state->pending_import_exception_ref);
  *out = exception;
  return true;
}

wasm_trap_t *JsImportCallback(void *raw, const wasm_val_vec_t *args,
                              wasm_val_vec_t *results) {
  auto *data = static_cast<ImportFuncData *>(raw);
  if (data == nullptr || data->env == nullptr ||
      data->function_ref == nullptr) {
    return MakeTrap(data == nullptr ? nullptr : data->state,
                    "Invalid WebAssembly import callback");
  }

  napi_env env = data->env;
  napi_value function = nullptr;
  if (napi_get_reference_value(env, data->function_ref, &function) != napi_ok ||
      function == nullptr) {
    return MakeTrap(data->state, "WebAssembly import callback was collected");
  }

  std::vector<napi_value> js_args(args == nullptr ? 0 : args->size);
  for (size_t i = 0; i < js_args.size(); ++i) {
    js_args[i] = WasmValToJs(env, &args->data[i]);
  }

  napi_value global = nullptr;
  napi_get_global(env, &global);
  napi_value result = nullptr;
  napi_status status =
      napi_call_function(env, global, function, js_args.size(),
                         js_args.empty() ? nullptr : js_args.data(), &result);
  if (status != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value exception = nullptr;
      if (napi_get_and_clear_last_exception(env, &exception) == napi_ok &&
          exception != nullptr) {
        DeleteRefIfPresent(env, &data->state->pending_import_exception_ref);
        napi_create_reference(env, exception, 1,
                              &data->state->pending_import_exception_ref);
      }
    }
    return MakeTrap(data->state, "WebAssembly import function threw");
  }

  if (results != nullptr && results->size > 0) {
    if (data->result_kinds.size() < results->size) {
      return MakeTrap(
          data->state,
          "WebAssembly import function result type metadata is missing");
    }
    if (!JsToWasmVal(env, result, data->result_kinds[0], &results->data[0])) {
      return MakeTrap(
          data->state,
          "WebAssembly import function returned an incompatible value");
    }
    for (size_t i = 1; i < results->size; ++i) {
      results->data[i].kind = data->result_kinds[i];
      results->data[i].of.ref = nullptr;
    }
  }
  return nullptr;
}

void ImportFuncDataFinalizer(void *raw) {
  auto *data = static_cast<ImportFuncData *>(raw);
  if (data == nullptr)
    return;
  DeleteRefIfPresent(data->env, &data->function_ref);
  delete data;
}

void ModuleFinalize(napi_env, void *data, void *) {
  auto *object = static_cast<WasmModuleObject *>(data);
  if (object == nullptr)
    return;
  if (object->module != nullptr)
    wasm_module_delete(object->module);
  delete object;
}

void InstanceFinalize(napi_env env, void *data, void *) {
  auto *object = static_cast<WasmInstanceObject *>(data);
  if (object == nullptr)
    return;
  DeleteRefIfPresent(env, &object->exports_ref);
  if (object->instance != nullptr)
    wasm_instance_delete(object->instance);
  delete object;
}

void MemoryFinalize(napi_env, void *data, void *) {
  auto *object = static_cast<WasmMemoryObject *>(data);
  if (object == nullptr)
    return;
  if (object->memory != nullptr)
    wasm_memory_delete(object->memory);
  delete object;
}

void TableFinalize(napi_env, void *data, void *) {
  auto *object = static_cast<WasmTableObject *>(data);
  if (object == nullptr)
    return;
  if (object->table != nullptr)
    wasm_table_delete(object->table);
  delete object;
}

void GlobalFinalize(napi_env, void *data, void *) {
  auto *object = static_cast<WasmGlobalObject *>(data);
  if (object == nullptr)
    return;
  if (object->global != nullptr)
    wasm_global_delete(object->global);
  delete object;
}

void FunctionFinalize(napi_env, void *data, void *) {
  auto *object = static_cast<WasmFunctionObject *>(data);
  if (object == nullptr)
    return;
  if (object->func != nullptr)
    wasm_func_delete(object->func);
  delete object;
}

void ExternalArrayBufferNoopFinalize(node_api_basic_env, void *, void *) {}

napi_value CreateFunctionObject(WasmState *state, const std::string &name,
                                wasm_func_t *owned_func) {
  if (state == nullptr || owned_func == nullptr)
    return nullptr;
  napi_env env = state->env;
  auto *object = new WasmFunctionObject();
  object->base.kind = WasmObjectKind::kFunction;
  object->base.state = state;
  object->func = owned_func;

  napi_value fn = nullptr;
  if (napi_create_function(
          env, name.empty() ? nullptr : name.c_str(),
          name.empty() ? 0 : name.size(),
          [](napi_env env, napi_callback_info info) -> napi_value {
            size_t argc = 32;
            napi_value argv[32] = {};
            napi_value this_arg = nullptr;
            void *raw = nullptr;
            if (!GetCallback(env, info, &argc, argv, &this_arg, &raw))
              return nullptr;
            auto *function = static_cast<WasmFunctionObject *>(raw);
            if (function == nullptr || function->func == nullptr) {
              napi_throw_type_error(env, nullptr,
                                    "Invalid WebAssembly function");
              return nullptr;
            }

            wasm_functype_t *type = wasm_func_type(function->func);
            if (type == nullptr) {
              napi_throw_error(env, nullptr,
                               "Failed to read WebAssembly function type");
              return nullptr;
            }
            const wasm_valtype_vec_t *params = wasm_functype_params(type);
            const wasm_valtype_vec_t *result_types =
                wasm_functype_results(type);
            size_t param_count = params == nullptr ? 0 : params->size;
            size_t result_count =
                result_types == nullptr ? 0 : result_types->size;
            if (argc < param_count) {
              wasm_functype_delete(type);
              napi_throw_type_error(
                  env, nullptr, "Too few arguments for WebAssembly function");
              return nullptr;
            }

            wasm_val_vec_t wasm_args;
            wasm_val_vec_t wasm_results;
            wasm_val_vec_new_uninitialized(&wasm_args, param_count);
            wasm_val_vec_new_uninitialized(&wasm_results, result_count);
            bool ok = true;
            for (size_t i = 0; i < param_count; ++i) {
              wasm_valkind_t kind = wasm_valtype_kind(params->data[i]);
              if (!JsToWasmVal(env, argv[i], kind, &wasm_args.data[i])) {
                ok = false;
                break;
              }
            }
            for (size_t i = 0; i < result_count; ++i) {
              wasm_results.data[i].kind =
                  wasm_valtype_kind(result_types->data[i]);
            }
            wasm_functype_delete(type);
            if (!ok) {
              wasm_val_vec_delete(&wasm_args);
              wasm_val_vec_delete(&wasm_results);
              napi_throw_type_error(
                  env, nullptr, "Invalid argument for WebAssembly function");
              return nullptr;
            }

            wasm_trap_t *trap =
                wasm_func_call(function->func, &wasm_args, &wasm_results);
            wasm_val_vec_delete(&wasm_args);
            if (trap != nullptr) {
              napi_value pending_exception = nullptr;
              if (TakePendingImportException(function->base.state,
                                             &pending_exception)) {
                wasm_trap_delete(trap);
                wasm_val_vec_delete(&wasm_results);
                napi_throw(env, pending_exception);
                return nullptr;
              }
              std::string message = TrapMessage(trap);
              wasm_trap_delete(trap);
              wasm_val_vec_delete(&wasm_results);
              ThrowWasmError(env, "RuntimeError", message);
              return nullptr;
            }

            napi_value out = result_count == 0
                                 ? Undefined(env)
                                 : WasmValToJs(env, &wasm_results.data[0]);
            wasm_val_vec_delete(&wasm_results);
            return out;
          },
          object, &fn) != napi_ok ||
      fn == nullptr) {
    delete object;
    return nullptr;
  }
  if (napi_wrap(env, fn, object, FunctionFinalize, nullptr, nullptr) !=
      napi_ok) {
    delete object;
    return nullptr;
  }
  return fn;
}

napi_value CreateExternObject(WasmState *state, wasm_extern_t *ext,
                              const std::string &name) {
  if (state == nullptr || ext == nullptr)
    return nullptr;
  switch (wasm_extern_kind(ext)) {
  case WASM_EXTERN_FUNC: {
    wasm_func_t *func = wasm_extern_as_func(ext);
    return func == nullptr
               ? nullptr
               : CreateFunctionObject(state, name, wasm_func_copy(func));
  }
  case WASM_EXTERN_GLOBAL: {
    wasm_global_t *global = wasm_extern_as_global(ext);
    return global == nullptr
               ? nullptr
               : CreateObjectFromOwned(state, WasmObjectKind::kGlobal,
                                       wasm_global_copy(global));
  }
  case WASM_EXTERN_TABLE: {
    wasm_table_t *table = wasm_extern_as_table(ext);
    return table == nullptr
               ? nullptr
               : CreateObjectFromOwned(state, WasmObjectKind::kTable,
                                       wasm_table_copy(table));
  }
  case WASM_EXTERN_MEMORY: {
    wasm_memory_t *memory = wasm_extern_as_memory(ext);
    return memory == nullptr
               ? nullptr
               : CreateObjectFromOwned(state, WasmObjectKind::kMemory,
                                       wasm_memory_copy(memory));
  }
  default:
    return Undefined(state->env);
  }
}

napi_value ModuleConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  napi_value this_arg = nullptr;
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr) {
    napi_throw_error(env, nullptr, "WebAssembly state is not initialized");
    return nullptr;
  }

  void *internal_module = nullptr;
  if (argc >= 1 &&
      TryConsumeInternalCreate(env, argv[0], WasmObjectKind::kModule,
                               &internal_module)) {
    auto *object = new WasmModuleObject();
    object->base.kind = WasmObjectKind::kModule;
    object->base.state = state;
    object->module = static_cast<wasm_module_t *>(internal_module);
    if (napi_wrap(env, this_arg, object, ModuleFinalize, nullptr, nullptr) !=
        napi_ok) {
      ModuleFinalize(env, object, nullptr);
      return nullptr;
    }
    return this_arg;
  }

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok ||
      new_target == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Module must be called with new");
    return nullptr;
  }
  if (argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Module requires a buffer source");
    return nullptr;
  }

  const wasm_byte_t *data = nullptr;
  size_t length = 0;
  if (!ReadBufferSource(env, argv[0], &data, &length)) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Module expects a BufferSource");
    return nullptr;
  }
  wasm_byte_vec_t bytes = BorrowedByteVec(data, length);
  wasm_module_t *module = wasm_module_new(state->store, &bytes);
  if (module == nullptr) {
    ThrowWasmError(env, "CompileError",
                   "WebAssembly.Module compilation failed");
    return nullptr;
  }

  auto *object = new WasmModuleObject();
  object->base.kind = WasmObjectKind::kModule;
  object->base.state = state;
  object->module = module;
  if (napi_wrap(env, this_arg, object, ModuleFinalize, nullptr, nullptr) !=
      napi_ok) {
    ModuleFinalize(env, object, nullptr);
    napi_throw_error(env, nullptr, "Failed to wrap WebAssembly.Module");
    return nullptr;
  }
  return this_arg;
}

bool BuildImportExtern(WasmState *state, napi_value import_object,
                       const wasm_importtype_t *import_type,
                       std::vector<wasm_func_t *> *owned_funcs,
                       wasm_extern_t **out) {
  if (state == nullptr || import_type == nullptr || owned_funcs == nullptr ||
      out == nullptr)
    return false;
  *out = nullptr;
  napi_env env = state->env;
  const std::string module_name =
      NameToString(wasm_importtype_module(import_type));
  const std::string import_name =
      NameToString(wasm_importtype_name(import_type));

  napi_value module_object = nullptr;
  if (import_object == nullptr ||
      !GetNamed(env, import_object, module_name.c_str(), &module_object) ||
      IsUndefined(env, module_object)) {
    ThrowWasmError(env, "LinkError",
                   "Missing WebAssembly import: " + module_name + "." +
                       import_name);
    return false;
  }
  napi_value value = nullptr;
  if (!GetNamed(env, module_object, import_name.c_str(), &value) ||
      IsUndefined(env, value)) {
    ThrowWasmError(env, "LinkError",
                   "Missing WebAssembly import: " + module_name + "." +
                       import_name);
    return false;
  }

  const wasm_externtype_t *ext_type = wasm_importtype_type(import_type);
  switch (wasm_externtype_kind(ext_type)) {
  case WASM_EXTERN_FUNC: {
    if (auto *wrapped =
            Unwrap<WasmFunctionObject>(env, value, WasmObjectKind::kFunction);
        wrapped != nullptr) {
      wasm_func_t *func = wasm_func_copy(wrapped->func);
      owned_funcs->push_back(func);
      *out = wasm_func_as_extern(func);
      return *out != nullptr;
    }

    napi_valuetype value_type = napi_undefined;
    if (napi_typeof(env, value, &value_type) != napi_ok ||
        value_type != napi_function) {
      ThrowWasmError(env, "LinkError",
                     "WebAssembly function import must be callable: " +
                         module_name + "." + import_name);
      return false;
    }

    const wasm_functype_t *expected_type =
        wasm_externtype_as_functype_const(ext_type);
    wasm_functype_t *copied_type =
        wasm_functype_copy(const_cast<wasm_functype_t *>(expected_type));
    auto *callback_data = new ImportFuncData();
    callback_data->state = state;
    callback_data->env = env;
    const wasm_valtype_vec_t *result_types =
        wasm_functype_results(expected_type);
    if (result_types != nullptr) {
      callback_data->result_kinds.reserve(result_types->size);
      for (size_t i = 0; i < result_types->size; ++i) {
        callback_data->result_kinds.push_back(
            wasm_valtype_kind(result_types->data[i]));
      }
    }
    if (napi_create_reference(env, value, 1, &callback_data->function_ref) !=
        napi_ok) {
      delete callback_data;
      wasm_functype_delete(copied_type);
      napi_throw_error(env, nullptr,
                       "Failed to retain WebAssembly import function");
      return false;
    }
    wasm_func_t *func =
        wasm_func_new_with_env(state->store, copied_type, JsImportCallback,
                               callback_data, ImportFuncDataFinalizer);
    wasm_functype_delete(copied_type);
    if (func == nullptr) {
      ImportFuncDataFinalizer(callback_data);
      ThrowWasmError(env, "LinkError",
                     "Failed to create WebAssembly import function");
      return false;
    }
    owned_funcs->push_back(func);
    *out = wasm_func_as_extern(func);
    return *out != nullptr;
  }
  case WASM_EXTERN_GLOBAL: {
    auto *global =
        Unwrap<WasmGlobalObject>(env, value, WasmObjectKind::kGlobal);
    if (global == nullptr || global->global == nullptr) {
      ThrowWasmError(env, "LinkError",
                     "WebAssembly global import has incompatible value: " +
                         module_name + "." + import_name);
      return false;
    }
    *out = wasm_global_as_extern(global->global);
    return *out != nullptr;
  }
  case WASM_EXTERN_TABLE: {
    auto *table = Unwrap<WasmTableObject>(env, value, WasmObjectKind::kTable);
    if (table == nullptr || table->table == nullptr) {
      ThrowWasmError(env, "LinkError",
                     "WebAssembly table import has incompatible value: " +
                         module_name + "." + import_name);
      return false;
    }
    *out = wasm_table_as_extern(table->table);
    return *out != nullptr;
  }
  case WASM_EXTERN_MEMORY: {
    auto *memory =
        Unwrap<WasmMemoryObject>(env, value, WasmObjectKind::kMemory);
    if (memory == nullptr || memory->memory == nullptr) {
      ThrowWasmError(env, "LinkError",
                     "WebAssembly memory import has incompatible value: " +
                         module_name + "." + import_name);
      return false;
    }
    *out = wasm_memory_as_extern(memory->memory);
    return *out != nullptr;
  }
  default:
    ThrowWasmError(env, "LinkError", "Unsupported WebAssembly import type");
    return false;
  }
}

napi_value InstanceConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  napi_value this_arg = nullptr;
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr) {
    napi_throw_error(env, nullptr, "WebAssembly state is not initialized");
    return nullptr;
  }

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok ||
      new_target == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Instance must be called with new");
    return nullptr;
  }
  if (argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Instance requires a WebAssembly.Module");
    return nullptr;
  }
  auto *module =
      Unwrap<WasmModuleObject>(env, argv[0], WasmObjectKind::kModule);
  if (module == nullptr || module->module == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Instance expects a WebAssembly.Module");
    return nullptr;
  }

  wasm_importtype_vec_t import_types;
  wasm_module_imports(module->module, &import_types);
  std::vector<wasm_extern_t *> import_externs(import_types.size);
  std::vector<wasm_func_t *> owned_import_funcs;
  bool ok = true;
  for (size_t i = 0; i < import_types.size; ++i) {
    if (!BuildImportExtern(state, argc >= 2 ? argv[1] : nullptr,
                           import_types.data[i], &owned_import_funcs,
                           &import_externs[i])) {
      ok = false;
      break;
    }
  }
  if (!ok) {
    for (wasm_func_t *func : owned_import_funcs)
      wasm_func_delete(func);
    wasm_importtype_vec_delete(&import_types);
    return nullptr;
  }

  wasm_extern_vec_t imports;
  imports.size = import_externs.size();
  imports.data = import_externs.empty() ? nullptr : import_externs.data();
  wasm_trap_t *trap = nullptr;
  wasm_instance_t *instance =
      wasm_instance_new(state->store, module->module, &imports, &trap);
  for (wasm_func_t *func : owned_import_funcs)
    wasm_func_delete(func);
  wasm_importtype_vec_delete(&import_types);

  if (trap != nullptr) {
    napi_value pending_exception = nullptr;
    if (TakePendingImportException(state, &pending_exception)) {
      wasm_trap_delete(trap);
      napi_throw(env, pending_exception);
      return nullptr;
    }
    std::string message = TrapMessage(trap);
    wasm_trap_delete(trap);
    ThrowWasmError(env, "RuntimeError", message);
    return nullptr;
  }
  if (instance == nullptr) {
    ThrowWasmError(env, "LinkError",
                   "WebAssembly.Instance instantiation failed");
    return nullptr;
  }

  wasm_extern_vec_t exports;
  wasm_exporttype_vec_t export_types;
  wasm_instance_exports(instance, &exports);
  wasm_module_exports(module->module, &export_types);

  napi_value exports_object = nullptr;
  napi_create_object(env, &exports_object);
  const size_t export_count = std::min(exports.size, export_types.size);
  for (size_t i = 0; i < export_count; ++i) {
    std::string name = NameToString(wasm_exporttype_name(export_types.data[i]));
    napi_value js_export = CreateExternObject(state, exports.data[i], name);
    if (js_export != nullptr) {
      SetNamed(env, exports_object, name.c_str(), js_export);
    }
  }
  wasm_extern_vec_delete(&exports);
  wasm_exporttype_vec_delete(&export_types);

  auto *object = new WasmInstanceObject();
  object->base.kind = WasmObjectKind::kInstance;
  object->base.state = state;
  object->instance = instance;
  napi_create_reference(env, exports_object, 1, &object->exports_ref);
  if (napi_wrap(env, this_arg, object, InstanceFinalize, nullptr, nullptr) !=
      napi_ok) {
    InstanceFinalize(env, object, nullptr);
    napi_throw_error(env, nullptr, "Failed to wrap WebAssembly.Instance");
    return nullptr;
  }
  return this_arg;
}

napi_value InstanceExportsGetter(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, nullptr, &this_arg, nullptr))
    return nullptr;
  auto *object =
      Unwrap<WasmInstanceObject>(env, this_arg, WasmObjectKind::kInstance);
  if (object == nullptr || object->exports_ref == nullptr)
    return Undefined(env);
  napi_value exports = nullptr;
  if (napi_get_reference_value(env, object->exports_ref, &exports) != napi_ok ||
      exports == nullptr) {
    return Undefined(env);
  }
  return exports;
}

bool BuildLimitsFromDescriptor(napi_env env, napi_value descriptor,
                               wasm_limits_t *limits) {
  if (limits == nullptr)
    return false;
  uint32_t initial = 0;
  uint32_t maximum = wasm_limits_max_default;
  if (!GetRequiredUint32(env, descriptor, "initial", &initial) ||
      !GetOptionalUint32(env, descriptor, "maximum", wasm_limits_max_default,
                         &maximum)) {
    return false;
  }
  if (maximum != wasm_limits_max_default && initial > maximum)
    return false;
  limits->min = initial;
  limits->max = maximum;
  return true;
}

napi_value MemoryConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  napi_value this_arg = nullptr;
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr)
    return nullptr;

  void *internal_memory = nullptr;
  if (argc >= 1 &&
      TryConsumeInternalCreate(env, argv[0], WasmObjectKind::kMemory,
                               &internal_memory)) {
    auto *object = new WasmMemoryObject();
    object->base.kind = WasmObjectKind::kMemory;
    object->base.state = state;
    object->memory = static_cast<wasm_memory_t *>(internal_memory);
    if (napi_wrap(env, this_arg, object, MemoryFinalize, nullptr, nullptr) !=
        napi_ok) {
      MemoryFinalize(env, object, nullptr);
      return nullptr;
    }
    return this_arg;
  }

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok ||
      new_target == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Memory must be called with new");
    return nullptr;
  }
  if (argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Memory requires a descriptor");
    return nullptr;
  }
  bool shared = false;
  if (GetOptionalBool(env, argv[0], "shared", false, &shared) && shared) {
    napi_throw_type_error(
        env, nullptr,
        "Shared WebAssembly.Memory is not supported by this QuickJS build");
    return nullptr;
  }

  wasm_limits_t limits;
  if (!BuildLimitsFromDescriptor(env, argv[0], &limits)) {
    napi_throw_type_error(env, nullptr,
                          "Invalid WebAssembly.Memory descriptor");
    return nullptr;
  }
  wasm_memorytype_t *type = wasm_memorytype_new(&limits);
  if (type == nullptr) {
    napi_throw_range_error(env, nullptr, "Invalid WebAssembly.Memory limits");
    return nullptr;
  }
  wasm_memory_t *memory = wasm_memory_new(state->store, type);
  wasm_memorytype_delete(type);
  if (memory == nullptr) {
    napi_throw_range_error(env, nullptr, "Failed to create WebAssembly.Memory");
    return nullptr;
  }

  auto *object = new WasmMemoryObject();
  object->base.kind = WasmObjectKind::kMemory;
  object->base.state = state;
  object->memory = memory;
  if (napi_wrap(env, this_arg, object, MemoryFinalize, nullptr, nullptr) !=
      napi_ok) {
    MemoryFinalize(env, object, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value MemoryBufferGetter(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, nullptr, &this_arg, nullptr))
    return nullptr;
  auto *object =
      Unwrap<WasmMemoryObject>(env, this_arg, WasmObjectKind::kMemory);
  if (object == nullptr || object->memory == nullptr) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Memory");
    return nullptr;
  }
  void *data = wasm_memory_data(object->memory);
  size_t size = wasm_memory_data_size(object->memory);
  napi_value array_buffer = nullptr;
  if (napi_create_external_arraybuffer(env, data, size,
                                       ExternalArrayBufferNoopFinalize, nullptr,
                                       &array_buffer) != napi_ok ||
      array_buffer == nullptr) {
    napi_throw_error(env, nullptr,
                     "Failed to create WebAssembly.Memory buffer");
    return nullptr;
  }
  return array_buffer;
}

napi_value MemoryGrow(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, nullptr))
    return nullptr;
  auto *object =
      Unwrap<WasmMemoryObject>(env, this_arg, WasmObjectKind::kMemory);
  if (object == nullptr || object->memory == nullptr) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Memory");
    return nullptr;
  }
  uint32_t delta = 0;
  if (argc < 1 || napi_get_value_uint32(env, argv[0], &delta) != napi_ok) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Memory.grow expects a page count");
    return nullptr;
  }
  uint32_t previous = wasm_memory_size(object->memory);
  if (!wasm_memory_grow(object->memory, delta)) {
    napi_throw_range_error(env, nullptr, "WebAssembly.Memory.grow failed");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_uint32(env, previous, &out);
  return out;
}

napi_value TableConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  napi_value this_arg = nullptr;
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr)
    return nullptr;

  void *internal_table = nullptr;
  if (argc >= 1 && TryConsumeInternalCreate(
                       env, argv[0], WasmObjectKind::kTable, &internal_table)) {
    auto *object = new WasmTableObject();
    object->base.kind = WasmObjectKind::kTable;
    object->base.state = state;
    object->table = static_cast<wasm_table_t *>(internal_table);
    if (napi_wrap(env, this_arg, object, TableFinalize, nullptr, nullptr) !=
        napi_ok) {
      TableFinalize(env, object, nullptr);
      return nullptr;
    }
    return this_arg;
  }

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok ||
      new_target == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table must be called with new");
    return nullptr;
  }
  if (argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table requires a descriptor");
    return nullptr;
  }

  napi_value element_value = nullptr;
  wasm_valkind_t element_kind = WASM_FUNCREF;
  if (!GetNamed(env, argv[0], "element", &element_value) ||
      IsUndefined(env, element_value) ||
      !ParseValueKind(env, element_value, &element_kind) ||
      (element_kind != WASM_FUNCREF && element_kind != WASM_EXTERNREF)) {
    napi_throw_type_error(env, nullptr,
                          "Invalid WebAssembly.Table element type");
    return nullptr;
  }
  wasm_limits_t limits;
  if (!BuildLimitsFromDescriptor(env, argv[0], &limits)) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Table descriptor");
    return nullptr;
  }

  if (argc >= 2 && !IsNullOrUndefined(env, argv[1])) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table non-null initial values are not "
                          "supported by this Wasmer C API build");
    return nullptr;
  }

  auto *object = new WasmTableObject();
  object->base.kind = WasmObjectKind::kTable;
  object->base.state = state;
  object->local_only = true;
  object->local_size = limits.min;
  object->local_max = limits.max;
  object->local_element_kind = element_kind;
  if (napi_wrap(env, this_arg, object, TableFinalize, nullptr, nullptr) !=
      napi_ok) {
    TableFinalize(env, object, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value TableLengthGetter(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, nullptr, &this_arg, nullptr))
    return nullptr;
  auto *object = Unwrap<WasmTableObject>(env, this_arg, WasmObjectKind::kTable);
  if (object == nullptr || (!object->local_only && object->table == nullptr)) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Table");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_uint32(env,
                     object->local_only ? object->local_size
                                        : wasm_table_size(object->table),
                     &out);
  return out;
}

napi_value TableGet(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, nullptr))
    return nullptr;
  auto *object = Unwrap<WasmTableObject>(env, this_arg, WasmObjectKind::kTable);
  if (object == nullptr || (!object->local_only && object->table == nullptr)) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Table");
    return nullptr;
  }
  uint32_t index = 0;
  if (argc < 1 || napi_get_value_uint32(env, argv[0], &index) != napi_ok) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table.get expects an index");
    return nullptr;
  }
  uint32_t size =
      object->local_only ? object->local_size : wasm_table_size(object->table);
  if (index >= size) {
    napi_throw_range_error(env, nullptr,
                           "WebAssembly.Table.get index is out of range");
    return nullptr;
  }
  return Null(env);
}

napi_value TableSet(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, nullptr))
    return nullptr;
  auto *object = Unwrap<WasmTableObject>(env, this_arg, WasmObjectKind::kTable);
  if (object == nullptr || (!object->local_only && object->table == nullptr)) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Table");
    return nullptr;
  }
  uint32_t index = 0;
  if (argc < 1 || napi_get_value_uint32(env, argv[0], &index) != napi_ok) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table.set expects an index");
    return nullptr;
  }
  if (argc >= 2 && !IsNullOrUndefined(env, argv[1])) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table.set only supports null values "
                          "with this Wasmer C API build");
    return nullptr;
  }
  uint32_t size =
      object->local_only ? object->local_size : wasm_table_size(object->table);
  if (index >= size) {
    napi_throw_range_error(env, nullptr,
                           "WebAssembly.Table.set index is out of range");
    return nullptr;
  }
  return Undefined(env);
}

napi_value TableGrow(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, nullptr))
    return nullptr;
  auto *object = Unwrap<WasmTableObject>(env, this_arg, WasmObjectKind::kTable);
  if (object == nullptr || (!object->local_only && object->table == nullptr)) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Table");
    return nullptr;
  }
  uint32_t delta = 0;
  if (argc < 1 || napi_get_value_uint32(env, argv[0], &delta) != napi_ok) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table.grow expects a count");
    return nullptr;
  }
  if (argc >= 2 && !IsNullOrUndefined(env, argv[1])) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Table.grow only supports null initial "
                          "values with this Wasmer C API build");
    return nullptr;
  }
  uint32_t previous =
      object->local_only ? object->local_size : wasm_table_size(object->table);
  if (object->local_only) {
    if (delta > object->local_max || previous > object->local_max - delta) {
      napi_throw_range_error(env, nullptr, "WebAssembly.Table.grow failed");
      return nullptr;
    }
    object->local_size += delta;
  } else if (!wasm_table_grow(object->table, delta, nullptr)) {
    napi_throw_range_error(env, nullptr, "WebAssembly.Table.grow failed");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_uint32(env, previous, &out);
  return out;
}

napi_value GlobalConstructor(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  napi_value this_arg = nullptr;
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr)
    return nullptr;

  void *internal_global = nullptr;
  if (argc >= 1 &&
      TryConsumeInternalCreate(env, argv[0], WasmObjectKind::kGlobal,
                               &internal_global)) {
    auto *object = new WasmGlobalObject();
    object->base.kind = WasmObjectKind::kGlobal;
    object->base.state = state;
    object->global = static_cast<wasm_global_t *>(internal_global);
    if (napi_wrap(env, this_arg, object, GlobalFinalize, nullptr, nullptr) !=
        napi_ok) {
      GlobalFinalize(env, object, nullptr);
      return nullptr;
    }
    return this_arg;
  }

  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok ||
      new_target == nullptr) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Global must be called with new");
    return nullptr;
  }
  if (argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.Global requires a descriptor");
    return nullptr;
  }
  napi_value value_kind_name = nullptr;
  wasm_valkind_t value_kind = WASM_I32;
  if (!GetNamed(env, argv[0], "value", &value_kind_name) ||
      IsUndefined(env, value_kind_name) ||
      !ParseValueKind(env, value_kind_name, &value_kind)) {
    napi_throw_type_error(env, nullptr,
                          "Invalid WebAssembly.Global value type");
    return nullptr;
  }
  bool is_mutable = false;
  if (!GetOptionalBool(env, argv[0], "mutable", false, &is_mutable)) {
    napi_throw_type_error(env, nullptr,
                          "Invalid WebAssembly.Global mutability");
    return nullptr;
  }

  napi_value initial = argc >= 2 ? argv[1] : nullptr;
  napi_value zero = nullptr;
  if (IsUndefined(env, initial)) {
    napi_create_int32(env, 0, &zero);
    initial = zero;
  }
  wasm_val_t initial_value;
  if (!JsToWasmVal(env, initial, value_kind, &initial_value)) {
    napi_throw_type_error(env, nullptr,
                          "Invalid WebAssembly.Global initial value");
    return nullptr;
  }
  wasm_valtype_t *content = wasm_valtype_new(value_kind);
  wasm_globaltype_t *type =
      wasm_globaltype_new(content, is_mutable ? WASM_VAR : WASM_CONST);
  wasm_global_t *global = wasm_global_new(state->store, type, &initial_value);
  wasm_globaltype_delete(type);
  wasm_val_delete(&initial_value);
  if (global == nullptr) {
    napi_throw_error(env, nullptr, "Failed to create WebAssembly.Global");
    return nullptr;
  }

  auto *object = new WasmGlobalObject();
  object->base.kind = WasmObjectKind::kGlobal;
  object->base.state = state;
  object->global = global;
  if (napi_wrap(env, this_arg, object, GlobalFinalize, nullptr, nullptr) !=
      napi_ok) {
    GlobalFinalize(env, object, nullptr);
    return nullptr;
  }
  return this_arg;
}

napi_value GlobalValueGetter(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, nullptr, &this_arg, nullptr))
    return nullptr;
  auto *object =
      Unwrap<WasmGlobalObject>(env, this_arg, WasmObjectKind::kGlobal);
  if (object == nullptr || object->global == nullptr) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Global");
    return nullptr;
  }
  wasm_val_t value;
  wasm_global_get(object->global, &value);
  napi_value out = WasmValToJs(env, &value);
  wasm_val_delete(&value);
  return out;
}

napi_value GlobalValueSetter(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  napi_value this_arg = nullptr;
  if (!GetCallback(env, info, &argc, argv, &this_arg, nullptr))
    return nullptr;
  auto *object =
      Unwrap<WasmGlobalObject>(env, this_arg, WasmObjectKind::kGlobal);
  if (object == nullptr || object->global == nullptr) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Global");
    return nullptr;
  }
  wasm_globaltype_t *type = wasm_global_type(object->global);
  if (type == nullptr)
    return nullptr;
  if (wasm_globaltype_mutability(type) != WASM_VAR) {
    wasm_globaltype_delete(type);
    napi_throw_type_error(env, nullptr, "WebAssembly.Global is immutable");
    return nullptr;
  }
  wasm_valkind_t kind = wasm_valtype_kind(wasm_globaltype_content(type));
  wasm_val_t value;
  bool ok = argc >= 1 && JsToWasmVal(env, argv[0], kind, &value);
  wasm_globaltype_delete(type);
  if (!ok) {
    napi_throw_type_error(env, nullptr, "Invalid WebAssembly.Global value");
    return nullptr;
  }
  wasm_global_set(object->global, &value);
  wasm_val_delete(&value);
  return Undefined(env);
}

napi_value ValidateCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  void *raw_state = nullptr;
  if (!GetCallback(env, info, &argc, argv, nullptr, &raw_state))
    return nullptr;
  auto *state = static_cast<WasmState *>(raw_state);
  if (state == nullptr || argc < 1) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.validate expects a BufferSource");
    return nullptr;
  }
  const wasm_byte_t *data = nullptr;
  size_t length = 0;
  if (!ReadBufferSource(env, argv[0], &data, &length)) {
    napi_throw_type_error(env, nullptr,
                          "WebAssembly.validate expects a BufferSource");
    return nullptr;
  }
  wasm_byte_vec_t bytes = BorrowedByteVec(data, length);
  bool valid = wasm_module_validate(state->store, &bytes);
  napi_value out = nullptr;
  napi_get_boolean(env, valid, &out);
  return out;
}

napi_value ModuleImportsCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  if (!GetCallback(env, info, &argc, argv, nullptr, nullptr))
    return nullptr;
  auto *module = argc >= 1 ? Unwrap<WasmModuleObject>(env, argv[0],
                                                      WasmObjectKind::kModule)
                           : nullptr;
  if (module == nullptr || module->module == nullptr) {
    napi_throw_type_error(
        env, nullptr,
        "WebAssembly.Module.imports expects a WebAssembly.Module");
    return nullptr;
  }
  wasm_importtype_vec_t imports;
  wasm_module_imports(module->module, &imports);
  napi_value array = nullptr;
  napi_create_array_with_length(env, imports.size, &array);
  for (size_t i = 0; i < imports.size; ++i) {
    napi_value desc = nullptr;
    napi_create_object(env, &desc);
    SetNamedString(env, desc, "module",
                   NameToString(wasm_importtype_module(imports.data[i])));
    SetNamedString(env, desc, "name",
                   NameToString(wasm_importtype_name(imports.data[i])));
    SetNamedString(env, desc, "kind",
                   ExternKindName(wasm_externtype_kind(
                       wasm_importtype_type(imports.data[i]))));
    napi_set_element(env, array, static_cast<uint32_t>(i), desc);
  }
  wasm_importtype_vec_delete(&imports);
  return array;
}

napi_value ModuleExportsCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {};
  if (!GetCallback(env, info, &argc, argv, nullptr, nullptr))
    return nullptr;
  auto *module = argc >= 1 ? Unwrap<WasmModuleObject>(env, argv[0],
                                                      WasmObjectKind::kModule)
                           : nullptr;
  if (module == nullptr || module->module == nullptr) {
    napi_throw_type_error(
        env, nullptr,
        "WebAssembly.Module.exports expects a WebAssembly.Module");
    return nullptr;
  }
  wasm_exporttype_vec_t exports;
  wasm_module_exports(module->module, &exports);
  napi_value array = nullptr;
  napi_create_array_with_length(env, exports.size, &array);
  for (size_t i = 0; i < exports.size; ++i) {
    napi_value desc = nullptr;
    napi_create_object(env, &desc);
    SetNamedString(env, desc, "name",
                   NameToString(wasm_exporttype_name(exports.data[i])));
    SetNamedString(env, desc, "kind",
                   ExternKindName(wasm_externtype_kind(
                       wasm_exporttype_type(exports.data[i]))));
    napi_set_element(env, array, static_cast<uint32_t>(i), desc);
  }
  wasm_exporttype_vec_delete(&exports);
  return array;
}

napi_value ModuleCustomSectionsCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {};
  if (!GetCallback(env, info, &argc, argv, nullptr, nullptr))
    return nullptr;
  auto *module = argc >= 1 ? Unwrap<WasmModuleObject>(env, argv[0],
                                                      WasmObjectKind::kModule)
                           : nullptr;
  if (module == nullptr || module->module == nullptr) {
    napi_throw_type_error(
        env, nullptr,
        "WebAssembly.Module.customSections expects a WebAssembly.Module");
    return nullptr;
  }
  napi_value array = nullptr;
  napi_create_array_with_length(env, 0, &array);
  return array;
}

napi_status DefineValue(
    napi_env env, napi_value object, const char *name, napi_value value,
    napi_property_attributes attributes = static_cast<napi_property_attributes>(
        napi_writable | napi_configurable)) {
  napi_property_descriptor desc = {};
  desc.utf8name = name;
  desc.value = value;
  desc.attributes = attributes;
  return napi_define_properties(env, object, 1, &desc);
}

bool InstallJsWrappers(napi_env env, std::string *error_out) {
  static const char *kScript = R"JS(
(function(WA) {
  'use strict';
  function makeError(name) {
    function WasmError(message) {
      var err = new Error(message === undefined ? '' : String(message));
      if (new.target) Object.setPrototypeOf(err, new.target.prototype);
      Object.defineProperty(err, 'name', { value: name, configurable: true });
      return err;
    }
    Object.setPrototypeOf(WasmError, Error);
    WasmError.prototype = Object.create(Error.prototype, {
      constructor: { value: WasmError, writable: true, configurable: true },
      name: { value: name, configurable: true }
    });
    return WasmError;
  }
  if (typeof WA.CompileError !== 'function') WA.CompileError = makeError('CompileError');
  if (typeof WA.LinkError !== 'function') WA.LinkError = makeError('LinkError');
  if (typeof WA.RuntimeError !== 'function') WA.RuntimeError = makeError('RuntimeError');

  var NativeModule = WA.Module;
  var NativeInstance = WA.Instance;
  WA.validate = WA.__edgeValidate;
  NativeModule.imports = WA.__edgeModuleImports;
  NativeModule.exports = WA.__edgeModuleExports;
  NativeModule.customSections = WA.__edgeModuleCustomSections;

  WA.compile = function compile(bytes) {
    return Promise.resolve().then(function() {
      return new NativeModule(bytes);
    });
  };

  WA.instantiate = function instantiate(bytesOrModule, imports) {
    return Promise.resolve().then(function() {
      if (bytesOrModule instanceof NativeModule) {
        return new NativeInstance(bytesOrModule, imports);
      }
      var module = new NativeModule(bytesOrModule);
      var instance = new NativeInstance(module, imports);
      return { module: module, instance: instance };
    });
  };

  WA.compileStreaming = function compileStreaming(source) {
    return Promise.resolve(source).then(function(response) {
      if (response == null || typeof response.arrayBuffer !== 'function') {
        throw new TypeError('WebAssembly.compileStreaming expects a Response or Response-like object');
      }
      return response.arrayBuffer();
    }).then(WA.compile);
  };

  WA.instantiateStreaming = function instantiateStreaming(source, imports) {
    return Promise.resolve(source).then(function(response) {
      if (response == null || typeof response.arrayBuffer !== 'function') {
        throw new TypeError('WebAssembly.instantiateStreaming expects a Response or Response-like object');
      }
      return response.arrayBuffer();
    }).then(function(bytes) {
      return WA.instantiate(bytes, imports);
    });
  };

  delete WA.__edgeValidate;
  delete WA.__edgeModuleImports;
  delete WA.__edgeModuleExports;
  delete WA.__edgeModuleCustomSections;
  if (typeof Symbol === 'function' && Symbol.toStringTag) {
    Object.defineProperty(WA, Symbol.toStringTag, { value: 'WebAssembly', configurable: true });
  }
})(globalThis.WebAssembly);
)JS";
  napi_value source = MakeString(env, kScript);
  napi_value result = nullptr;
  if (napi_run_script(env, source, &result) == napi_ok)
    return true;
  if (error_out != nullptr) {
    *error_out = "Failed to install QuickJS WebAssembly JS wrappers";
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value exception = nullptr;
      if (napi_get_and_clear_last_exception(env, &exception) == napi_ok &&
          exception != nullptr) {
        napi_value text = nullptr;
        if (napi_coerce_to_string(env, exception, &text) == napi_ok &&
            text != nullptr) {
          *error_out += ": ";
          *error_out += GetString(env, text);
        }
      }
    }
  }
  return false;
}

bool StoreConstructorRef(napi_env env, napi_value ctor, napi_ref *ref) {
  DeleteRefIfPresent(env, ref);
  return napi_create_reference(env, ctor, 1, ref) == napi_ok;
}

} // namespace

bool EdgeInstallQuickJsWebAssembly(napi_env env, std::string *error_out) {
  if (env == nullptr) {
    if (error_out != nullptr)
      *error_out = "Invalid environment";
    return false;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    if (error_out != nullptr)
      *error_out = "Failed to fetch global object";
    return false;
  }
  napi_value existing = nullptr;
  if (GetNamed(env, global, "WebAssembly", &existing) &&
      !IsUndefined(env, existing)) {
    return true;
  }

  WasmState *state = EnsureState(env, error_out);
  if (state == nullptr)
    return false;

  napi_value webassembly = nullptr;
  if (napi_create_object(env, &webassembly) != napi_ok ||
      webassembly == nullptr) {
    if (error_out != nullptr)
      *error_out = "Failed to create WebAssembly object";
    return false;
  }

  napi_property_descriptor memory_props[] = {
      {"grow", nullptr, MemoryGrow, nullptr, nullptr, nullptr,
       napi_default_method, nullptr},
      {"buffer", nullptr, nullptr, MemoryBufferGetter, nullptr, nullptr,
       napi_default, nullptr},
  };
  napi_property_descriptor table_props[] = {
      {"get", nullptr, TableGet, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"set", nullptr, TableSet, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"grow", nullptr, TableGrow, nullptr, nullptr, nullptr,
       napi_default_method, nullptr},
      {"length", nullptr, nullptr, TableLengthGetter, nullptr, nullptr,
       napi_default, nullptr},
  };
  napi_property_descriptor global_props[] = {
      {"valueOf", nullptr, GlobalValueGetter, nullptr, nullptr, nullptr,
       napi_default_method, nullptr},
      {"value", nullptr, nullptr, GlobalValueGetter, GlobalValueSetter, nullptr,
       napi_default, nullptr},
  };
  napi_property_descriptor instance_props[] = {
      {"exports", nullptr, nullptr, InstanceExportsGetter, nullptr, nullptr,
       napi_default, nullptr},
  };

  napi_value module_ctor = nullptr;
  napi_value instance_ctor = nullptr;
  napi_value memory_ctor = nullptr;
  napi_value table_ctor = nullptr;
  napi_value global_ctor = nullptr;
  if (napi_define_class(env, "Module", NAPI_AUTO_LENGTH, ModuleConstructor,
                        state, 0, nullptr, &module_ctor) != napi_ok ||
      napi_define_class(env, "Instance", NAPI_AUTO_LENGTH, InstanceConstructor,
                        state,
                        sizeof(instance_props) / sizeof(instance_props[0]),
                        instance_props, &instance_ctor) != napi_ok ||
      napi_define_class(env, "Memory", NAPI_AUTO_LENGTH, MemoryConstructor,
                        state, sizeof(memory_props) / sizeof(memory_props[0]),
                        memory_props, &memory_ctor) != napi_ok ||
      napi_define_class(env, "Table", NAPI_AUTO_LENGTH, TableConstructor, state,
                        sizeof(table_props) / sizeof(table_props[0]),
                        table_props, &table_ctor) != napi_ok ||
      napi_define_class(env, "Global", NAPI_AUTO_LENGTH, GlobalConstructor,
                        state, sizeof(global_props) / sizeof(global_props[0]),
                        global_props, &global_ctor) != napi_ok) {
    if (error_out != nullptr)
      *error_out = "Failed to define WebAssembly constructors";
    return false;
  }

  if (!StoreConstructorRef(env, module_ctor, &state->module_ctor_ref) ||
      !StoreConstructorRef(env, instance_ctor, &state->instance_ctor_ref) ||
      !StoreConstructorRef(env, memory_ctor, &state->memory_ctor_ref) ||
      !StoreConstructorRef(env, table_ctor, &state->table_ctor_ref) ||
      !StoreConstructorRef(env, global_ctor, &state->global_ctor_ref)) {
    if (error_out != nullptr)
      *error_out = "Failed to retain WebAssembly constructors";
    return false;
  }

  napi_value validate = nullptr;
  napi_value imports = nullptr;
  napi_value exports = nullptr;
  napi_value custom_sections = nullptr;
  if (napi_create_function(env, "__edgeValidate", NAPI_AUTO_LENGTH,
                           ValidateCallback, state, &validate) != napi_ok ||
      napi_create_function(env, "__edgeModuleImports", NAPI_AUTO_LENGTH,
                           ModuleImportsCallback, state, &imports) != napi_ok ||
      napi_create_function(env, "__edgeModuleExports", NAPI_AUTO_LENGTH,
                           ModuleExportsCallback, state, &exports) != napi_ok ||
      napi_create_function(env, "__edgeModuleCustomSections", NAPI_AUTO_LENGTH,
                           ModuleCustomSectionsCallback, state,
                           &custom_sections) != napi_ok) {
    if (error_out != nullptr)
      *error_out = "Failed to create WebAssembly native methods";
    return false;
  }

  DefineValue(env, webassembly, "Module", module_ctor);
  DefineValue(env, webassembly, "Instance", instance_ctor);
  DefineValue(env, webassembly, "Memory", memory_ctor);
  DefineValue(env, webassembly, "Table", table_ctor);
  DefineValue(env, webassembly, "Global", global_ctor);
  DefineValue(env, webassembly, "__edgeValidate", validate);
  DefineValue(env, webassembly, "__edgeModuleImports", imports);
  DefineValue(env, webassembly, "__edgeModuleExports", exports);
  DefineValue(env, webassembly, "__edgeModuleCustomSections", custom_sections);

  if (DefineValue(env, global, "WebAssembly", webassembly,
                  static_cast<napi_property_attributes>(
                      napi_writable | napi_configurable)) != napi_ok) {
    if (error_out != nullptr)
      *error_out = "Failed to install global WebAssembly";
    return false;
  }
  DeleteRefIfPresent(env, &state->webassembly_ref);
  napi_create_reference(env, webassembly, 1, &state->webassembly_ref);

  return InstallJsWrappers(env, error_out);
}
