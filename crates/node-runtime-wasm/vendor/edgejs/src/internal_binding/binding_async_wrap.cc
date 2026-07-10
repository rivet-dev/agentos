#include "internal_binding/binding_initializers.h"
#include "internal_binding/binding_async_wrap.h"

#include <array>
#include <cstdint>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "edge_environment.h"
#include "edge_handle_scope.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"
#include "edge_runtime_platform.h"

namespace internal_binding {

namespace {

void ResetRef(napi_env env, napi_ref* ref);

enum AsyncWrapConstants : uint32_t {
  kInit = 0,
  kBefore = 1,
  kAfter = 2,
  kDestroy = 3,
  kPromiseResolve = 4,
  kTotals = 5,
  kCheck = 6,
  kStackLength = 7,
  kUsesExecutionAsyncResource = 8,
  kExecutionAsyncId = 0,
  kTriggerAsyncId = 1,
  kAsyncIdCounter = 2,
  kDefaultTriggerAsyncId = 3,
};

struct AsyncWrapState {
  explicit AsyncWrapState(napi_env env_in) : env(env_in) {}
  ~AsyncWrapState() {
    ResetRef(env, &binding_ref);
    ResetRef(env, &async_hook_fields_ref);
    ResetRef(env, &async_id_fields_ref);
    ResetRef(env, &async_ids_stack_ref);
    ResetRef(env, &execution_async_resources_ref);
    ResetRef(env, &hooks_ref);
    ResetRef(env, &callback_trampoline_ref);
    for (napi_ref& hook_ref : promise_hooks) {
      ResetRef(env, &hook_ref);
    }
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref async_hook_fields_ref = nullptr;
  napi_ref async_id_fields_ref = nullptr;
  napi_ref async_ids_stack_ref = nullptr;
  napi_ref execution_async_resources_ref = nullptr;
  napi_ref hooks_ref = nullptr;
  napi_ref callback_trampoline_ref = nullptr;
  std::array<napi_ref, 4> promise_hooks = {nullptr, nullptr, nullptr, nullptr};
  std::vector<double> overflow_stack;
  std::vector<double> queued_destroy_async_ids;
  bool destroy_task_scheduled = false;
};

struct DestroyHookData {
  napi_env env = nullptr;
  double async_id = 0;
  napi_ref destroyed_ref = nullptr;
};

void ResetRef(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

AsyncWrapState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<AsyncWrapState>(
      env, kEdgeEnvironmentSlotInternalAsyncWrapBindingState);
}

AsyncWrapState& EnsureState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<AsyncWrapState>(
      env, kEdgeEnvironmentSlotInternalAsyncWrapBindingState);
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

bool IsDestroyHookAlreadyHandled(napi_env env, napi_ref destroyed_ref) {
  if (env == nullptr || destroyed_ref == nullptr) return false;
  edge::HandleScope scope(env);
  napi_value destroyed_obj = GetRefValue(env, destroyed_ref);
  if (destroyed_obj == nullptr) return false;
  bool has_flag = false;
  if (napi_has_named_property(env, destroyed_obj, "destroyed", &has_flag) != napi_ok || !has_flag) return false;
  napi_value flag = nullptr;
  if (napi_get_named_property(env, destroyed_obj, "destroyed", &flag) != napi_ok || flag == nullptr) return false;
  bool handled = false;
  napi_get_value_bool(env, flag, &handled);
  return handled;
}

void EmitDestroyHookForAsyncId(napi_env env, double async_id) {
  edge::HandleScope scope(env);
  AsyncWrapState* state = GetState(env);
  if (state == nullptr || state->hooks_ref == nullptr) return;
  napi_value hooks = GetRefValue(env, state->hooks_ref);
  if (hooks == nullptr) return;
  napi_value destroy_fn = nullptr;
  if (napi_get_named_property(env, hooks, "destroy", &destroy_fn) != napi_ok || destroy_fn == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, destroy_fn, &type) != napi_ok || type != napi_function) return;
  napi_value async_id_value = nullptr;
  if (napi_create_double(env, async_id, &async_id_value) != napi_ok || async_id_value == nullptr) return;
  napi_value ignored = nullptr;
  napi_call_function(env, hooks, destroy_fn, 1, &async_id_value, &ignored);
}

void DrainQueuedDestroyHooks(napi_env env, void* /*data*/) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return;

