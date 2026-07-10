#include "internal_binding/binding_initializers.h"

#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

napi_value InternalOnlyV8QueryObjects(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_array_with_length(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
}

}  // namespace

napi_value InitInternalOnlyV8(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  napi_value query_objects = nullptr;
  if (napi_create_function(env,
                           "queryObjects",
                           NAPI_AUTO_LENGTH,
                           InternalOnlyV8QueryObjects,
                           nullptr,
                           &query_objects) == napi_ok &&
      query_objects != nullptr) {
    napi_set_named_property(env, out, "queryObjects", query_objects);
  }

  return out;
}

}  // namespace internal_binding

