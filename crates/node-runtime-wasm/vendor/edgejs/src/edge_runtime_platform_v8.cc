#include "edge_runtime_platform.h"

#include <cassert>
#include <atomic>
#include <chrono>
#include <deque>
#include <mutex>
#include <queue>
#include <thread>

#include <uv.h>

#include "edge_environment.h"
#include "edge_runtime.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_timers_host.h"
#include "unofficial_napi.h"

namespace {

using Clock = std::chrono::steady_clock;

struct PlatformTask {
  EdgeRuntimePlatformTaskCallback callback = nullptr;
  EdgeRuntimePlatformTaskCleanup cleanup = nullptr;
  void* data = nullptr;
  bool refed = false;
};

void CleanupTask(napi_env env, PlatformTask* task);
struct PlatformTaskState;

struct DelayedPlatformTask {
  PlatformTask task;
  uint64_t seq = 0;
  Clock::time_point due;
};

struct ScheduledForegroundTask {
  PlatformTaskState* state = nullptr;
  PlatformTask task;
  uv_timer_t timer{};
};

struct DelayedPlatformTaskCompare {
  bool operator()(const DelayedPlatformTask& a, const DelayedPlatformTask& b) const {
    if (a.due != b.due) return a.due > b.due;
    return a.seq > b.seq;
  }
};

struct PlatformTaskState {
  explicit PlatformTaskState(napi_env env_in)
      : env(env_in), owning_thread(std::this_thread::get_id()) {}
  ~PlatformTaskState() {
    while (!immediate_tasks.empty()) {
      PlatformTask task = std::move(immediate_tasks.front());
      immediate_tasks.pop_front();
      CleanupTask(env, &task);
    }
    while (!foreground_tasks.empty()) {
      PlatformTask task = std::move(foreground_tasks.front());
      foreground_tasks.pop_front();
      CleanupTask(env, &task);
    }
    while (!delayed_foreground_tasks.empty()) {
      DelayedPlatformTask delayed =
          std::move(const_cast<DelayedPlatformTask&>(delayed_foreground_tasks.top()));
      delayed_foreground_tasks.pop();
      CleanupTask(env, &delayed.task);
    }
  }

  napi_env env = nullptr;
  std::thread::id owning_thread;
  std::mutex foreground_mutex;
  size_t foreground_async_refs = 0;

  std::deque<PlatformTask> immediate_tasks;
  size_t refed_immediate_count = 0;
  bool draining_immediates = false;

  std::deque<PlatformTask> foreground_tasks;
  std::priority_queue<DelayedPlatformTask,
                      std::vector<DelayedPlatformTask>,
                      DelayedPlatformTaskCompare> delayed_foreground_tasks;
  std::vector<ScheduledForegroundTask*> scheduled_foreground_tasks;
  uint64_t next_foreground_seq = 0;
  bool draining_foreground = false;
  bool foreground_async_pending = false;

  uv_async_t foreground_async{};
  bool foreground_async_initialized = false;