  while (!state->queued_destroy_async_ids.empty()) {
    std::vector<double> destroy_async_ids;
    destroy_async_ids.swap(state->queued_destroy_async_ids);
    for (double async_id : destroy_async_ids) {
      EmitDestroyHookForAsyncId(env, async_id);
    }
  }

  state->destroy_task_scheduled = false;
}

void QueueDestroyHookForAsyncId(napi_env env, double async_id) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr || async_id <= 0) return;

  state->queued_destroy_async_ids.push_back(async_id);
  if (state->destroy_task_scheduled) return;

  state->destroy_task_scheduled = true;
  if (EdgeRuntimePlatformEnqueueTask(
          env,
          DrainQueuedDestroyHooks,
          nullptr,
          nullptr,
          kEdgeRuntimePlatformTaskNone) == napi_ok) {
    return;
  }

  state->destroy_task_scheduled = false;
  DrainQueuedDestroyHooks(env, nullptr);
}

void DestroyHookFinalizer(napi_env env, void* data, void* /*hint*/) {
  auto* hook = static_cast<DestroyHookData*>(data);
  if (hook == nullptr) return;
  if (env != nullptr) {
    if (!IsDestroyHookAlreadyHandled(env, hook->destroyed_ref)) {
      QueueDestroyHookForAsyncId(env, hook->async_id);
    }
    if (hook->destroyed_ref != nullptr) {
      napi_delete_reference(env, hook->destroyed_ref);
      hook->destroyed_ref = nullptr;
    }
  }
  delete hook;
}

bool GetTypedArrayView(napi_env env,
                       napi_ref ref,
                       napi_typedarray_type expected_type,
                       size_t min_length,
                       void** data,
                       size_t* length) {
  if (data == nullptr || length == nullptr) return false;
  *data = nullptr;
  *length = 0;
  napi_value value = GetRefValue(env, ref);
  if (value == nullptr) return false;
  bool is_typed_array = false;
  if (napi_is_typedarray(env, value, &is_typed_array) != napi_ok || !is_typed_array) return false;
  napi_typedarray_type type = napi_uint8_array;
  napi_value arraybuffer = nullptr;
  size_t offset = 0;
  if (napi_get_typedarray_info(env, value, &type, length, data, &arraybuffer, &offset) != napi_ok ||
      *data == nullptr || type != expected_type || *length < min_length) {
    return false;
  }
  return true;
}

uint32_t* HookFields(napi_env env, AsyncWrapState* state) {
  void* data = nullptr;
  size_t length = 0;
  if (state == nullptr || !GetTypedArrayView(env, state->async_hook_fields_ref, napi_uint32_array, 9, &data, &length)) {
    return nullptr;
  }
  return static_cast<uint32_t*>(data);
}

double* IdFields(napi_env env, AsyncWrapState* state) {
  void* data = nullptr;
  size_t length = 0;
  if (state == nullptr || !GetTypedArrayView(env, state->async_id_fields_ref, napi_float64_array, 4, &data, &length)) {
    return nullptr;
  }
  return static_cast<double*>(data);
}

double* IdStack(napi_env env, AsyncWrapState* state, size_t* capacity_pairs) {
  void* data = nullptr;
  size_t length = 0;
  if (capacity_pairs != nullptr) *capacity_pairs = 0;
  if (state == nullptr || !GetTypedArrayView(env, state->async_ids_stack_ref, napi_float64_array, 2, &data, &length)) {
    return nullptr;
  }
  if (capacity_pairs != nullptr) *capacity_pairs = length / 2;
  return static_cast<double*>(data);
}

napi_value GetExecutionResources(napi_env env, AsyncWrapState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->execution_async_resources_ref);
}

void SetPromiseHookRef(napi_env env, AsyncWrapState* state, size_t index, napi_value value) {
  if (state == nullptr || index >= state->promise_hooks.size()) return;
  if (state->promise_hooks[index] != nullptr) {
    napi_delete_reference(env, state->promise_hooks[index]);
    state->promise_hooks[index] = nullptr;
  }
  if (value == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_function) return;
  napi_create_reference(env, value, 1, &state->promise_hooks[index]);
}

