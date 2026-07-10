#include "internal_binding/binding_initializers.h"

#include <cstdint>
#include <vector>

#include "binding_registry/binding_registry.h"
#include "edge_util.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

struct UtilBindingState {
  napi_ref types_ref = nullptr;
  std::vector<void*> callback_data;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value GetTypesBinding(napi_env env, UtilBindingState* state) {
  if (state == nullptr || state->types_ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, state->types_ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

enum class TypesAlias : uint8_t {
  kIsAnyArrayBuffer,
  kIsArrayBuffer,
  kIsAsyncFunction,
  kIsDataView,
  kIsDate,
  kIsExternal,
  kIsMap,
  kIsMapIterator,
  kIsNativeError,
  kIsPromise,
  kIsRegExp,
  kIsSet,
  kIsSetIterator,
};

const char* TypesMethodName(TypesAlias alias) {
  switch (alias) {
    case TypesAlias::kIsAnyArrayBuffer:
      return "isAnyArrayBuffer";
    case TypesAlias::kIsArrayBuffer:
      return "isArrayBuffer";
    case TypesAlias::kIsAsyncFunction:
      return "isAsyncFunction";
    case TypesAlias::kIsDataView:
      return "isDataView";
    case TypesAlias::kIsDate:
      return "isDate";
    case TypesAlias::kIsExternal:
      return "isExternal";
    case TypesAlias::kIsMap:
      return "isMap";
    case TypesAlias::kIsMapIterator:
      return "isMapIterator";
    case TypesAlias::kIsNativeError:
      return "isNativeError";
    case TypesAlias::kIsPromise:
      return "isPromise";
    case TypesAlias::kIsRegExp:
      return "isRegExp";
    case TypesAlias::kIsSet:
      return "isSet";
    case TypesAlias::kIsSetIterator:
      return "isSetIterator";
  }
  return "";
}

struct UtilTypesMethodData {
  UtilBindingState* state = nullptr;
  TypesAlias alias = TypesAlias::kIsAnyArrayBuffer;
};

void UtilBindingFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* state = static_cast<UtilBindingState*>(data);
  if (state == nullptr) return;
  DeleteRefIfPresent(env, &state->types_ref);
  for (void* ptr : state->callback_data) {
    delete static_cast<UtilTypesMethodData*>(ptr);
  }
  delete state;
}

UtilBindingState* GetOrCreateUtilBindingState(napi_env env, napi_value binding) {
  if (binding == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, binding, &data) == napi_ok && data != nullptr) {
    return static_cast<UtilBindingState*>(data);
  }

  auto* state = new UtilBindingState();
  if (napi_wrap(env, binding, state, UtilBindingFinalize, nullptr, nullptr) != napi_ok) {
    delete state;
    return nullptr;
  }
  return state;
}

napi_value CallTypesPredicate(napi_env env, UtilBindingState* state, TypesAlias alias, napi_value value) {
  napi_value types = GetTypesBinding(env, state);
  if (types == nullptr || IsUndefined(env, types)) return Undefined(env);
  const char* name = TypesMethodName(alias);
  napi_value fn = nullptr;
  napi_valuetype t = napi_undefined;
  if (napi_get_named_property(env, types, name, &fn) != napi_ok ||
      fn == nullptr ||
      napi_typeof(env, fn, &t) != napi_ok ||
      t != napi_function) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  napi_value argv[1] = {value != nullptr ? value : Undefined(env)};
  if (napi_call_function(env, types, fn, 1, argv, &out) != napi_ok || out == nullptr) return Undefined(env);
  return out;
}

napi_value UtilTypesAliasCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  void* data = nullptr;
  napi_get_cb_info(env, info, &argc, argv, nullptr, &data);
  auto* method_data = static_cast<UtilTypesMethodData*>(data);
  if (method_data == nullptr) return Undefined(env);
  return CallTypesPredicate(
      env,
      method_data->state,
      method_data->alias,
      argc >= 1 ? argv[0] : Undefined(env));
}

