#include "edge_timers_host.h"

#include <cmath>

#include "edge_environment.h"
#include "edge_handle_scope.h"
#include "edge_runtime.h"
#include "edge_worker_env.h"

namespace {

struct TimersHostState {
  explicit TimersHostState(napi_env env_in) : env(env_in) {}
  ~TimersHostState() {
    if (env != nullptr && immediate_callback_ref != nullptr) {
      napi_delete_reference(env, immediate_callback_ref);
    }
    if (env != nullptr && timers_callback_ref != nullptr) {
      napi_delete_reference(env, timers_callback_ref);
    }
  }

  napi_env env = nullptr;
  napi_ref timers_callback_ref = nullptr;
  napi_ref immediate_callback_ref = nullptr;
};

void DeleteRefIfAny(napi_env env, napi_ref* ref_slot) {
  if (env == nullptr || ref_slot == nullptr || *ref_slot == nullptr) return;
  napi_delete_reference(env, *ref_slot);
  *ref_slot = nullptr;
}

TimersHostState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<TimersHostState>(
      env, kEdgeEnvironmentSlotTimersHostState);
}

TimersHostState* GetOrCreateState(napi_env env) {
  if (env == nullptr) return nullptr;
  return &EdgeEnvironmentGetOrCreateSlotData<TimersHostState>(
      env, kEdgeEnvironmentSlotTimersHostState);
}

void SetFunctionRef(napi_env env, napi_value fn, napi_ref* ref_slot) {
  if (env == nullptr || ref_slot == nullptr) return;
  DeleteRefIfAny(env, ref_slot);
  if (fn == nullptr) return;
  napi_valuetype value_type = napi_undefined;
  if (napi_typeof(env, fn, &value_type) != napi_ok || value_type != napi_function) return;
  napi_create_reference(env, fn, 1, ref_slot);
}

bool GetProcessReceiver(napi_env env, napi_value* recv_out) {
  if (recv_out == nullptr) return false;
  *recv_out = nullptr;
  if (env == nullptr) return false;

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value process = nullptr;
  if (napi_get_named_property(env, global, "process", &process) == napi_ok &&
      process != nullptr) {
    *recv_out = process;
  } else {
    *recv_out = global;
  }
  return true;
}

bool CanCallTimersCallback(napi_env env) {
  if (env == nullptr) return false;
  auto* environment = EdgeEnvironmentGet(env);
  if (environment == nullptr || !environment->can_call_into_js()) return false;
  if (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env)) {
    return false;
  }
  return true;
}

bool CallImmediateCallback(TimersHostState* st) {
  if (st == nullptr || st->env == nullptr || st->immediate_callback_ref == nullptr) return true;
  edge::HandleScope scope(st->env);
  if (!scope.is_open()) return false;

  napi_value cb = nullptr;
  if (napi_get_reference_value(st->env, st->immediate_callback_ref, &cb) != napi_ok ||
      cb == nullptr) {
    return true;
  }
  napi_value recv = nullptr;
  if (!GetProcessReceiver(st->env, &recv) || recv == nullptr) return false;
  napi_value ignored = nullptr;
  const napi_status status = EdgeMakeCallback(st->env, recv, cb, 0, nullptr, &ignored);
  return status == napi_ok;
}

double CallTimersCallback(TimersHostState* st, double now) {
  if (st == nullptr || st->env == nullptr || st->timers_callback_ref == nullptr) return 0;
  edge::HandleScope scope(st->env);
  if (!scope.is_open()) return 0;

  napi_value cb = nullptr;
  if (napi_get_reference_value(st->env, st->timers_callback_ref, &cb) != napi_ok ||
      cb == nullptr) {
    return 0;
  }

  napi_value now_value = nullptr;
  if (napi_create_double(st->env, now, &now_value) != napi_ok || now_value == nullptr) return 0;

  napi_value recv = nullptr;
  if (!GetProcessReceiver(st->env, &recv) || recv == nullptr) return 0;

  napi_value result = nullptr;
  napi_status call_status = napi_ok;
  do {
    result = nullptr;
    call_status = EdgeMakeCallbackWithFlags(st->env,
                                            recv,
                                            cb,
                                            1,
                                            &now_value,
                                            &result,
                                            kEdgeMakeCallbackSkipTaskQueues);
    if (call_status == napi_ok && result != nullptr) break;

    bool pending = false;
    if (napi_is_exception_pending(st->env, &pending) == napi_ok && pending) {
      return 0;
    }
  } while (result == nullptr && CanCallTimersCallback(st->env));

  if (result == nullptr) return 0;

  double next = 0;
  if (napi_get_value_double(st->env, result, &next) != napi_ok || !std::isfinite(next)) {
    return 0;
  }
  return next;
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) != napi_ok ||
      fn == nullptr) {
    return;
  }
  napi_set_named_property(env, obj, name, fn);
}