napi_value ResolvePromiseHookValue(napi_env env, AsyncWrapState* state, size_t index) {
  if (state == nullptr || index >= state->promise_hooks.size()) return Undefined(env);
  napi_value value = GetRefValue(env, state->promise_hooks[index]);
  return value != nullptr ? value : Undefined(env);
}

napi_value AsyncWrapSetupHooks(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return Undefined(env);

  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  if (state->hooks_ref != nullptr) {
    napi_delete_reference(env, state->hooks_ref);
    state->hooks_ref = nullptr;
  }
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_object) {
      napi_create_reference(env, argv[0], 1, &state->hooks_ref);
    }
  }
  return Undefined(env);
}

napi_value AsyncWrapSetCallbackTrampoline(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return Undefined(env);

  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  if (state->callback_trampoline_ref != nullptr) {
    napi_delete_reference(env, state->callback_trampoline_ref);
    state->callback_trampoline_ref = nullptr;
  }
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      napi_create_reference(env, argv[0], 1, &state->callback_trampoline_ref);
    }
  }
  return Undefined(env);
}

void PushAsyncContextImpl(napi_env env,
                          AsyncWrapState* state,
                          double async_id,
                          double trigger_id,
                          napi_value resource) {
  uint32_t* fields = HookFields(env, state);
  double* ids = IdFields(env, state);
  size_t capacity_pairs = 0;
  double* stack = IdStack(env, state, &capacity_pairs);
  if (state == nullptr || fields == nullptr || ids == nullptr || stack == nullptr) return;

  const uint32_t offset = fields[kStackLength];
  if (resource != nullptr) {
    napi_value resources = GetExecutionResources(env, state);
    if (resources != nullptr) {
      napi_set_element(env, resources, offset, resource);
    }
  }

  if (offset < capacity_pairs) {
    stack[offset * 2] = ids[kExecutionAsyncId];
    stack[offset * 2 + 1] = ids[kTriggerAsyncId];
  } else {
    const size_t base = static_cast<size_t>(offset - capacity_pairs) * 2;
    if (state->overflow_stack.size() < base + 2) state->overflow_stack.resize(base + 2, 0);
    state->overflow_stack[base] = ids[kExecutionAsyncId];
    state->overflow_stack[base + 1] = ids[kTriggerAsyncId];
  }

  fields[kStackLength] = offset + 1;
  ids[kExecutionAsyncId] = async_id;
  ids[kTriggerAsyncId] = trigger_id;
}

bool PopAsyncContextImpl(napi_env env, AsyncWrapState* state, double async_id) {
  uint32_t* fields = HookFields(env, state);
  double* ids = IdFields(env, state);
  size_t capacity_pairs = 0;
  double* stack = IdStack(env, state, &capacity_pairs);
  if (state == nullptr || fields == nullptr || ids == nullptr || stack == nullptr) return false;

  const uint32_t stack_length = fields[kStackLength];
  bool result = false;
  if (stack_length != 0) {
    const uint32_t check = fields[kCheck];
    if (!(check > 0 && ids[kExecutionAsyncId] != async_id)) {
      const uint32_t offset = stack_length - 1;
      if (offset < capacity_pairs) {
        ids[kExecutionAsyncId] = stack[offset * 2];
        ids[kTriggerAsyncId] = stack[offset * 2 + 1];
      } else {
        const size_t base = static_cast<size_t>(offset - capacity_pairs) * 2;
        if (state->overflow_stack.size() >= base + 2) {
          ids[kExecutionAsyncId] = state->overflow_stack[base];
          ids[kTriggerAsyncId] = state->overflow_stack[base + 1];
          state->overflow_stack.resize(base);
        } else {
          ids[kExecutionAsyncId] = 0;
          ids[kTriggerAsyncId] = 0;
        }
      }
      fields[kStackLength] = offset;
      result = offset > 0;

      napi_value resources = GetExecutionResources(env, state);
      if (resources != nullptr) {
        napi_value pop_fn = nullptr;
        napi_valuetype type = napi_undefined;
        if (napi_get_named_property(env, resources, "pop", &pop_fn) == napi_ok &&
            pop_fn != nullptr &&
            napi_typeof(env, pop_fn, &type) == napi_ok &&
            type == napi_function) {
          napi_value ignored = nullptr;
          napi_call_function(env, resources, pop_fn, 0, nullptr, &ignored);
        }
      }
    }
  }
  return result;
}

