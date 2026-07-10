#include "internal_binding/binding_initializers.h"

#include "edge_environment.h"
#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

struct PermissionBindingState {
  explicit PermissionBindingState(napi_env env_in) : env(env_in) {}
  ~PermissionBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

PermissionBindingState* GetPermissionState(napi_env env) {
  return EdgeEnvironmentGetSlotData<PermissionBindingState>(
      env, kEdgeEnvironmentSlotPermissionBindingState);
}

PermissionBindingState& EnsurePermissionState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<PermissionBindingState>(
      env, kEdgeEnvironmentSlotPermissionBindingState);
}

napi_value HasPermissionCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value GetCachedPermission(napi_env env) {
  auto* state = GetPermissionState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

}  // namespace

napi_value InitPermission(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedPermission(env);
  if (cached != nullptr) return cached;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  napi_value has_fn = nullptr;
  if (napi_create_function(env,
                           "has",
                           NAPI_AUTO_LENGTH,
                           HasPermissionCallback,
                           nullptr,
                           &has_fn) == napi_ok &&
      has_fn != nullptr) {
    napi_set_named_property(env, out, "has", has_fn);
  }

  auto& state = EnsurePermissionState(env);
  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, out, 1, &state.binding_ref);
  return out;
}

}  // namespace internal_binding
