#include "internal_binding/binding_initializers.h"

#include "unofficial_napi.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

napi_value InitSerdes(napi_env env) {
  napi_value out = nullptr;
  if (unofficial_napi_create_serdes_binding(env, &out) != napi_ok || out == nullptr) {
    return Undefined(env);
  }
  return out;
}

}  // namespace internal_binding