napi_value AsyncWrapPushAsyncContext(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return Undefined(env);

  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  double async_id = 0;
  double trigger_id = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_double(env, argv[0], &async_id);
  if (argc >= 2 && argv[1] != nullptr) napi_get_value_double(env, argv[1], &trigger_id);
  PushAsyncContextImpl(env, state, async_id, trigger_id, nullptr);
  return Undefined(env);
}

napi_value AsyncWrapPopAsyncContext(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return Undefined(env);

  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  double async_id = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_double(env, argv[0], &async_id);

  bool result = PopAsyncContextImpl(env, state, async_id);
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value AsyncWrapExecutionAsyncResource(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  napi_value resources = GetExecutionResources(env, state);
  if (resources == nullptr) return Undefined(env);

  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  uint32_t index = 0;
  if (argc >= 1 && argv[0] != nullptr) napi_get_value_uint32(env, argv[0], &index);

  napi_value out = nullptr;
  if (napi_get_element(env, resources, index, &out) != napi_ok || out == nullptr) return Undefined(env);
  return out;
}

napi_value AsyncWrapClearAsyncIdStack(napi_env env, napi_callback_info /*info*/) {
  AsyncWrapState* state = GetState(env);
  uint32_t* fields = HookFields(env, state);
  double* ids = IdFields(env, state);
  if (state == nullptr || fields == nullptr || ids == nullptr) return Undefined(env);

  ids[kExecutionAsyncId] = 0;
  ids[kTriggerAsyncId] = 0;
  fields[kStackLength] = 0;
  state->overflow_stack.clear();

  napi_value resources = GetExecutionResources(env, state);
  if (resources != nullptr) {
    napi_value zero = nullptr;
    if (napi_create_uint32(env, 0, &zero) == napi_ok && zero != nullptr) {
      napi_set_named_property(env, resources, "length", zero);
    }
  }
  return Undefined(env);
}

napi_value AsyncWrapQueueDestroyAsyncId(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  double async_id = 0;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_get_value_double(env, argv[0], &async_id);
  }
  QueueDestroyHookForAsyncId(env, async_id);
  return Undefined(env);
}

napi_value AsyncWrapSetPromiseHooks(napi_env env, napi_callback_info info) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return Undefined(env);
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  for (size_t i = 0; i < state->promise_hooks.size(); ++i) {
    SetPromiseHookRef(env, state, i, i < argc ? argv[i] : nullptr);
  }
  (void)unofficial_napi_set_promise_hooks(env,
                                          argc > 0 ? argv[0] : nullptr,
                                          argc > 1 ? argv[1] : nullptr,
                                          argc > 2 ? argv[2] : nullptr,
                                          argc > 3 ? argv[3] : nullptr);
  return Undefined(env);
}

napi_value AsyncWrapGetPromiseHooks(napi_env env, napi_callback_info /*info*/) {
  AsyncWrapState* state = GetState(env);
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 4, &out) != napi_ok || out == nullptr) return Undefined(env);
  for (size_t i = 0; i < 4; ++i) {
    napi_set_element(env, out, static_cast<uint32_t>(i), ResolvePromiseHookValue(env, state, i));
  }
  return out;
}

napi_value AsyncWrapRegisterDestroyHook(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) return Undefined(env);

  napi_valuetype target_type = napi_undefined;
  if (napi_typeof(env, argv[0], &target_type) != napi_ok ||
      (target_type != napi_object && target_type != napi_function)) {
    return Undefined(env);
  }

  double async_id = 0;
  if (napi_get_value_double(env, argv[1], &async_id) != napi_ok) return Undefined(env);

  auto* hook = new DestroyHookData();
  hook->env = env;
  hook->async_id = async_id;

  if (argc >= 3 && argv[2] != nullptr) {
    napi_valuetype destroyed_type = napi_undefined;
    if (napi_typeof(env, argv[2], &destroyed_type) == napi_ok &&
        (destroyed_type == napi_object || destroyed_type == napi_function)) {
      napi_create_reference(env, argv[2], 1, &hook->destroyed_ref);
    }
  }

  if (napi_add_finalizer(env, argv[0], hook, DestroyHookFinalizer, nullptr, nullptr) != napi_ok) {
    if (hook->destroyed_ref != nullptr) napi_delete_reference(env, hook->destroyed_ref);
    delete hook;
  }
  return Undefined(env);
}