  std::atomic<bool> cleanup_started {false};
  uint32_t pending_handle_closes = 0;
};

std::mutex g_retired_platform_states_mutex;
std::vector<std::unique_ptr<PlatformTaskState>> g_retired_platform_states;

size_t DrainForegroundTasksFromState(PlatformTaskState* state,
                                     bool run_checkpoint,
                                     bool clear_async_pending,
                                     napi_status* status_out,
                                     size_t* ran_out);

PlatformTaskState* GetState(napi_env env) {
  return EdgeEnvironmentGetSlotData<PlatformTaskState>(
      env, kEdgeEnvironmentSlotPlatformTaskState);
}

PlatformTaskState& EnsureState(napi_env env) {
  if (auto* existing = GetState(env); existing != nullptr) {
    return *existing;
  }
  auto* created = new PlatformTaskState(env);
  EdgeEnvironmentSetOpaqueSlot(
      env,
      kEdgeEnvironmentSlotPlatformTaskState,
      created,
      [](void* data) {
        auto* state = static_cast<PlatformTaskState*>(data);
        if (state == nullptr) return;
        std::lock_guard<std::mutex> lock(g_retired_platform_states_mutex);
        g_retired_platform_states.emplace_back(state);
      });
  return *created;
}

void AssertOwningThread(const PlatformTaskState* state, const char* where) {
  if (state == nullptr) return;
  assert(state->owning_thread == std::this_thread::get_id() &&
         "immediate/platform task APIs must run on the owning JS thread");
  (void)where;
}

void CleanupTask(napi_env env, PlatformTask* task) {
  if (task == nullptr) return;
  if (task->cleanup != nullptr) {
    task->cleanup(env, task->data);
  }
}

bool HasPendingException(napi_env env) {
  bool pending = false;
  return napi_is_exception_pending(env, &pending) == napi_ok && pending;
}

void MaybeDestroyState(PlatformTaskState* state) {
  (void)state;
}

void RemoveScheduledForegroundTask(PlatformTaskState* state,
                                   ScheduledForegroundTask* scheduled) {
  if (state == nullptr || scheduled == nullptr) return;
  auto it = std::find(state->scheduled_foreground_tasks.begin(),
                      state->scheduled_foreground_tasks.end(),
                      scheduled);
  if (it != state->scheduled_foreground_tasks.end()) {
    state->scheduled_foreground_tasks.erase(it);
  }
}

void OnScheduledForegroundTaskClosed(uv_handle_t* handle) {
  auto* scheduled = static_cast<ScheduledForegroundTask*>(handle != nullptr ? handle->data : nullptr);
  if (scheduled == nullptr) return;
  PlatformTaskState* state = scheduled->state;
  if (state != nullptr) {
    RemoveScheduledForegroundTask(state, scheduled);
    if (state->pending_handle_closes > 0) {
      --state->pending_handle_closes;
    }
  }
  CleanupTask(state != nullptr ? state->env : nullptr, &scheduled->task);
  delete scheduled;
  MaybeDestroyState(state);
}

void CloseScheduledForegroundTask(ScheduledForegroundTask* scheduled) {
  if (scheduled == nullptr) return;
  PlatformTaskState* state = scheduled->state;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&scheduled->timer);
  if (uv_is_closing(handle) != 0) {
    return;
  }
  if (state == nullptr) {
    CleanupTask(state != nullptr ? state->env : nullptr, &scheduled->task);
    delete scheduled;
    return;
  }
  ++state->pending_handle_closes;
  uv_close(handle, OnScheduledForegroundTaskClosed);
}

void RunScheduledForegroundTask(uv_timer_t* handle) {
  auto* scheduled = static_cast<ScheduledForegroundTask*>(handle != nullptr ? handle->data : nullptr);
  if (scheduled == nullptr) return;
  PlatformTaskState* state = scheduled->state;
  if (state != nullptr &&
      !state->cleanup_started.load(std::memory_order_acquire) &&
      scheduled->task.callback != nullptr) {
    edge::HandleScope scope(state->env);
    if (!scope.is_open()) {
      CloseScheduledForegroundTask(scheduled);
      return;
    }
    scheduled->task.callback(state->env, scheduled->task.data);
    if (HasPendingException(state->env)) {
      bool handled = false;
      (void)EdgeHandlePendingExceptionNow(state->env, &handled);
    }
  }
  if (handle != nullptr && handle->loop != nullptr) {
    uv_stop(handle->loop);
  }
  CloseScheduledForegroundTask(scheduled);
}

