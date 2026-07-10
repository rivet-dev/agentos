#include "edge_stream_wrap.h"

#include <cstdint>

#include "edge_async_wrap.h"
#include "edge_environment.h"

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct StreamWrapBindingState {
  explicit StreamWrapBindingState(napi_env env_in) : env(env_in) {}
  ~StreamWrapBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
    if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
      DeleteRefIfPresent(env, &environment->stream_base_state()->ref);
      environment->stream_base_state()->fields = nullptr;
    }
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  int32_t* stream_state = nullptr;
};

struct StreamReqWrap {
  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref active_ref = nullptr;
  int64_t async_id = -1;
  int32_t provider_type = kEdgeProviderNone;
  bool destroy_queued = false;
  bool active = false;
};

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value StreamReqGetAsyncId(napi_env env, napi_callback_info info);
napi_value StreamReqGetProviderTypeValue(napi_env env, napi_callback_info info);

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

StreamReqWrap* UnwrapStreamReqWrap(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  StreamReqWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok) return nullptr;
  return wrap;
}

void QueueDestroyIfNeeded(StreamReqWrap* wrap) {
  if (wrap == nullptr || wrap->destroy_queued || wrap->async_id <= 0) return;
  EdgeAsyncWrapQueueDestroyId(wrap->env, wrap->async_id);
  wrap->destroy_queued = true;
}

void StreamReqWrapFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<StreamReqWrap*>(data);
  if (wrap == nullptr) return;
  QueueDestroyIfNeeded(wrap);
  DeleteRefIfPresent(env != nullptr ? env : wrap->env, &wrap->active_ref);
  if (wrap->wrapper_ref != nullptr) napi_delete_reference(env, wrap->wrapper_ref);
  delete wrap;
}

napi_value StreamReqCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  auto* wrap = new StreamReqWrap();
  wrap->env = env;
  napi_wrap(env, self, wrap, StreamReqWrapFinalize, nullptr, &wrap->wrapper_ref);
  return self;
}

bool DefineReqMethods(napi_env env, napi_value target) {
  if (env == nullptr || target == nullptr) return false;
  napi_property_descriptor req_props[] = {
      {"getAsyncId", nullptr, StreamReqGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType",
       nullptr,
       StreamReqGetProviderTypeValue,
       nullptr,
       nullptr,
       nullptr,
       napi_default_method,
       nullptr},
  };
  return napi_define_properties(env, target, sizeof(req_props) / sizeof(req_props[0]), req_props) == napi_ok;
}

bool DefineReqPrototypeMethods(napi_env env, napi_value ctor) {
  if (env == nullptr || ctor == nullptr) return false;
  napi_value prototype = nullptr;
  if (napi_get_named_property(env, ctor, "prototype", &prototype) != napi_ok || prototype == nullptr) {
    return false;
  }
  return DefineReqMethods(env, prototype);
}

napi_value StreamReqGetAsyncId(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, self);
  napi_value out = nullptr;
  napi_create_int64(env, wrap != nullptr ? wrap->async_id : -1, &out);
  return out;
}

napi_value StreamReqGetProviderTypeValue(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr);
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, self);
  napi_value out = nullptr;
  napi_create_int32(env, wrap != nullptr ? wrap->provider_type : kEdgeProviderNone, &out);
  return out;
}

void SetNamedU32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value v = nullptr;
  if (napi_create_uint32(env, value, &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, key, v);
  }
}

StreamWrapBindingState& EnsureBindingState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<StreamWrapBindingState>(
      env, kEdgeEnvironmentSlotStreamWrapBindingState);
}

}  // namespace

int32_t* EdgeGetStreamBaseState(napi_env env) {
  if (env == nullptr) return nullptr;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    if (environment->stream_base_state()->fields != nullptr) {
      return environment->stream_base_state()->fields;
    }
  }
  auto* state = EdgeEnvironmentGetSlotData<StreamWrapBindingState>(
      env, kEdgeEnvironmentSlotStreamWrapBindingState);
  return state != nullptr ? state->stream_state : nullptr;
}

napi_value EdgeCreateStreamReqObject(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value req = nullptr;
  if (napi_create_object(env, &req) != napi_ok || req == nullptr) return nullptr;
  if (!DefineReqMethods(env, req)) return nullptr;
  auto* wrap = new StreamReqWrap();
  wrap->env = env;
  if (napi_wrap(env, req, wrap, StreamReqWrapFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  return req;
}

int64_t EdgeStreamReqGetAsyncId(napi_env env, napi_value req_obj) {
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, req_obj);
  return wrap != nullptr ? wrap->async_id : -1;
}

int32_t EdgeStreamReqGetProviderType(napi_env env, napi_value req_obj) {
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, req_obj);
  return wrap != nullptr ? wrap->provider_type : kEdgeProviderNone;
}