napi_value UtilIsArrayBufferView(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool result = false;
  if (argc >= 1 && argv[0] != nullptr) {
    bool is_typedarray = false;
    bool is_dataview = false;
    if (napi_is_typedarray(env, argv[0], &is_typedarray) == napi_ok && is_typedarray) result = true;
    if (napi_is_dataview(env, argv[0], &is_dataview) == napi_ok && is_dataview) result = true;
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value UtilIsTypedArray(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool result = false;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_is_typedarray(env, argv[0], &result);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value UtilIsUint8Array(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool result = false;
  if (argc >= 1 && argv[0] != nullptr) {
    bool is_typedarray = false;
    if (napi_is_typedarray(env, argv[0], &is_typedarray) == napi_ok && is_typedarray) {
      napi_typedarray_type type = napi_uint8_array;
      size_t length = 0;
      void* data = nullptr;
      napi_value arraybuffer = nullptr;
      size_t offset = 0;
      if (napi_get_typedarray_info(env, argv[0], &type, &length, &data, &arraybuffer, &offset) == napi_ok) {
        result = (type == napi_uint8_array);
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

void EnsureMethodFromTypes(napi_env env,
                           napi_value binding,
                           UtilBindingState* state,
                           const char* name,
                           TypesAlias alias) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  if (state == nullptr) return;
  auto* data = new UtilTypesMethodData();
  data->state = state;
  data->alias = alias;
  state->callback_data.push_back(data);
  napi_value fn = nullptr;
  napi_create_function(env,
                       name,
                       NAPI_AUTO_LENGTH,
                       UtilTypesAliasCallback,
                       data,
                       &fn);
  if (fn != nullptr) napi_set_named_property(env, binding, name, fn);
}

void EnsureMethod(napi_env env, napi_value binding, const char* name, napi_callback cb) {
  bool has = false;
  if (napi_has_named_property(env, binding, name, &has) != napi_ok || has) return;
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, binding, name, fn);
  }
}

}  // namespace

napi_value InitUtil(napi_env env) {
  napi_value binding = EdgeInstallUtilBinding(env);
  if (binding == nullptr || IsUndefined(env, binding)) return Undefined(env);
  UtilBindingState* state = GetOrCreateUtilBindingState(env, binding);

  napi_value types = edge::binding_registry::Get(env, "types");
  if (state != nullptr) {
    DeleteRefIfPresent(env, &state->types_ref);
    if (types != nullptr && !IsUndefined(env, types)) {
      napi_create_reference(env, types, 1, &state->types_ref);
    }
  }

  EnsureMethodFromTypes(env, binding, state, "isAnyArrayBuffer", TypesAlias::kIsAnyArrayBuffer);
  EnsureMethodFromTypes(env, binding, state, "isArrayBuffer", TypesAlias::kIsArrayBuffer);
  EnsureMethodFromTypes(env, binding, state, "isAsyncFunction", TypesAlias::kIsAsyncFunction);
  EnsureMethodFromTypes(env, binding, state, "isDataView", TypesAlias::kIsDataView);
  EnsureMethodFromTypes(env, binding, state, "isDate", TypesAlias::kIsDate);
  EnsureMethodFromTypes(env, binding, state, "isExternal", TypesAlias::kIsExternal);
  EnsureMethodFromTypes(env, binding, state, "isMap", TypesAlias::kIsMap);
  EnsureMethodFromTypes(env, binding, state, "isMapIterator", TypesAlias::kIsMapIterator);
  EnsureMethodFromTypes(env, binding, state, "isNativeError", TypesAlias::kIsNativeError);
  EnsureMethodFromTypes(env, binding, state, "isPromise", TypesAlias::kIsPromise);
  EnsureMethodFromTypes(env, binding, state, "isRegExp", TypesAlias::kIsRegExp);
  EnsureMethodFromTypes(env, binding, state, "isSet", TypesAlias::kIsSet);
  EnsureMethodFromTypes(env, binding, state, "isSetIterator", TypesAlias::kIsSetIterator);

  EnsureMethod(env, binding, "isArrayBufferView", UtilIsArrayBufferView);
  EnsureMethod(env, binding, "isTypedArray", UtilIsTypedArray);
  EnsureMethod(env, binding, "isUint8Array", UtilIsUint8Array);

  return binding;
}

}  // namespace internal_binding