bool ScheduleForegroundTaskTimer(PlatformTaskState* state,
                                 PlatformTask task,
                                 Clock::time_point due) {
  if (state == nullptr) {
    CleanupTask(nullptr, &task);
    return false;
  }

  uv_loop_t* loop = EdgeGetEnvLoop(state->env);
  if (loop == nullptr) {
    CleanupTask(state->env, &task);
    return false;
  }

  auto* scheduled = new ScheduledForegroundTask();
  scheduled->state = state;
  scheduled->task = std::move(task);
  scheduled->timer.data = scheduled;

  if (uv_timer_init(loop, &scheduled->timer) != 0) {
    CleanupTask(state->env, &scheduled->task);
    delete scheduled;
    return false;
  }

  Clock::duration remaining = due - Clock::now();
  if (remaining < Clock::duration::zero()) {
    remaining = Clock::duration::zero();
  }
  const auto delay_ms = std::chrono::duration_cast<std::chrono::milliseconds>(remaining).count();
  if (uv_timer_start(&scheduled->timer,
                     RunScheduledForegroundTask,
                     static_cast<uint64_t>(delay_ms),
                     0) != 0) {
    CloseScheduledForegroundTask(scheduled);
    return false;
  }

  uv_unref(reinterpret_cast<uv_handle_t*>(&scheduled->timer));
  state->scheduled_foreground_tasks.push_back(scheduled);
  return true;
}

void OnForegroundHandleClosed(uv_handle_t* handle) {
  auto* state = static_cast<PlatformTaskState*>(handle->data);
  if (state == nullptr) return;

  if (handle == reinterpret_cast<uv_handle_t*>(&state->foreground_async)) {
    state->foreground_async_initialized = false;
  }

  if (state->pending_handle_closes > 0) {
    --state->pending_handle_closes;
  }
  MaybeDestroyState(state);
}

void CloseHandleIfInitialized(PlatformTaskState* state, uv_handle_t* handle, bool* initialized_flag) {
  if (state == nullptr || handle == nullptr || initialized_flag == nullptr || !*initialized_flag) return;
  *initialized_flag = false;
  if (uv_is_closing(handle) != 0) return;
  ++state->pending_handle_closes;
  uv_close(handle, OnForegroundHandleClosed);
}

void RefreshForegroundAsyncRef(PlatformTaskState* state) {
  if (state == nullptr || !state->foreground_async_initialized) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&state->foreground_async);
  if (state->foreground_async_refs == 0) {
    uv_unref(handle);
  } else {
    uv_ref(handle);
  }
}

size_t DrainForegroundTasksFromState(PlatformTaskState* state,
                                     bool run_checkpoint,
                                     bool clear_async_pending,
                                     napi_status* status_out,
                                     size_t* ran_out) {
  if (status_out != nullptr) *status_out = napi_ok;
  if (ran_out != nullptr) *ran_out = 0;
  if (state == nullptr ||
      state->cleanup_started.load(std::memory_order_acquire) ||
      state->draining_foreground) {
    return 0;
  }
  AssertOwningThread(state, "DrainForegroundTasksFromState");

  size_t ran = 0;
  state->draining_foreground = true;

  if (clear_async_pending) {
    std::lock_guard<std::mutex> lock(state->foreground_mutex);
    state->foreground_async_pending = false;
  }

  std::deque<PlatformTask> batch;
  std::vector<DelayedPlatformTask> delayed_batch;
  {
    std::lock_guard<std::mutex> lock(state->foreground_mutex);
    batch.swap(state->foreground_tasks);
    while (!state->delayed_foreground_tasks.empty()) {
      delayed_batch.push_back(
          std::move(const_cast<DelayedPlatformTask&>(state->delayed_foreground_tasks.top())));
      state->delayed_foreground_tasks.pop();
    }
  }

  for (auto& delayed : delayed_batch) {
    if (!ScheduleForegroundTaskTimer(state, std::move(delayed.task), delayed.due)) {
      continue;
    }
  }

  while (!batch.empty()) {
    PlatformTask task = std::move(batch.front());
    batch.pop_front();
    if (task.callback == nullptr) {
      CleanupTask(state->env, &task);
      continue;
    }
    edge::HandleScope scope(state->env);
    if (!scope.is_open()) {
      CleanupTask(state->env, &task);
      state->draining_foreground = false;
      if (status_out != nullptr) *status_out = scope.status();
      if (ran_out != nullptr) *ran_out = ran;
      return ran;
    }
    task.callback(state->env, task.data);
    ++ran;
    CleanupTask(state->env, &task);

    if (HasPendingException(state->env)) {
      bool handled = false;
      (void)EdgeHandlePendingExceptionNow(state->env, &handled);
      if (HasPendingException(state->env)) {
        state->draining_foreground = false;
        if (status_out != nullptr) *status_out = napi_pending_exception;
        if (ran_out != nullptr) *ran_out = ran;
        return ran;
      }
    }
  }

  (void)run_checkpoint;

  state->draining_foreground = false;
  if (ran_out != nullptr) *ran_out = ran;
  return ran;
}

