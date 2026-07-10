#include "edge_task_queue.h"

#include "edge_environment.h"
#include "edge_handle_scope.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"

namespace {

void DeleteRefIfAny(napi_env env, napi_ref* ref_slot);

struct TaskQueueBindingState {
  explicit TaskQueueBindingState(napi_env env_in) : env(env_in) {}
  ~TaskQueueBindingState() {
    DeleteRefIfAny(env, &binding_ref);
    DeleteRefIfAny(env, &tick_callback_ref);
    DeleteRefIfAny(env, &promise_reject_callback_ref);
    tick_info_fields = nullptr;
    if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
      environment->tick_info()->fields = nullptr;
      DeleteRefIfAny(env, &environment->tick_info()->ref);
    }
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref tick_callback_ref = nullptr;
  napi_ref promise_reject_callback_ref = nullptr;
  int32_t* tick_info_fields = nullptr;
};

void DeleteRefIfAny(napi_env env, napi_ref* ref_slot) {
  if (env == nullptr || ref_slot == nullptr || *ref_slot == nullptr) return;
  napi_delete_reference(env, *ref_slot);
  *ref_slot = nullptr;
}

TaskQueueBindingState& GetTaskQueueState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<TaskQueueBindingState>(
      env, kEdgeEnvironmentSlotTaskQueueBindingState);
}

static napi_value TaskQueueEnqueueMicrotask(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

  if (unofficial_napi_enqueue_microtask(env, argv[0]) == napi_ok) {
    return internal_binding::Undefined(env);
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value queue_microtask = nullptr;
  if (napi_get_named_property(env, global, "queueMicrotask", &queue_microtask) == napi_ok &&
      queue_microtask != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, queue_microtask, &t) == napi_ok && t == napi_function) {
      napi_value ignored = nullptr;
      napi_call_function(env, global, queue_microtask, 1, argv, &ignored);
    }
  }

  return internal_binding::Undefined(env);
}

static napi_value TaskQueueRunMicrotasks(napi_env env, napi_callback_info /*info*/) {
  (void)unofficial_napi_process_microtasks(env);
  return internal_binding::Undefined(env);
}

static napi_value TaskQueueSetTickCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

  auto& st = GetTaskQueueState(env);
  DeleteRefIfAny(env, &st.tick_callback_ref);

  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, argv[0], &t) == napi_ok && t == napi_function) {
    napi_create_reference(env, argv[0], 1, &st.tick_callback_ref);
  }

  return internal_binding::Undefined(env);
}

static napi_value TaskQueueSetPromiseRejectCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;

#if defined(EDGE_EMBEDDED_NAPI_PROVIDER)
  (void)unofficial_napi_set_promise_reject_callback(env, argv[0]);
#endif

  auto& st = GetTaskQueueState(env);
  DeleteRefIfAny(env, &st.promise_reject_callback_ref);

  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, argv[0], &t) == napi_ok && t == napi_function) {
    napi_create_reference(env, argv[0], 1, &st.promise_reject_callback_ref);
  }

  return internal_binding::Undefined(env);
}

}  // namespace

