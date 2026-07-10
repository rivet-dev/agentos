#include "internal_binding/binding_initializers.h"

#include "edge_environment.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct WasmWebApiState {
  explicit WasmWebApiState(napi_env env_in) : env(env_in) {}
  ~WasmWebApiState() {
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &implementation_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref implementation_ref = nullptr;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

WasmWebApiState* GetWasmWebApiState(napi_env env) {
  return EdgeEnvironmentGetSlotData<WasmWebApiState>(
      env, kEdgeEnvironmentSlotWasmWebApiBindingState);
}

WasmWebApiState& EnsureWasmWebApiState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<WasmWebApiState>(
      env, kEdgeEnvironmentSlotWasmWebApiBindingState);
}

napi_value SetImplementationCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  auto& state = EnsureWasmWebApiState(env);
  DeleteRefIfPresent(env, &state.implementation_ref);

  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      napi_create_reference(env, argv[0], 1, &state.implementation_ref);
    }
  }
  return Undefined(env);
}

napi_value GetCachedWasmWebApi(napi_env env) {
  auto* state = GetWasmWebApiState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

}  // namespace

napi_value InitWasmWebApi(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedWasmWebApi(env);
  if (cached != nullptr) return cached;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  napi_value set_implementation = nullptr;
  if (napi_create_function(env,
                           "setImplementation",
                           NAPI_AUTO_LENGTH,
                           SetImplementationCallback,
                           nullptr,
                           &set_implementation) == napi_ok &&
      set_implementation != nullptr) {
    napi_set_named_property(env, out, "setImplementation", set_implementation);
  }

  auto& state = EnsureWasmWebApiState(env);
  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, out, 1, &state.binding_ref);
  return out;
}

}  // namespace internal_binding