bool EnsureForegroundHandles(PlatformTaskState* state) {
  if (state == nullptr) return false;
  uv_loop_t* loop = EdgeGetEnvLoop(state->env);
  if (loop == nullptr) return false;

  if (!state->foreground_async_initialized) {
    state->foreground_async.data = state;
    if (uv_async_init(loop,
                      &state->foreground_async,
                      [](uv_async_t* handle) {
                        auto* state = static_cast<PlatformTaskState*>(handle->data);
                        if (state == nullptr ||
                            state->cleanup_started.load(std::memory_order_acquire) ||
                            state->draining_foreground) {
                          return;
                        }
                        if (handle->loop != nullptr) {
                          uv_stop(handle->loop);
                        }
                      }) != 0) {
      return false;
    }
    uv_unref(reinterpret_cast<uv_handle_t*>(&state->foreground_async));
    state->foreground_async_initialized = true;
    RefreshForegroundAsyncRef(state);
  }

  return true;
}

napi_status EnqueueForegroundTaskFromEngine(void* target,
                                           unofficial_napi_foreground_task_callback callback,
                                           void* data,
                                           unofficial_napi_foreground_task_cleanup cleanup,
                                           uint64_t delay_millis);

napi_status InstallForegroundEnqueueHook(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = &EnsureState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  if (state == nullptr || !EnsureForegroundHandles(state)) return napi_generic_failure;
  return unofficial_napi_set_enqueue_foreground_task_callback(
      env, EnqueueForegroundTaskFromEngine, state);
}

void OnPlatformEnvCleanup(void* arg) {
  napi_env env = static_cast<napi_env>(arg);
  PlatformTaskState* state = GetState(env);
  if (state == nullptr) return;
  AssertOwningThread(state, "OnPlatformEnvCleanup");

  state->cleanup_started.store(true, std::memory_order_release);
  (void)unofficial_napi_set_enqueue_foreground_task_callback(env, nullptr, nullptr);
  state->foreground_async_refs = 0;
  while (!state->immediate_tasks.empty()) {
    PlatformTask task = std::move(state->immediate_tasks.front());
    state->immediate_tasks.pop_front();
    CleanupTask(env, &task);
  }
  {
    std::lock_guard<std::mutex> lock(state->foreground_mutex);
    state->foreground_async_pending = false;
    while (!state->foreground_tasks.empty()) {
      PlatformTask task = std::move(state->foreground_tasks.front());
      state->foreground_tasks.pop_front();
      CleanupTask(env, &task);
    }
    while (!state->delayed_foreground_tasks.empty()) {
      DelayedPlatformTask delayed =
          std::move(const_cast<DelayedPlatformTask&>(state->delayed_foreground_tasks.top()));
      state->delayed_foreground_tasks.pop();
      CleanupTask(env, &delayed.task);
    }
  }
  std::vector<ScheduledForegroundTask*> scheduled_tasks = std::move(state->scheduled_foreground_tasks);
  state->scheduled_foreground_tasks.clear();
  for (ScheduledForegroundTask* scheduled : scheduled_tasks) {
    CloseScheduledForegroundTask(scheduled);
  }
  state->refed_immediate_count = 0;
  state->env = nullptr;

  CloseHandleIfInitialized(state,
                           reinterpret_cast<uv_handle_t*>(&state->foreground_async),
                           &state->foreground_async_initialized);

  MaybeDestroyState(state);
}

