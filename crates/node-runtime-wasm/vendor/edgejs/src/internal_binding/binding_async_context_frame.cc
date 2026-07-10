#include "internal_binding/binding_initializers.h"

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"

namespace internal_binding {

namespace {

struct AsyncContextFrameBindingState {
  explicit AsyncContextFrameBindingState(napi_env env_in) : env(env_in) {}
  ~AsyncContextFrameBindingState() {
    if (binding_ref != nullptr) {
      napi_delete_reference(env, binding_ref);
      binding_ref = nullptr;
    }
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

AsyncContextFrameBindingState& GetState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<AsyncContextFrameBindingState>(
      env, kEdgeEnvironmentSlotAsyncContextFrameBindingState);
}

napi_value GetContinuationPreservedEmbedderData(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value value = nullptr;
  if (unofficial_napi_get_continuation_preserved_embedder_data(env, &value) != napi_ok ||
      value == nullptr) {
    return Undefined(env);
  }
  return value;
}

napi_value SetContinuationPreservedEmbedderData(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  napi_value value = Undefined(env);
  if (argc >= 1 && argv[0] != nullptr) {
    value = argv[0];
  }
  (void)unofficial_napi_set_continuation_preserved_embedder_data(env, value);
  return Undefined(env);
}

}  // namespace

napi_value InitAsyncContextFrame(napi_env env) {
  AsyncContextFrameBindingState& state = GetState(env);
  if (state.binding_ref != nullptr) {
    napi_value cached = nullptr;
    if (napi_get_reference_value(env, state.binding_ref, &cached) == napi_ok && cached != nullptr) {
      return cached;
    }
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  napi_value getter = nullptr;
  napi_create_function(env,
                       "getContinuationPreservedEmbedderData",
                       NAPI_AUTO_LENGTH,
                       GetContinuationPreservedEmbedderData,
                       nullptr,
                       &getter);
  if (getter != nullptr) {
    napi_set_named_property(env, binding, "getContinuationPreservedEmbedderData", getter);
  }

  napi_value setter = nullptr;
  napi_create_function(env,
                       "setContinuationPreservedEmbedderData",
                       NAPI_AUTO_LENGTH,
                       SetContinuationPreservedEmbedderData,
                       nullptr,
                       &setter);
  if (setter != nullptr) {
    napi_set_named_property(env, binding, "setContinuationPreservedEmbedderData", setter);
  }

  if (state.binding_ref != nullptr) {
    napi_delete_reference(env, state.binding_ref);
    state.binding_ref = nullptr;
  }
  napi_create_reference(env, binding, 1, &state.binding_ref);

  return binding;
}

}  // namespace internal_binding