void AttachInfoArrays(napi_env env, napi_value binding) {
  auto* environment = EdgeEnvironmentGet(env);
  if (environment == nullptr) return;

  napi_value immediate_ab = nullptr;
  void* immediate_data = nullptr;
  if (napi_create_arraybuffer(env, 3 * sizeof(int32_t), &immediate_data, &immediate_ab) == napi_ok &&
      immediate_ab != nullptr && immediate_data != nullptr) {
    auto* ptr = static_cast<int32_t*>(immediate_data);
    ptr[0] = 0;
    ptr[1] = 0;
    ptr[2] = 0;
    napi_value immediate_info = nullptr;
    if (napi_create_typedarray(env, napi_int32_array, 3, immediate_ab, 0, &immediate_info) == napi_ok &&
        immediate_info != nullptr) {
      napi_set_named_property(env, binding, "immediateInfo", immediate_info);
      environment->immediate_info()->fields = ptr;
      if (environment->immediate_info()->ref != nullptr) {
        napi_delete_reference(env, environment->immediate_info()->ref);
        environment->immediate_info()->ref = nullptr;
      }
      napi_create_reference(env, immediate_info, 1, &environment->immediate_info()->ref);
    }
  }

  napi_value timeout_ab = nullptr;
  void* timeout_data = nullptr;
  if (napi_create_arraybuffer(env, sizeof(int32_t), &timeout_data, &timeout_ab) == napi_ok &&
      timeout_ab != nullptr && timeout_data != nullptr) {
    auto* ptr = static_cast<int32_t*>(timeout_data);
    ptr[0] = 0;
    napi_value timeout_info = nullptr;
    if (napi_create_typedarray(env, napi_int32_array, 1, timeout_ab, 0, &timeout_info) == napi_ok &&
        timeout_info != nullptr) {
      napi_set_named_property(env, binding, "timeoutInfo", timeout_info);
      environment->timeout_info()->fields = ptr;
      if (environment->timeout_info()->ref != nullptr) {
        napi_delete_reference(env, environment->timeout_info()->ref);
        environment->timeout_info()->ref = nullptr;
      }
      napi_create_reference(env, timeout_info, 1, &environment->timeout_info()->ref);
    }
  }
}

napi_value SetupTimers(napi_env env, napi_callback_info info) {
  TimersHostState* st = GetOrCreateState(env);
  if (st == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc >= 1) SetFunctionRef(env, argv[0], &st->immediate_callback_ref);
  if (argc >= 2) SetFunctionRef(env, argv[1], &st->timers_callback_ref);

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ScheduleTimer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int64_t duration = 1;
  if (argc >= 1) {
    napi_get_value_int64(env, argv[0], &duration);
  }
  if (duration < 1) duration = 1;

  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->ScheduleTimer(duration);
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ToggleTimerRef(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool ref = true;
  if (argc >= 1) napi_get_value_bool(env, argv[0], &ref);
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->ToggleTimerRef(ref);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ToggleImmediateRef(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool ref = true;
  if (argc >= 1) napi_get_value_bool(env, argv[0], &ref);
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->ToggleImmediateRef(ref);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value GetLibuvNow(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  double now = 0;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    now = environment->GetNowMs();
  }
  napi_create_double(env, now, &out);
  return out;
}

}  // namespace

napi_value EdgeInstallTimersHostBinding(napi_env env) {
  if (env == nullptr) return nullptr;

  if (EdgeInitializeTimersHost(env) != napi_ok) return nullptr;

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) {
    return nullptr;
  }
  SetMethod(env, binding, "setupTimers", SetupTimers);
  SetMethod(env, binding, "scheduleTimer", ScheduleTimer);
  SetMethod(env, binding, "toggleTimerRef", ToggleTimerRef);
  SetMethod(env, binding, "toggleImmediateRef", ToggleImmediateRef);
  SetMethod(env, binding, "getLibuvNow", GetLibuvNow);
  AttachInfoArrays(env, binding);

  return binding;
}

napi_status EdgeInitializeTimersHost(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  if (GetOrCreateState(env) == nullptr) return napi_generic_failure;
  auto* environment = EdgeEnvironmentGet(env);
  if (environment == nullptr) return napi_generic_failure;
  return environment->InitializeTimers();
}

int32_t EdgeGetActiveTimeoutCount(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->active_timeout_count();
  }
  return 0;
}

uint32_t EdgeGetActiveImmediateRefCount(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->immediate_ref_count();
  }
  return 0;
}

void EdgeEnsureTimersImmediatePump(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->EnsureImmediatePump();
  }
}

void EdgeToggleImmediateRefFromNative(napi_env env, bool ref) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->ToggleImmediateRef(ref);
  }
}

bool EdgeTimersHostCallImmediateCallback(napi_env env) {
  return CallImmediateCallback(GetState(env));
}

double EdgeTimersHostCallTimersCallback(napi_env env, double now) {
  return CallTimersCallback(GetState(env), now);
}

bool EdgeTimersHostRunCallbackCheckpoint(napi_env env) {
  if (env == nullptr) return false;
  const napi_status status = EdgeRunCallbackScopeCheckpoint(env);
  if (status == napi_ok) return true;

  bool handled = false;
  (void)EdgeHandlePendingExceptionNow(env, &handled);

  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    return false;
  }

  if (status != napi_pending_exception) {
    return false;
  }
  return handled;
}