size_t DrainForegroundTasks(napi_env env, bool run_checkpoint, napi_status* status_out) {
  PlatformTaskState* state = GetState(env);
  return DrainForegroundTasksFromState(state, run_checkpoint, false, status_out, nullptr);
}

napi_status EnqueueForegroundTaskFromEngine(void* target,
                                           unofficial_napi_foreground_task_callback callback,
                                           void* data,
                                           unofficial_napi_foreground_task_cleanup cleanup,
                                           uint64_t delay_millis) {
  auto* state = static_cast<PlatformTaskState*>(target);
  if (state == nullptr || callback == nullptr) return napi_invalid_arg;
  if (state->cleanup_started.load(std::memory_order_acquire)) {
    if (cleanup != nullptr) cleanup(state->env, data);
    return napi_generic_failure;
  }

  PlatformTask task;
  task.callback = callback;
  task.cleanup = cleanup;
  task.data = data;

  if (delay_millis != 0 && state->owning_thread == std::this_thread::get_id()) {
    AssertOwningThread(state, "EnqueueForegroundTaskFromEngine.delay");
    if (state->cleanup_started.load(std::memory_order_acquire)) {
      CleanupTask(state->env, &task);
      return napi_generic_failure;
    }
    return ScheduleForegroundTaskTimer(
               state,
               std::move(task),
               Clock::now() + std::chrono::milliseconds(delay_millis)) ?
           napi_ok :
           napi_generic_failure;
  }

  bool should_signal = false;
  int signal_rc = 0;
  {
    std::lock_guard<std::mutex> lock(state->foreground_mutex);
    if (state->cleanup_started.load(std::memory_order_acquire) ||
        !state->foreground_async_initialized) {
      CleanupTask(state->env, &task);
      return napi_generic_failure;
    }

    if (delay_millis == 0) {
      state->foreground_tasks.push_back(std::move(task));
    } else {
      DelayedPlatformTask delayed;
      delayed.task = std::move(task);
      delayed.seq = state->next_foreground_seq++;
      delayed.due = Clock::now() + std::chrono::milliseconds(delay_millis);
      state->delayed_foreground_tasks.push(std::move(delayed));
    }

    state->foreground_async_pending = true;
    should_signal = true;

    if (should_signal) {
      signal_rc = uv_async_send(&state->foreground_async);
      if (signal_rc != 0) {
        state->foreground_async_pending = false;
      }
    }
  }

  if (should_signal && signal_rc != 0) {
    return napi_generic_failure;
  }
  return napi_ok;
}

}  // namespace

napi_status EdgeRuntimePlatformInstallHooks(napi_env env) {
  return InstallForegroundEnqueueHook(env);
}