napi_value EdgeGetOrCreateTaskQueueBinding(napi_env env) {
  if (env == nullptr) return nullptr;

  auto& st = GetTaskQueueState(env);
  if (st.binding_ref != nullptr) {
    napi_value existing = nullptr;
    if (napi_get_reference_value(env, st.binding_ref, &existing) == napi_ok && existing != nullptr) {
      return existing;
    }
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  auto define_method = [&](const char* name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) != napi_ok || fn == nullptr) {
      return false;
    }
    return napi_set_named_property(env, binding, name, fn) == napi_ok;
  };

  if (!define_method("enqueueMicrotask", TaskQueueEnqueueMicrotask) ||
      !define_method("setTickCallback", TaskQueueSetTickCallback) ||
      !define_method("runMicrotasks", TaskQueueRunMicrotasks) ||
      !define_method("setPromiseRejectCallback", TaskQueueSetPromiseRejectCallback)) {
    return nullptr;
  }

  napi_value tick_ab = nullptr;
  void* tick_data = nullptr;
  if (napi_create_arraybuffer(env, 2 * sizeof(int32_t), &tick_data, &tick_ab) != napi_ok ||
      tick_ab == nullptr || tick_data == nullptr) {
    return nullptr;
  }

  auto* fields = static_cast<int32_t*>(tick_data);
  fields[0] = 0;
  fields[1] = 0;
  st.tick_info_fields = fields;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->tick_info()->fields = fields;
  }

  napi_value tick_info = nullptr;
  if (napi_create_typedarray(env, napi_int32_array, 2, tick_ab, 0, &tick_info) != napi_ok || tick_info == nullptr) {
    return nullptr;
  }
  if (napi_set_named_property(env, binding, "tickInfo", tick_info) != napi_ok) return nullptr;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    DeleteRefIfAny(env, &environment->tick_info()->ref);
    napi_create_reference(env, tick_info, 1, &environment->tick_info()->ref);
  }

  napi_value promise_events = nullptr;
  if (napi_create_object(env, &promise_events) != napi_ok || promise_events == nullptr) return nullptr;
  auto set_event_const = [&](const char* name, int32_t value) -> bool {
    napi_value v = nullptr;
    return napi_create_int32(env, value, &v) == napi_ok && v != nullptr &&
           napi_set_named_property(env, promise_events, name, v) == napi_ok;
  };
  if (!set_event_const("kPromiseRejectWithNoHandler", 0) ||
      !set_event_const("kPromiseHandlerAddedAfterReject", 1) ||
      !set_event_const("kPromiseResolveAfterResolved", 2) ||
      !set_event_const("kPromiseRejectAfterResolved", 3)) {
    return nullptr;
  }
  if (napi_set_named_property(env, binding, "promiseRejectEvents", promise_events) != napi_ok) return nullptr;

  DeleteRefIfAny(env, &st.binding_ref);
  if (napi_create_reference(env, binding, 1, &st.binding_ref) != napi_ok || st.binding_ref == nullptr) {
    return nullptr;
  }

  return binding;
}

napi_status EdgeRunTaskQueueTickCallback(napi_env env, bool* called) {
  if (called != nullptr) {
    *called = false;
  }
  if (env == nullptr) {
    return napi_invalid_arg;
  }

  auto* state = EdgeEnvironmentGetSlotData<TaskQueueBindingState>(
      env, kEdgeEnvironmentSlotTaskQueueBindingState);
  if (state == nullptr || state->tick_callback_ref == nullptr) {
    return napi_ok;
  }

  edge::HandleScope scope(env);

  napi_value tick_cb = nullptr;
  napi_status status = napi_get_reference_value(env, state->tick_callback_ref, &tick_cb);
  if (status != napi_ok || tick_cb == nullptr) {
    return status == napi_ok ? napi_generic_failure : status;
  }

  napi_value global = nullptr;
  status = napi_get_global(env, &global);
  if (status != napi_ok || global == nullptr) {
    return status == napi_ok ? napi_generic_failure : status;
  }

  napi_value process = nullptr;
  if (napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) {
    process = global;
  }

  napi_value ignored = nullptr;
  status = napi_call_function(env, process, tick_cb, 0, nullptr, &ignored);
  if (status == napi_ok && called != nullptr) {
    *called = true;
  }
  return status;
}

bool EdgeGetTaskQueueFlags(napi_env env, bool* has_tick_scheduled, bool* has_rejection_to_warn) {
  if (has_tick_scheduled != nullptr) {
    *has_tick_scheduled = false;
  }
  if (has_rejection_to_warn != nullptr) {
    *has_rejection_to_warn = false;
  }
  if (env == nullptr) {
    return false;
  }

  int32_t* tick_info_fields = nullptr;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    tick_info_fields = environment->tick_info()->fields;
  }
  auto* state = EdgeEnvironmentGetSlotData<TaskQueueBindingState>(
      env, kEdgeEnvironmentSlotTaskQueueBindingState);
  if (tick_info_fields == nullptr && (state == nullptr || state->tick_info_fields == nullptr)) {
    return false;
  }
  if (tick_info_fields == nullptr) tick_info_fields = state->tick_info_fields;

  if (has_tick_scheduled != nullptr) {
    *has_tick_scheduled = tick_info_fields[0] != 0;
  }
  if (has_rejection_to_warn != nullptr) {
    *has_rejection_to_warn = tick_info_fields[1] != 0;
  }
  return true;
}
