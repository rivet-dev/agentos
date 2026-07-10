#include "internal_binding/binding_initializers.h"

namespace internal_binding {

namespace {

napi_value SeaIsSea(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out;
}

napi_value SeaGetAsset(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value SeaGetAssetKeys(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}

}  // namespace

napi_value InitSea(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  auto define_method = [&](const char* name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    return napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
           fn != nullptr &&
           napi_set_named_property(env, out, name, fn) == napi_ok;
  };

  if (!define_method("isSea", SeaIsSea) ||
      !define_method("getAsset", SeaGetAsset) ||
      !define_method("getAssetKeys", SeaGetAssetKeys)) {
    return nullptr;
  }

  return out;
}

}  // namespace internal_binding