napi_status EdgeRuntimePlatformEnqueueForegroundTask(napi_env env,
                                                   EdgeRuntimePlatformTaskCallback callback,
                                                   void* data,
                                                   EdgeRuntimePlatformTaskCleanup cleanup,
                                                   uint64_t delay_millis) {
  if (env == nullptr || callback == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = GetState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  return EnqueueForegroundTaskFromEngine(
      state, callback, data, cleanup, delay_millis);
}

napi_status EdgeRuntimePlatformAddRef(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = GetState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  AssertOwningThread(state, "EdgeRuntimePlatformAddRef");
  if (!state->foreground_async_initialized && !EnsureForegroundHandles(state)) {
    return napi_generic_failure;
  }
  state->foreground_async_refs++;
  RefreshForegroundAsyncRef(state);
  return napi_ok;
}

napi_status EdgeRuntimePlatformReleaseRef(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = GetState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  AssertOwningThread(state, "EdgeRuntimePlatformReleaseRef");
  if (state->foreground_async_refs == 0) {
    return napi_generic_failure;
  }
  state->foreground_async_refs--;
  RefreshForegroundAsyncRef(state);
  return napi_ok;
}

napi_status EdgeRuntimePlatformEnqueueTask(napi_env env,
                                         EdgeRuntimePlatformTaskCallback callback,
                                         void* data,
                                         EdgeRuntimePlatformTaskCleanup cleanup,
                                         int flags) {
  if (env == nullptr || callback == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = GetState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  AssertOwningThread(state, "EdgeRuntimePlatformEnqueueTask");

  const bool refed = (flags & kEdgeRuntimePlatformTaskRefed) != 0;
  const bool need_ref = refed && state->refed_immediate_count == 0;

  PlatformTask task;
  task.callback = callback;
  task.cleanup = cleanup;
  task.data = data;
  task.refed = refed;
  state->immediate_tasks.push_back(std::move(task));
  if (refed) {
    state->refed_immediate_count++;
  }

  EdgeEnsureTimersImmediatePump(env);
  if (need_ref) {
    EdgeToggleImmediateRefFromNative(env, true);
  }

  return napi_ok;
}

size_t EdgeRuntimePlatformDrainImmediateTasks(napi_env env, bool only_refed) {
  PlatformTaskState* state = GetState(env);
  if (state == nullptr ||
      state->cleanup_started.load(std::memory_order_acquire) ||
      state->draining_immediates) {
    return 0;
  }
  AssertOwningThread(state, "EdgeRuntimePlatformDrainImmediateTasks");

  size_t ran = 0;
  state->draining_immediates = true;

  for (;;) {
    std::deque<PlatformTask> batch;
    const size_t batch_size = state->immediate_tasks.size();
    for (size_t i = 0; i < batch_size; ++i) {
      PlatformTask task = std::move(state->immediate_tasks.front());
      state->immediate_tasks.pop_front();
      if (only_refed && !task.refed) {
        state->immediate_tasks.push_back(std::move(task));
        continue;
      }
      if (task.refed && state->refed_immediate_count > 0) {
        state->refed_immediate_count--;
      }
      batch.push_back(std::move(task));
    }

    if (batch.empty()) break;

    while (!batch.empty()) {
      PlatformTask task = std::move(batch.front());
      batch.pop_front();

      edge::HandleScope scope(env);
      if (!scope.is_open()) {
        CleanupTask(env, &task);
        state->draining_immediates = false;
        if (state->refed_immediate_count == 0) {
          EdgeToggleImmediateRefFromNative(env, false);
        }
        return ran;
      }
      task.callback(env, task.data);
      ran++;
      CleanupTask(env, &task);

      if (HasPendingException(env)) {
        bool handled = false;
        (void)EdgeHandlePendingExceptionNow(env, &handled);
        if (!HasPendingException(env) && handled) {
          continue;
        }
        state->draining_immediates = false;
        if (state->refed_immediate_count == 0) {
          EdgeToggleImmediateRefFromNative(env, false);
        }
        return ran;
      }
    }
  }

  state->draining_immediates = false;
  if (state->refed_immediate_count == 0) {
    EdgeToggleImmediateRefFromNative(env, false);
  }
  return ran;
}

bool EdgeRuntimePlatformHasImmediateTasks(napi_env env) {
  PlatformTaskState* state = GetState(env);
  AssertOwningThread(state, "EdgeRuntimePlatformHasImmediateTasks");
  return state != nullptr && !state->immediate_tasks.empty();
}

bool EdgeRuntimePlatformHasRefedImmediateTasks(napi_env env) {
  PlatformTaskState* state = GetState(env);
  AssertOwningThread(state, "EdgeRuntimePlatformHasRefedImmediateTasks");
  return state != nullptr && state->refed_immediate_count != 0;
}

napi_status EdgeRuntimePlatformDrainTasks(napi_env env) {
  if (env == nullptr) return napi_invalid_arg;
  PlatformTaskState* state = GetState(env);
  if (state == nullptr || state->cleanup_started.load(std::memory_order_acquire)) {
    return napi_generic_failure;
  }
  napi_status status = napi_ok;
  (void)DrainForegroundTasksFromState(state, true, true, &status, nullptr);
  return status;
}

void EdgeRunRuntimePlatformEnvCleanup(napi_env env) {
  if (env == nullptr) return;
  OnPlatformEnvCleanup(env);
}