void SetNamedUint32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value out = nullptr;
  if (napi_create_uint32(env, value, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, obj, key, out);
  }
}

void SetNamedMethod(napi_env env, napi_value obj, const char* key, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, key, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, key, fn);
  }
}

napi_value CreateTypedArray(napi_env env, napi_typedarray_type type, size_t length, void** out_data = nullptr) {
  napi_value arraybuffer = nullptr;
  void* data = nullptr;
  size_t byte_length = 0;
  switch (type) {
    case napi_uint32_array:
      byte_length = sizeof(uint32_t) * length;
      break;
    case napi_float64_array:
      byte_length = sizeof(double) * length;
      break;
    default:
      return nullptr;
  }
  if (napi_create_arraybuffer(env, byte_length, &data, &arraybuffer) != napi_ok || arraybuffer == nullptr ||
      data == nullptr) {
    return nullptr;
  }
  napi_value typed_array = nullptr;
  if (napi_create_typedarray(env, type, length, arraybuffer, 0, &typed_array) != napi_ok || typed_array == nullptr) {
    return nullptr;
  }
  if (out_data != nullptr) *out_data = data;
  return typed_array;
}

napi_value CreateProviders(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;
  SetNamedUint32(env, out, "NONE", 0);
  SetNamedUint32(env, out, "JSSTREAM", 20);
  SetNamedUint32(env, out, "JSUDPWRAP", 21);
  SetNamedUint32(env, out, "MESSAGEPORT", 22);
  SetNamedUint32(env, out, "PIPECONNECTWRAP", 23);
  SetNamedUint32(env, out, "PIPESERVERWRAP", 24);
  SetNamedUint32(env, out, "PIPEWRAP", 25);
  SetNamedUint32(env, out, "PROCESSWRAP", 26);
  SetNamedUint32(env, out, "SHUTDOWNWRAP", 35);
  SetNamedUint32(env, out, "TCPCONNECTWRAP", 39);
  SetNamedUint32(env, out, "TCPSERVERWRAP", 40);
  SetNamedUint32(env, out, "PROMISE", 27);
  SetNamedUint32(env, out, "TCPWRAP", 41);
  SetNamedUint32(env, out, "TTYWRAP", 42);
  SetNamedUint32(env, out, "UDPSENDWRAP", 43);
  SetNamedUint32(env, out, "UDPWRAP", 44);
  SetNamedUint32(env, out, "WRITEWRAP", 52);
  SetNamedUint32(env, out, "ZLIB", 53);
  SetNamedUint32(env, out, "TLSWRAP", 55);
  return out;
}

}  // namespace

napi_value AsyncWrapGetHooksObject(napi_env env) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->hooks_ref);
}

napi_value AsyncWrapGetCallbackTrampoline(napi_env env) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->callback_trampoline_ref);
}

void AsyncWrapQueueDestroyId(napi_env env, double async_id) {
  QueueDestroyHookForAsyncId(env, async_id);
}

void AsyncWrapPushContext(napi_env env,
                          double async_id,
                          double trigger_async_id,
                          napi_value resource) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return;
  PushAsyncContextImpl(env, state, async_id, trigger_async_id, resource);
}

bool AsyncWrapPopContext(napi_env env, double async_id) {
  AsyncWrapState* state = GetState(env);
  if (state == nullptr) return false;
  return PopAsyncContextImpl(env, state, async_id);
}

