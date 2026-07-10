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

struct MksnapshotBindingState {
  explicit MksnapshotBindingState(napi_env env_in) : env(env_in) {}
  ~MksnapshotBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
};

MksnapshotBindingState* GetMksnapshotState(napi_env env) {
  return EdgeEnvironmentGetSlotData<MksnapshotBindingState>(
      env, kEdgeEnvironmentSlotMksnapshotBindingState);
}

MksnapshotBindingState& EnsureMksnapshotState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<MksnapshotBindingState>(
      env, kEdgeEnvironmentSlotMksnapshotBindingState);
}

napi_value ReturnUndefined(napi_env env, napi_callback_info /*info*/) {
  return Undefined(env);
}

napi_value GetCachedMksnapshot(napi_env env) {
  auto* state = GetMksnapshotState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

}  // namespace

napi_value InitMksnapshot(napi_env env) {
  const napi_value undefined = Undefined(env);
  napi_value cached = GetCachedMksnapshot(env);
  if (cached != nullptr) return cached;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  auto define_noop = [&](const char* name) {
    napi_value fn = nullptr;
    if (napi_create_function(env, name, NAPI_AUTO_LENGTH, ReturnUndefined, nullptr, &fn) == napi_ok &&
        fn != nullptr) {
      napi_set_named_property(env, out, name, fn);
    }
  };
  define_noop("runEmbedderPreload");
  define_noop("compileSerializeMain");
  define_noop("setSerializeCallback");
  define_noop("setDeserializeCallback");
  define_noop("setDeserializeMainFunction");

  void* data = nullptr;
  napi_value ab = nullptr;
  napi_value is_building_snapshot_buffer = nullptr;
  if (napi_create_arraybuffer(env, 1, &data, &ab) == napi_ok && data != nullptr && ab != nullptr) {
    static_cast<uint8_t*>(data)[0] = 0;
    if (napi_create_typedarray(env, napi_uint8_array, 1, ab, 0, &is_building_snapshot_buffer) == napi_ok &&
        is_building_snapshot_buffer != nullptr) {
      napi_set_named_property(env, out, "isBuildingSnapshotBuffer", is_building_snapshot_buffer);
    }
  }

  SetString(env, out, "anonymousMainPath", "<anonymous>");

  auto& state = EnsureMksnapshotState(env);
  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, out, 1, &state.binding_ref);
  return out;
}

}  // namespace internal_binding