void EdgeStreamReqActivate(napi_env env,
                          napi_value req_obj,
                          int32_t provider_type,
                          int64_t trigger_async_id) {
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, req_obj);
  if (wrap == nullptr) return;

  wrap->provider_type = provider_type;
  if (wrap->async_id <= 0 || wrap->destroy_queued) {
    wrap->async_id = EdgeAsyncWrapNextId(env);
  } else {
    EdgeAsyncWrapReset(env, &wrap->async_id);
  }
  wrap->destroy_queued = false;
  wrap->active = true;

  DeleteRefIfPresent(env, &wrap->active_ref);
  if (req_obj != nullptr) napi_create_reference(env, req_obj, 1, &wrap->active_ref);
  EdgeAsyncWrapEmitInit(env, wrap->async_id, provider_type, trigger_async_id, req_obj);
}

void EdgeStreamReqMarkDone(napi_env env, napi_value req_obj) {
  StreamReqWrap* wrap = UnwrapStreamReqWrap(env, req_obj);
  if (wrap == nullptr) return;
  QueueDestroyIfNeeded(wrap);
  DeleteRefIfPresent(env, &wrap->active_ref);
  wrap->active = false;
}

napi_value EdgeInstallStreamWrapBinding(napi_env env) {
  StreamWrapBindingState& state = EnsureBindingState(env);
  if (state.binding_ref != nullptr) {
    napi_value cached = GetRefValue(env, state.binding_ref);
    if (cached != nullptr) return cached;
    DeleteRefIfPresent(env, &state.binding_ref);
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_value write_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "WriteWrap",
                        NAPI_AUTO_LENGTH,
                        StreamReqCtor,
                        nullptr,
                        0,
                        nullptr,
                        &write_wrap_ctor) != napi_ok ||
      write_wrap_ctor == nullptr) {
    return nullptr;
  }
  if (!DefineReqPrototypeMethods(env, write_wrap_ctor)) return nullptr;

  napi_value shutdown_wrap_ctor = nullptr;
  if (napi_define_class(env,
                        "ShutdownWrap",
                        NAPI_AUTO_LENGTH,
                        StreamReqCtor,
                        nullptr,
                        0,
                        nullptr,
                        &shutdown_wrap_ctor) != napi_ok ||
      shutdown_wrap_ctor == nullptr) {
    return nullptr;
  }
  if (!DefineReqPrototypeMethods(env, shutdown_wrap_ctor)) return nullptr;

  void* state_data = nullptr;
  napi_value state_ab = nullptr;
  if (napi_create_arraybuffer(env, sizeof(int32_t) * kEdgeStreamStateLength, &state_data, &state_ab) != napi_ok ||
      state_ab == nullptr || state_data == nullptr) {
    return nullptr;
  }
  int32_t* stream_state_data = static_cast<int32_t*>(state_data);
  for (int i = 0; i < kEdgeStreamStateLength; i++) stream_state_data[i] = 0;

  napi_value stream_state = nullptr;
  if (napi_create_typedarray(env,
                             napi_int32_array,
                             kEdgeStreamStateLength,
                             state_ab,
                             0,
                             &stream_state) != napi_ok ||
      stream_state == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "WriteWrap", write_wrap_ctor);
  napi_set_named_property(env, binding, "ShutdownWrap", shutdown_wrap_ctor);
  napi_set_named_property(env, binding, "streamBaseState", stream_state);

  SetNamedU32(env, binding, "kReadBytesOrError", kEdgeReadBytesOrError);
  SetNamedU32(env, binding, "kArrayBufferOffset", kEdgeArrayBufferOffset);
  SetNamedU32(env, binding, "kBytesWritten", kEdgeBytesWritten);
  SetNamedU32(env, binding, "kLastWriteWasAsync", kEdgeLastWriteWasAsync);

  state.stream_state = stream_state_data;
  DeleteRefIfPresent(env, &state.binding_ref);
  napi_create_reference(env, binding, 1, &state.binding_ref);
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->stream_base_state()->fields = stream_state_data;
    DeleteRefIfPresent(env, &environment->stream_base_state()->ref);
    napi_create_reference(env, stream_state, 1, &environment->stream_base_state()->ref);
  }

  return binding;
}