napi_value InitAsyncWrap(napi_env env) {
  auto& state = EnsureState(env);
  if (state.binding_ref != nullptr) {
    napi_value existing = GetRefValue(env, state.binding_ref);
    if (existing != nullptr) return existing;
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  napi_value constants = nullptr;
  if (napi_create_object(env, &constants) == napi_ok && constants != nullptr) {
    SetNamedUint32(env, constants, "kInit", kInit);
    SetNamedUint32(env, constants, "kBefore", kBefore);
    SetNamedUint32(env, constants, "kAfter", kAfter);
    SetNamedUint32(env, constants, "kDestroy", kDestroy);
    SetNamedUint32(env, constants, "kPromiseResolve", kPromiseResolve);
    SetNamedUint32(env, constants, "kTotals", kTotals);
    SetNamedUint32(env, constants, "kCheck", kCheck);
    SetNamedUint32(env, constants, "kStackLength", kStackLength);
    SetNamedUint32(env, constants, "kUsesExecutionAsyncResource", kUsesExecutionAsyncResource);
    SetNamedUint32(env, constants, "kExecutionAsyncId", kExecutionAsyncId);
    SetNamedUint32(env, constants, "kTriggerAsyncId", kTriggerAsyncId);
    SetNamedUint32(env, constants, "kAsyncIdCounter", kAsyncIdCounter);
    SetNamedUint32(env, constants, "kDefaultTriggerAsyncId", kDefaultTriggerAsyncId);
    napi_set_named_property(env, binding, "constants", constants);
  }

  napi_value providers = CreateProviders(env);
  if (providers != nullptr) napi_set_named_property(env, binding, "Providers", providers);

  void* hook_data = nullptr;
  napi_value async_hook_fields = CreateTypedArray(env, napi_uint32_array, 9, &hook_data);
  if (async_hook_fields != nullptr) {
    auto* values = static_cast<uint32_t*>(hook_data);
    for (size_t i = 0; i < 9; ++i) values[i] = 0;
    values[kCheck] = 1;
    napi_set_named_property(env, binding, "async_hook_fields", async_hook_fields);
    napi_create_reference(env, async_hook_fields, 1, &state.async_hook_fields_ref);
  }

  void* id_data = nullptr;
  napi_value async_id_fields = CreateTypedArray(env, napi_float64_array, 4, &id_data);
  if (async_id_fields != nullptr) {
    auto* values = static_cast<double*>(id_data);
    for (size_t i = 0; i < 4; ++i) values[i] = 0;
    values[kExecutionAsyncId] = 0;
    values[kTriggerAsyncId] = 0;
    values[kAsyncIdCounter] = 1;
    values[kDefaultTriggerAsyncId] = -1;
    napi_set_named_property(env, binding, "async_id_fields", async_id_fields);
    napi_create_reference(env, async_id_fields, 1, &state.async_id_fields_ref);
  }

  void* stack_data = nullptr;
  napi_value async_ids_stack = CreateTypedArray(env, napi_float64_array, 32, &stack_data);
  if (async_ids_stack != nullptr) {
    auto* values = static_cast<double*>(stack_data);
    for (size_t i = 0; i < 32; ++i) values[i] = 0;
    napi_set_named_property(env, binding, "async_ids_stack", async_ids_stack);
    napi_create_reference(env, async_ids_stack, 1, &state.async_ids_stack_ref);
  }

  napi_value resources = nullptr;
  if (napi_create_array_with_length(env, 0, &resources) == napi_ok && resources != nullptr) {
    napi_set_named_property(env, binding, "execution_async_resources", resources);
    napi_create_reference(env, resources, 1, &state.execution_async_resources_ref);
  }

  SetNamedMethod(env, binding, "setupHooks", AsyncWrapSetupHooks);
  SetNamedMethod(env, binding, "setCallbackTrampoline", AsyncWrapSetCallbackTrampoline);
  SetNamedMethod(env, binding, "pushAsyncContext", AsyncWrapPushAsyncContext);
  SetNamedMethod(env, binding, "popAsyncContext", AsyncWrapPopAsyncContext);
  SetNamedMethod(env, binding, "executionAsyncResource", AsyncWrapExecutionAsyncResource);
  SetNamedMethod(env, binding, "clearAsyncIdStack", AsyncWrapClearAsyncIdStack);
  SetNamedMethod(env, binding, "queueDestroyAsyncId", AsyncWrapQueueDestroyAsyncId);
  SetNamedMethod(env, binding, "setPromiseHooks", AsyncWrapSetPromiseHooks);
  SetNamedMethod(env, binding, "getPromiseHooks", AsyncWrapGetPromiseHooks);
  SetNamedMethod(env, binding, "registerDestroyHook", AsyncWrapRegisterDestroyHook);

  napi_create_reference(env, binding, 1, &state.binding_ref);
  return binding;
}

}  // namespace internal_binding
