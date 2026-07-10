#ifndef EDGE_INTERNAL_BINDING_HELPERS_H_
#define EDGE_INTERNAL_BINDING_HELPERS_H_

#include "node_api.h"

namespace internal_binding {

inline napi_value Undefined(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

inline napi_value GetGlobal(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return nullptr;
  }
  return global;
}

inline napi_value GetGlobalNamed(napi_env env, const char* key) {
  napi_value global = GetGlobal(env);
  if (global == nullptr) return Undefined(env);
  bool has_prop = false;
  if (napi_has_named_property(env, global, key, &has_prop) != napi_ok || !has_prop) {
    return Undefined(env);
  }
  napi_value out = nullptr;
  if (napi_get_named_property(env, global, key, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

inline bool IsUndefined(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_undefined;
}

inline bool SetInt32(napi_env env, napi_value obj, const char* key, int32_t value) {
  napi_value v = nullptr;
  return napi_create_int32(env, value, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, obj, key, v) == napi_ok;
}

inline bool SetBool(napi_env env, napi_value obj, const char* key, bool value) {
  napi_value v = nullptr;
  return napi_get_boolean(env, value, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, obj, key, v) == napi_ok;
}

inline bool SetString(napi_env env, napi_value obj, const char* key, const char* value) {
  napi_value v = nullptr;
  return napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, obj, key, v) == napi_ok;
}

}  // namespace internal_binding

#endif  // EDGE_INTERNAL_BINDING_HELPERS_H_
