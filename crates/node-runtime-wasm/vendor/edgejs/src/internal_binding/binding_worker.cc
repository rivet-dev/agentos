#include "internal_binding/binding_initializers.h"
#include "internal_binding/binding_messaging.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <sstream>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>
#include <vector>

#include <uv.h>

#include "internal_binding/helpers.h"
#include "unofficial_napi.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_environment_runtime.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_napi_embedder_hooks.h"
#include "edge_option_helpers.h"
#include "edge_process.h"
#include "edge_runtime.h"
#include "edge_runtime_platform.h"
#include "edge_stream_base.h"
#include "edge_stream_wrap.h"
#include "edge_timers_host.h"
#include "edge_worker_control.h"
#include "edge_worker_env.h"

namespace internal_binding {
namespace {

std::atomic<int32_t> g_next_worker_thread_id{1};
constexpr double kWorkerMb = 1024.0 * 1024.0;
constexpr size_t kDefaultWorkerStackSize = 4 * 1024 * 1024;
constexpr size_t kWorkerStackBufferSize = 192 * 1024;

struct Worker;
struct WorkerThreadData;

struct WorkerParentState {
  explicit WorkerParentState(napi_env) {}
  std::unordered_set<Worker*> workers;
};

std::mutex g_worker_registry_mu;

enum class WorkerTaskType {
  kCpuUsage,
  kGetHeapStatistics,
  kStartCpuProfile,
  kStopCpuProfile,
  kStartHeapProfile,
  kStopHeapProfile,
  kTakeHeapSnapshot,
};

struct WorkerTask;
struct HeapSnapshotHandleWrap;

struct WorkerThreadData {
  explicit WorkerThreadData(Worker* worker_in) : worker(worker_in) {}

  Worker* worker = nullptr;
  napi_env env = nullptr;
  void* scope = nullptr;
  uv_async_t stop_async{};
  std::atomic<bool> stop_async_initialized{false};
};

struct Worker {
  explicit Worker(napi_env env_in) : env(env_in), thread_data(std::make_unique<WorkerThreadData>(this)) {}

  napi_env env = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref message_port_ref = nullptr;
  int32_t thread_id = 0;
  std::string thread_name = "WorkerThread";
  std::vector<std::string> exec_argv;
  EdgeWorkerEnvConfig worker_config;
  EdgeMessagePortDataPtr child_port_data;
  uv_thread_t thread{};
  std::mutex mutex;
  std::unique_ptr<WorkerThreadData> thread_data;
  size_t thread_stack_size = 4 * 1024 * 1024;
  uintptr_t stack_base = 0;
  uv_async_t* completion_async = nullptr;
  std::atomic<bool> completion_async_initialized{false};
  std::atomic<bool> started{false};
  std::atomic<bool> thread_joined{true};
  std::atomic<bool> has_ref{true};
  std::atomic<bool> stop_requested{false};
  std::atomic<bool> parent_env_closing{false};
  std::atomic<bool> reached_heap_limit{false};
  std::atomic<bool> resource_alive{false};
  std::atomic<bool> parent_completion_queued{false};
  std::atomic<double> loop_start_time_ms{-1};
  int64_t async_id = 0;
  void* active_handle_token = nullptr;
  int32_t requested_exit_code = 1;
  int32_t exit_code = 0;
  std::string custom_err;
  std::string custom_err_reason;
  std::mutex task_mutex;
  std::deque<std::unique_ptr<WorkerTask>> completed_tasks;
};

struct WorkerTask {
  Worker* wrap = nullptr;
  WorkerTaskType type;
  napi_ref taker_ref = nullptr;
  uint32_t profile_id = 0;
  bool success = true;
  bool found = false;
  std::string error_code;
  std::string error_message;
  uv_rusage_t cpu_usage{};
  unofficial_napi_heap_statistics heap_statistics{};
  uint32_t started_profile_id = 0;
  char* json_data = nullptr;
  size_t json_len = 0;
  unofficial_napi_heap_snapshot_options heap_snapshot_options{};
};

struct HeapSnapshotHandleWrap {
  napi_env env = nullptr;
  std::string payload;
  bool delivered = false;
};

struct WorkerReportTaskState {
  std::mutex mutex;
  std::condition_variable cv;
  size_t pending = 0;
  std::vector<EdgeWorkerReportEntry> reports;
};

struct WorkerReportTask {
  WorkerReportTaskState* state = nullptr;
  int32_t thread_id = 0;
  std::string thread_name;
};

void ClearCompletedWorkerTasks(napi_env env, Worker* worker);
void OnWorkerCompletionAsync(uv_async_t* handle);

napi_value GetNamed(napi_env env, napi_value obj, const char* key) {
  napi_value out = nullptr;
  if (obj == nullptr || napi_get_named_property(env, obj, key, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool IsNullOrUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && (type == napi_null || type == napi_undefined);
}

bool IsNullValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_null;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return {};
  napi_value as_string = nullptr;
  if (napi_coerce_to_string(env, value, &as_string) != napi_ok || as_string == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, as_string, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, as_string, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

void ClearPendingException(napi_env env) {
  if (env == nullptr) return;
  // TODO: Make napi_get_and_clear_last_exception() match upstream Node so
  // this helper does not need a separate pending-exception check first.
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
    napi_value ignored = nullptr;
    (void)napi_get_and_clear_last_exception(env, &ignored);
  }
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void SetRefValue(napi_env env, napi_ref* slot, napi_value value) {
  if (slot == nullptr) return;
  DeleteRefIfPresent(env, slot);
  if (value != nullptr) {
    napi_create_reference(env, value, 1, slot);
  }
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

void CompleteWorkerReportTask(WorkerReportTask* task, std::string json) {
  if (task == nullptr || task->state == nullptr) {
    delete task;
    return;
  }
  {
    std::lock_guard<std::mutex> lock(task->state->mutex);
    if (!json.empty()) {
      task->state->reports.push_back(
          EdgeWorkerReportEntry{task->thread_id, std::move(task->thread_name), std::move(json)});
    }
    if (task->state->pending > 0) {
      task->state->pending -= 1;
    }
  }
  task->state->cv.notify_one();
  delete task;
}

void OnWorkerReportInterrupt(napi_env env, void* data) {
  auto* task = static_cast<WorkerReportTask*>(data);
  if (env == nullptr || task == nullptr) {
    CompleteWorkerReportTask(task, {});
    return;
  }

  std::string json;
  napi_value report_binding = EdgeGetReportBinding(env);
  napi_value get_report = GetNamed(env, report_binding, "getReport");
  napi_value report_string = nullptr;
  if (IsFunction(env, get_report) &&
      napi_call_function(env, report_binding, get_report, 0, nullptr, &report_string) == napi_ok &&
      report_string != nullptr) {
    json = ValueToUtf8(env, report_string);
  }
  CompleteWorkerReportTask(task, std::move(json));
}

Worker* UnwrapWorker(napi_env env, napi_value this_arg) {
  if (this_arg == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<Worker*>(data);
}

void RemoveWorkerFromRegistry(Worker* worker);
void CloseWorkerMessagePort(Worker* worker);

napi_env GetWorkerThreadEnv(Worker* worker) {
  if (worker == nullptr || worker->thread_data == nullptr) return nullptr;
  std::lock_guard<std::mutex> lock(worker->mutex);
  return worker->thread_data->env;
}

bool JoinWorkerThread(Worker* worker) {
  if (worker == nullptr) return false;
  if (worker->thread_joined.exchange(true, std::memory_order_acq_rel)) return false;
  uv_thread_join(&worker->thread);
  return true;
}

void RequestWorkerStop(Worker* worker) {
  if (worker == nullptr || !worker->started.load(std::memory_order_acquire)) return;
  worker->stop_requested.store(true, std::memory_order_release);
  napi_env worker_env = GetWorkerThreadEnv(worker);
  if (worker_env != nullptr) {
    EdgeWorkerEnvRequestStop(worker_env);
    (void)unofficial_napi_terminate_execution(worker_env);
  }
  if (worker->thread_data != nullptr &&
      worker->thread_data->stop_async_initialized.load(std::memory_order_acquire)) {
    uv_async_send(&worker->thread_data->stop_async);
  }
}

void OnWorkerCompletionAsyncClosed(uv_handle_t* handle) {
  delete reinterpret_cast<uv_async_t*>(handle);
}

void CloseWorkerCompletionAsync(Worker* worker) {
  if (worker == nullptr) return;
  uv_async_t* handle = worker->completion_async;
  worker->completion_async = nullptr;
  if (!worker->completion_async_initialized.exchange(false, std::memory_order_acq_rel) ||
      handle == nullptr) {
    delete handle;
    return;
  }
  if (uv_is_closing(reinterpret_cast<uv_handle_t*>(handle)) == 0) {
    uv_close(reinterpret_cast<uv_handle_t*>(handle), OnWorkerCompletionAsyncClosed);
  }
}

void CleanupWorkerForParentEnvShutdown(Worker* worker) {
  if (worker == nullptr || worker->env == nullptr) return;
  const bool was_started = worker->started.exchange(false, std::memory_order_acq_rel);
  if (was_started && worker->has_ref.load(std::memory_order_acquire)) {
    (void)EdgeRuntimePlatformReleaseRef(worker->env);
  }
  (void)JoinWorkerThread(worker);
  RemoveWorkerFromRegistry(worker);
  worker->resource_alive.store(false, std::memory_order_release);
  if (worker->active_handle_token != nullptr) {
    EdgeUnregisterActiveHandle(worker->env, worker->active_handle_token);
    worker->active_handle_token = nullptr;
  }
  if (worker->async_id > 0) {
    EdgeAsyncWrapQueueDestroyId(worker->env, worker->async_id);
    worker->async_id = 0;
  }
  CloseWorkerMessagePort(worker);
  DeleteRefIfPresent(worker->env, &worker->message_port_ref);
  ClearCompletedWorkerTasks(worker->env, worker);
  CloseWorkerCompletionAsync(worker);
}

void DetachWorkerFromJs(Worker* worker) {
  if (worker == nullptr || worker->env == nullptr || worker->wrapper_ref == nullptr) return;
  napi_value self = GetRefValue(worker->env, worker->wrapper_ref);
  if (self != nullptr) {
    void* removed = nullptr;
    (void)napi_remove_wrap(worker->env, self, &removed);
  }
  DeleteRefIfPresent(worker->env, &worker->wrapper_ref);
}

WorkerParentState* GetWorkerParentStateLocked(napi_env env) {
  return EdgeEnvironmentGetSlotData<WorkerParentState>(env, kEdgeEnvironmentSlotWorkerParentState);
}

WorkerParentState& EnsureWorkerParentStateLocked(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<WorkerParentState>(
      env, kEdgeEnvironmentSlotWorkerParentState);
}

void AddWorkerToRegistry(Worker* worker) {
  if (worker == nullptr || worker->env == nullptr) return;
  std::lock_guard<std::mutex> lock(g_worker_registry_mu);
  EnsureWorkerParentStateLocked(worker->env).workers.insert(worker);
}

void RemoveWorkerFromRegistry(Worker* worker) {
  if (worker == nullptr || worker->env == nullptr) return;
  std::lock_guard<std::mutex> lock(g_worker_registry_mu);
  auto* state = GetWorkerParentStateLocked(worker->env);
  if (state == nullptr) return;
  state->workers.erase(worker);
}

void StopWorkersForEnv(napi_env env) {
  if (env == nullptr) return;
  std::vector<Worker*> workers;
  {
    std::lock_guard<std::mutex> lock(g_worker_registry_mu);
    auto* state = GetWorkerParentStateLocked(env);
    if (state != nullptr) {
      workers.assign(state->workers.begin(), state->workers.end());
      state->workers.clear();
    }
  }

  for (Worker* worker : workers) {
    if (worker == nullptr) continue;
    worker->parent_env_closing.store(true, std::memory_order_release);
    RequestWorkerStop(worker);
    CleanupWorkerForParentEnvShutdown(worker);
  }
}

bool WorkerImplHasRefActive(void* data) {
  auto* worker = static_cast<Worker*>(data);
  return worker != nullptr &&
         worker->resource_alive.load(std::memory_order_acquire) &&
         worker->has_ref.load(std::memory_order_acquire);
}

napi_value WorkerImplGetActiveOwner(napi_env env, void* data) {
  auto* worker = static_cast<Worker*>(data);
  return worker != nullptr ? GetRefValue(env, worker->wrapper_ref) : nullptr;
}

std::string FileUrlToPath(const std::string& maybe_url) {
  if (maybe_url.rfind("file://", 0) != 0) return maybe_url;
  std::string path = maybe_url.substr(7);
  if (path.size() >= 3 && path[0] == '/' && path[2] == ':') return path.substr(1);
  return path;
}

bool SnapshotStringMapFromObject(napi_env env, napi_value object, std::map<std::string, std::string>* out) {
  if (env == nullptr || object == nullptr || out == nullptr) return false;
  napi_value keys = nullptr;
  if (napi_get_property_names(env, object, &keys) != napi_ok || keys == nullptr) return false;
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return false;
  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    const std::string key_string = ValueToUtf8(env, key);
    if (key_string.empty()) continue;
    napi_value value = nullptr;
    if (napi_get_property(env, object, key, &value) != napi_ok || value == nullptr) continue;
    napi_value string_value = nullptr;
    if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) continue;
    (*out)[key_string] = ValueToUtf8(env, string_value);
  }
  return true;
}

std::map<std::string, std::string> SnapshotCurrentProcessEnv(napi_env env) {
  std::map<std::string, std::string> out;
  napi_value global = GetGlobal(env);
  napi_value process = GetNamed(env, global, "process");
  napi_value process_env = GetNamed(env, process, "env");
  if (process_env != nullptr) {
    (void)SnapshotStringMapFromObject(env, process_env, &out);
  }
  return out;
}

std::vector<std::string> ReadStringArray(napi_env env, napi_value value) {
  std::vector<std::string> out;
  bool is_array = false;
  if (value == nullptr || napi_is_array(env, value, &is_array) != napi_ok || !is_array) return out;
  uint32_t len = 0;
  if (napi_get_array_length(env, value, &len) != napi_ok) return out;
  out.reserve(len);
  for (uint32_t i = 0; i < len; ++i) {
    napi_value element = nullptr;
    if (napi_get_element(env, value, i, &element) != napi_ok || element == nullptr) continue;
    const std::string arg = ValueToUtf8(env, element);
    if (arg == "--") break;
    out.push_back(arg);
  }
  return out;
}

bool IsAllowedNodeEnvironmentFlag(napi_env env, const std::string& flag) {
  if (flag == "--no-addons") return true;
  napi_value global = GetGlobal(env);
  napi_value process = GetNamed(env, global, "process");
  napi_value allowed = GetNamed(env, process, "allowedNodeEnvironmentFlags");
  napi_value has = GetNamed(env, allowed, "has");
  if (!IsFunction(env, has)) return true;
  napi_value flag_v = nullptr;
  if (napi_create_string_utf8(env, flag.c_str(), NAPI_AUTO_LENGTH, &flag_v) != napi_ok || flag_v == nullptr) {
    return false;
  }
  napi_value result = nullptr;
  if (napi_call_function(env, allowed, has, 1, &flag_v, &result) != napi_ok || result == nullptr) {
    return false;
  }
  bool is_allowed = false;
  return napi_get_value_bool(env, result, &is_allowed) == napi_ok && is_allowed;
}

bool IsDisallowedWorkerExecArgvFlag(const std::string& flag) {
  return flag == "--expose-gc" ||
         flag == "--expose_gc" ||
         flag == "--title" ||
         flag == "--redirect-warnings";
}

std::vector<std::string> ValidateExecArgv(napi_env env,
                                          const std::vector<std::string>& args,
                                          bool explicitly_provided) {
  std::vector<std::string> invalid;
  if (!explicitly_provided) return invalid;
  for (const std::string& arg : args) {
    if (arg.empty() || arg[0] != '-') continue;
    std::string flag = arg;
    const size_t eq = flag.find('=');
    if (eq != std::string::npos) flag.resize(eq);
    if (IsDisallowedWorkerExecArgvFlag(flag) || !IsAllowedNodeEnvironmentFlag(env, flag)) {
      invalid.push_back(arg);
    }
  }
  return invalid;
}

std::vector<std::string> ValidateNodeOptionsEnv(napi_env env, const std::map<std::string, std::string>& entries) {
  auto it = entries.find("NODE_OPTIONS");
  if (it == entries.end()) return {};
  const std::map<std::string, std::string> parent_env = SnapshotCurrentProcessEnv(env);
  auto parent_it = parent_env.find("NODE_OPTIONS");
  if (parent_it != parent_env.end() && parent_it->second == it->second) {
    return {};
  }

  std::vector<std::string> invalid;
  std::string parse_error;
  const std::vector<std::string> tokens = edge_options::ParseNodeOptionsString(it->second, &parse_error);
  if (!parse_error.empty()) {
    invalid.push_back(parse_error);
    return invalid;
  }

  for (const std::string& token : tokens) {
    if (token.empty() || token[0] != '-') continue;
    std::string flag = token;
    const size_t eq = flag.find('=');
    if (eq != std::string::npos) flag.resize(eq);
    if (IsDisallowedWorkerExecArgvFlag(flag) || !IsAllowedNodeEnvironmentFlag(env, flag)) {
      invalid.push_back(token);
    }
  }
  return invalid;
}

std::array<double, 4> ReadResourceLimits(napi_env env, napi_value value) {
  std::array<double, 4> limits = {-1, -1, -1, -1};
  bool is_typed_array = false;
  napi_typedarray_type typed_array_type = napi_int8_array;
  size_t length = 0;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (value == nullptr ||
      napi_is_typedarray(env, value, &is_typed_array) != napi_ok ||
      !is_typed_array ||
      napi_get_typedarray_info(env, value, &typed_array_type, &length, nullptr, &arraybuffer, &byte_offset) != napi_ok ||
      typed_array_type != napi_float64_array) {
    return limits;
  }
  void* data = nullptr;
  size_t byte_length = 0;
  if (napi_get_arraybuffer_info(env, arraybuffer, &data, &byte_length) != napi_ok || data == nullptr) return limits;
  const double* values = reinterpret_cast<const double*>(static_cast<uint8_t*>(data) + byte_offset);
  const size_t count = length < limits.size() ? length : limits.size();
  for (size_t i = 0; i < count; ++i) limits[i] = values[i];
  return limits;
}

napi_value BuildResourceLimitsArray(napi_env env, const std::array<double, 4>& limits) {
  napi_value ab = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, sizeof(double) * limits.size(), &data, &ab) != napi_ok || ab == nullptr ||
      data == nullptr) {
    return Undefined(env);
  }
  double* values = static_cast<double*>(data);
  for (size_t i = 0; i < limits.size(); ++i) values[i] = limits[i];
  napi_value typed = nullptr;
  if (napi_create_typedarray(env, napi_float64_array, limits.size(), ab, 0, &typed) != napi_ok || typed == nullptr) {
    return Undefined(env);
  }
  return typed;
}

void FreeWorkerTaskPayload(WorkerTask* task) {
  if (task == nullptr) return;
  if (task->json_data != nullptr) {
    unofficial_napi_free_buffer(task->json_data);
    task->json_data = nullptr;
    task->json_len = 0;
  }
}

void DeleteWorkerTaskRef(napi_env env, WorkerTask* task) {
  if (env == nullptr || task == nullptr || task->taker_ref == nullptr) return;
  napi_delete_reference(env, task->taker_ref);
  task->taker_ref = nullptr;
}

void ClearCompletedWorkerTasks(napi_env env, Worker* wrap) {
  if (wrap == nullptr) return;
  std::deque<std::unique_ptr<WorkerTask>> completed;
  {
    std::lock_guard<std::mutex> lock(wrap->task_mutex);
    completed.swap(wrap->completed_tasks);
  }
  for (auto& task : completed) {
    if (!task) continue;
    DeleteWorkerTaskRef(env, task.get());
    FreeWorkerTaskPayload(task.get());
  }
}

napi_value CreateNull(napi_env env) {
  napi_value out = nullptr;
  napi_get_null(env, &out);
  return out;
}

bool SetNamedInt64(napi_env env, napi_value obj, const char* key, int64_t value) {
  napi_value out = nullptr;
  return napi_create_int64(env, value, &out) == napi_ok &&
         out != nullptr &&
         napi_set_named_property(env, obj, key, out) == napi_ok;
}

bool SetNamedDouble(napi_env env, napi_value obj, const char* key, double value) {
  napi_value out = nullptr;
  return napi_create_double(env, value, &out) == napi_ok &&
         out != nullptr &&
         napi_set_named_property(env, obj, key, out) == napi_ok;
}

napi_value CreateErrorWithCode(napi_env env, const std::string& code, const std::string& message) {
  napi_value msg = nullptr;
  napi_value err = nullptr;
  if (napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &msg) != napi_ok ||
      msg == nullptr ||
      napi_create_error(env, nullptr, msg, &err) != napi_ok ||
      err == nullptr) {
    return nullptr;
  }
  napi_value code_value = nullptr;
  if (napi_create_string_utf8(env, code.c_str(), NAPI_AUTO_LENGTH, &code_value) == napi_ok &&
      code_value != nullptr) {
    napi_set_named_property(env, err, "code", code_value);
  }
  return err;
}

napi_value CreateTakerObject(napi_env env) {
  napi_value taker = nullptr;
  if (napi_create_object(env, &taker) != napi_ok) return nullptr;
  return taker;
}

bool WorkerCanAcceptTask(Worker* wrap) {
  if (wrap == nullptr) return false;
  if (!wrap->started.load(std::memory_order_acquire) ||
      !wrap->resource_alive.load(std::memory_order_acquire) ||
      wrap->stop_requested.load(std::memory_order_acquire) ||
      !wrap->completion_async_initialized.load(std::memory_order_acquire)) {
    return false;
  }
  return GetWorkerThreadEnv(wrap) != nullptr;
}

HeapSnapshotHandleWrap* UnwrapHeapSnapshotHandle(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, value, &data) != napi_ok) return nullptr;
  return static_cast<HeapSnapshotHandleWrap*>(data);
}

void HeapSnapshotHandleFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  delete static_cast<HeapSnapshotHandleWrap*>(data);
}

napi_value HeapSnapshotHandleReadStop(napi_env env, napi_callback_info /*info*/) {
  napi_value zero = nullptr;
  napi_create_int32(env, 0, &zero);
  return zero != nullptr ? zero : Undefined(env);
}

napi_value HeapSnapshotHandleReadStart(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok || self == nullptr) {
    return Undefined(env);
  }

  HeapSnapshotHandleWrap* wrap = UnwrapHeapSnapshotHandle(env, self);
  napi_value onread = GetNamed(env, self, "onread");
  if (wrap == nullptr || !IsFunction(env, onread)) return Undefined(env);

  int32_t* state = EdgeGetStreamBaseState(env);
  if (state == nullptr) return Undefined(env);

  auto emit_onread = [&](int32_t nread, const std::string* payload) {
    state[kEdgeReadBytesOrError] = nread;
    state[kEdgeArrayBufferOffset] = 0;
    napi_value arraybuffer = nullptr;
    void* data = nullptr;
    size_t length = payload != nullptr ? payload->size() : 0;
    if (napi_create_arraybuffer(env, length, &data, &arraybuffer) != napi_ok || arraybuffer == nullptr) {
      return;
    }
    if (payload != nullptr && data != nullptr && !payload->empty()) {
      std::memcpy(data, payload->data(), payload->size());
    }
    napi_value argv[1] = {arraybuffer};
    napi_value ignored = nullptr;
    (void)napi_call_function(env, self, onread, 1, argv, &ignored);
  };

  if (!wrap->delivered) {
    wrap->delivered = true;
    emit_onread(static_cast<int32_t>(wrap->payload.size()), &wrap->payload);
  }
  emit_onread(UV_EOF, nullptr);

  napi_value zero = nullptr;
  napi_create_int32(env, 0, &zero);
  return zero != nullptr ? zero : Undefined(env);
}

napi_value CreateHeapSnapshotHandle(napi_env env, WorkerTask* task) {
  if (env == nullptr || task == nullptr || task->json_data == nullptr) return nullptr;
  napi_value handle = nullptr;
  if (napi_create_object(env, &handle) != napi_ok || handle == nullptr) return nullptr;
  auto* wrap = new HeapSnapshotHandleWrap();
  wrap->env = env;
  wrap->payload.assign(task->json_data, task->json_len);
  if (napi_wrap(env, handle, wrap, HeapSnapshotHandleFinalize, nullptr, nullptr) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  napi_value read_start = nullptr;
  napi_value read_stop = nullptr;
  if (napi_create_function(env, "readStart", NAPI_AUTO_LENGTH, HeapSnapshotHandleReadStart, nullptr, &read_start) == napi_ok &&
      read_start != nullptr) {
    napi_set_named_property(env, handle, "readStart", read_start);
  }
  if (napi_create_function(env, "readStop", NAPI_AUTO_LENGTH, HeapSnapshotHandleReadStop, nullptr, &read_stop) == napi_ok &&
      read_stop != nullptr) {
    napi_set_named_property(env, handle, "readStop", read_stop);
  }
  return handle;
}

void CompleteWorkerTaskOnWorkerThread(WorkerTask* task) {
  if (task == nullptr || task->wrap == nullptr) return;
  Worker* wrap = task->wrap;
  {
    std::lock_guard<std::mutex> lock(wrap->task_mutex);
    wrap->completed_tasks.emplace_back(task);
  }
  uv_async_t* handle = wrap->completion_async;
  if (wrap->completion_async_initialized.load(std::memory_order_acquire) && handle != nullptr) {
    uv_async_send(handle);
  }
}

void SetTaskError(WorkerTask* task, const char* code, const std::string& message) {
  if (task == nullptr) return;
  task->success = false;
  task->error_code = code != nullptr ? code : "ERR_WORKER_OPERATION_FAILED";
  task->error_message = message;
}

void OnWorkerTaskInterrupt(napi_env worker_env, void* data) {
  auto* task = static_cast<WorkerTask*>(data);
  if (task == nullptr || task->wrap == nullptr || worker_env == nullptr) return;

  switch (task->type) {
    case WorkerTaskType::kCpuUsage: {
      const int rc = uv_getrusage_thread(&task->cpu_usage);
      task->found = (rc == 0);
      if (rc != 0) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", uv_strerror(rc));
      }
      break;
    }
    case WorkerTaskType::kGetHeapStatistics: {
      if (unofficial_napi_get_heap_statistics(worker_env, &task->heap_statistics) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to get heap statistics");
      } else {
        task->found = true;
      }
      break;
    }
    case WorkerTaskType::kStartCpuProfile: {
      unofficial_napi_cpu_profile_start_result result = unofficial_napi_cpu_profile_start_ok;
      if (unofficial_napi_start_cpu_profile(worker_env, &result, &task->started_profile_id) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to start CPU profile");
      } else if (result == unofficial_napi_cpu_profile_start_too_many) {
        SetTaskError(task, "ERR_CPU_PROFILE_TOO_MANY", "There are too many CPU profiles");
      } else {
        task->found = true;
      }
      break;
    }
    case WorkerTaskType::kStopCpuProfile: {
      if (unofficial_napi_stop_cpu_profile(
              worker_env, task->profile_id, &task->found, &task->json_data, &task->json_len) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to stop CPU profile");
      } else if (!task->found) {
        SetTaskError(task, "ERR_CPU_PROFILE_NOT_STARTED", "CPU profile not started");
      }
      break;
    }
    case WorkerTaskType::kStartHeapProfile: {
      if (unofficial_napi_start_heap_profile(worker_env, &task->found) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to start heap profile");
      } else if (!task->found) {
        SetTaskError(task, "ERR_HEAP_PROFILE_HAVE_BEEN_STARTED", "heap profiler have been started");
      }
      break;
    }
    case WorkerTaskType::kStopHeapProfile: {
      if (unofficial_napi_stop_heap_profile(
              worker_env, &task->found, &task->json_data, &task->json_len) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to stop heap profile");
      } else if (!task->found) {
        SetTaskError(task, "ERR_HEAP_PROFILE_NOT_STARTED", "heap profile not started");
      }
      break;
    }
    case WorkerTaskType::kTakeHeapSnapshot: {
      if (unofficial_napi_take_heap_snapshot(
              worker_env, &task->heap_snapshot_options, &task->json_data, &task->json_len) != napi_ok) {
        SetTaskError(task, "ERR_WORKER_OPERATION_FAILED", "Failed to take heap snapshot");
      } else {
        task->found = true;
      }
      break;
    }
  }

  CompleteWorkerTaskOnWorkerThread(task);
}

void OnWorkerCompletionAsync(uv_async_t* handle) {
  auto* wrap = static_cast<Worker*>(handle != nullptr ? handle->data : nullptr);
  if (wrap == nullptr || wrap->env == nullptr) return;
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;

  std::deque<std::unique_ptr<WorkerTask>> completed;
  {
    std::lock_guard<std::mutex> lock(wrap->task_mutex);
    completed.swap(wrap->completed_tasks);
  }

  for (auto& task : completed) {
    if (!task || task->taker_ref == nullptr) {
      if (task) FreeWorkerTaskPayload(task.get());
      continue;
    }
    napi_value taker = GetRefValue(wrap->env, task->taker_ref);
    DeleteWorkerTaskRef(wrap->env, task.get());
    if (taker == nullptr) {
      FreeWorkerTaskPayload(task.get());
      continue;
    }

    napi_value ondone = GetNamed(wrap->env, taker, "ondone");
    if (!IsFunction(wrap->env, ondone)) {
      FreeWorkerTaskPayload(task.get());
      continue;
    }

    napi_value argv[2] = {nullptr, nullptr};
    size_t argc = 0;
    switch (task->type) {
      case WorkerTaskType::kCpuUsage: {
        argc = 2;
        argv[0] = task->success ? CreateNull(wrap->env)
                                : CreateErrorWithCode(wrap->env, task->error_code, task->error_message);
        if (task->success) {
          napi_create_object(wrap->env, &argv[1]);
          SetNamedInt64(
              wrap->env,
              argv[1],
              "user",
              static_cast<int64_t>(1000000ll * task->cpu_usage.ru_utime.tv_sec +
                                   task->cpu_usage.ru_utime.tv_usec));
          SetNamedInt64(
              wrap->env,
              argv[1],
              "system",
              static_cast<int64_t>(1000000ll * task->cpu_usage.ru_stime.tv_sec +
                                   task->cpu_usage.ru_stime.tv_usec));
        }
        break;
      }
      case WorkerTaskType::kGetHeapStatistics: {
        argc = 1;
        napi_create_object(wrap->env, &argv[0]);
        SetNamedDouble(wrap->env, argv[0], "total_heap_size", static_cast<double>(task->heap_statistics.total_heap_size));
        SetNamedDouble(wrap->env, argv[0], "total_heap_size_executable", static_cast<double>(task->heap_statistics.total_heap_size_executable));
        SetNamedDouble(wrap->env, argv[0], "total_physical_size", static_cast<double>(task->heap_statistics.total_physical_size));
        SetNamedDouble(wrap->env, argv[0], "total_available_size", static_cast<double>(task->heap_statistics.total_available_size));
        SetNamedDouble(wrap->env, argv[0], "used_heap_size", static_cast<double>(task->heap_statistics.used_heap_size));
        SetNamedDouble(wrap->env, argv[0], "heap_size_limit", static_cast<double>(task->heap_statistics.heap_size_limit));
        SetNamedDouble(wrap->env, argv[0], "malloced_memory", static_cast<double>(task->heap_statistics.malloced_memory));
        SetNamedDouble(wrap->env, argv[0], "peak_malloced_memory", static_cast<double>(task->heap_statistics.peak_malloced_memory));
        SetBool(wrap->env, argv[0], "does_zap_garbage", task->heap_statistics.does_zap_garbage);
        SetNamedDouble(wrap->env, argv[0], "number_of_native_contexts", static_cast<double>(task->heap_statistics.number_of_native_contexts));
        SetNamedDouble(wrap->env, argv[0], "number_of_detached_contexts", static_cast<double>(task->heap_statistics.number_of_detached_contexts));
        SetNamedDouble(wrap->env, argv[0], "total_global_handles_size", static_cast<double>(task->heap_statistics.total_global_handles_size));
        SetNamedDouble(wrap->env, argv[0], "used_global_handles_size", static_cast<double>(task->heap_statistics.used_global_handles_size));
        SetNamedDouble(wrap->env, argv[0], "external_memory", static_cast<double>(task->heap_statistics.external_memory));
        break;
      }
      case WorkerTaskType::kStartCpuProfile: {
        argc = 2;
        argv[0] = task->success ? CreateNull(wrap->env)
                                : CreateErrorWithCode(wrap->env, task->error_code, task->error_message);
        if (task->success) napi_create_uint32(wrap->env, task->started_profile_id, &argv[1]);
        break;
      }
      case WorkerTaskType::kStopCpuProfile:
      case WorkerTaskType::kStopHeapProfile: {
        argc = 2;
        argv[0] = task->success ? CreateNull(wrap->env)
                                : CreateErrorWithCode(wrap->env, task->error_code, task->error_message);
        if (task->success) {
          napi_create_string_utf8(wrap->env, task->json_data, task->json_len, &argv[1]);
        }
        break;
      }
      case WorkerTaskType::kStartHeapProfile: {
        argc = 1;
        argv[0] = task->success ? CreateNull(wrap->env)
                                : CreateErrorWithCode(wrap->env, task->error_code, task->error_message);
        break;
      }
      case WorkerTaskType::kTakeHeapSnapshot: {
        argc = 1;
        argv[0] = task->success ? CreateHeapSnapshotHandle(wrap->env, task.get())
                                : CreateErrorWithCode(wrap->env, task->error_code, task->error_message);
        break;
      }
    }

    napi_value ignored = nullptr;
    (void)EdgeMakeCallback(wrap->env, taker, ondone, argc, argv, &ignored);
    FreeWorkerTaskPayload(task.get());
  }
}

bool QueueWorkerTask(Worker* wrap, std::unique_ptr<WorkerTask> task) {
  if (wrap == nullptr || task == nullptr || !WorkerCanAcceptTask(wrap)) return false;
  napi_env worker_env = GetWorkerThreadEnv(wrap);
  if (worker_env == nullptr) return false;
  WorkerTask* raw_task = task.release();
  napi_status status = napi_generic_failure;
  if (auto* environment = EdgeEnvironmentGet(worker_env); environment != nullptr) {
    status = environment->RequestInterrupt(OnWorkerTaskInterrupt, raw_task);
  } else {
    status = unofficial_napi_request_interrupt(worker_env, OnWorkerTaskInterrupt, raw_task);
  }
  if (status != napi_ok) {
    task.reset(raw_task);
    return false;
  }
  return true;
}

void CallWorkerOnExit(Worker* wrap) {
  if (wrap == nullptr ||
      wrap->parent_env_closing.load(std::memory_order_acquire) ||
      wrap->env == nullptr ||
      wrap->wrapper_ref == nullptr) {
    return;
  }
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return;
  napi_value onexit = GetNamed(wrap->env, self, "onexit");
  if (!IsFunction(wrap->env, onexit)) return;
  std::string effective_custom_err = wrap->custom_err;
  std::string effective_custom_err_reason = wrap->custom_err_reason;
  napi_value argv[3] = {nullptr, CreateNull(wrap->env), CreateNull(wrap->env)};
  napi_create_int32(wrap->env, wrap->exit_code, &argv[0]);
  if (!effective_custom_err.empty()) {
    napi_create_string_utf8(wrap->env, effective_custom_err.c_str(), NAPI_AUTO_LENGTH, &argv[1]);
  }
  if (!effective_custom_err_reason.empty()) {
    napi_create_string_utf8(wrap->env, effective_custom_err_reason.c_str(), NAPI_AUTO_LENGTH, &argv[2]);
  }
  napi_value ignored = nullptr;
  (void)EdgeMakeCallback(wrap->env, self, onexit, 3, argv, &ignored);
}

void ResetWorkerMessagePort(Worker* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr) return;
  napi_value self = GetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return;
  napi_value undefined = Undefined(wrap->env);
  if (undefined == nullptr) return;
  (void)napi_set_named_property(wrap->env, self, "messagePort", undefined);
}

void CloseWorkerMessagePort(Worker* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->message_port_ref == nullptr) return;
  napi_value port = GetRefValue(wrap->env, wrap->message_port_ref);
  if (port == nullptr) return;
  EdgeCloseMessagePortForValue(wrap->env, port);
}

void UnrefWorkerMessagePort(Worker* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->message_port_ref == nullptr) return;
  napi_value port = GetRefValue(wrap->env, wrap->message_port_ref);
  if (port == nullptr) return;
  napi_value unref = GetNamed(wrap->env, port, "unref");
  if (!IsFunction(wrap->env, unref)) return;
  napi_value ignored = nullptr;
  (void)napi_call_function(wrap->env, port, unref, 0, nullptr, &ignored);
}

void FinishWorkerOnParentThread(Worker* wrap) {
  if (wrap == nullptr || wrap->env == nullptr) return;
  if (wrap->parent_env_closing.load(std::memory_order_acquire)) {
    CleanupWorkerForParentEnvShutdown(wrap);
    return;
  }
  if (wrap->has_ref.load(std::memory_order_acquire)) {
    (void)EdgeRuntimePlatformReleaseRef(wrap->env);
  }
  (void)JoinWorkerThread(wrap);
  wrap->started.store(false, std::memory_order_release);
  RemoveWorkerFromRegistry(wrap);
  wrap->resource_alive.store(false, std::memory_order_release);
  if (wrap->active_handle_token != nullptr) {
    EdgeUnregisterActiveHandle(wrap->env, wrap->active_handle_token);
    wrap->active_handle_token = nullptr;
  }
  if (wrap->async_id > 0) {
    EdgeAsyncWrapQueueDestroyId(wrap->env, wrap->async_id);
    wrap->async_id = 0;
  }
  ResetWorkerMessagePort(wrap);
  CallWorkerOnExit(wrap);
  CloseWorkerMessagePort(wrap);
  DeleteRefIfPresent(wrap->env, &wrap->message_port_ref);
  ClearCompletedWorkerTasks(wrap->env, wrap);
  DetachWorkerFromJs(wrap);
  CloseWorkerCompletionAsync(wrap);
  delete wrap;
}

void OnWorkerParentCompletionTask(napi_env /*env*/, void* data) {
  FinishWorkerOnParentThread(static_cast<Worker*>(data));
}

void QueueWorkerParentCompletion(Worker* wrap) {
  if (wrap == nullptr ||
      wrap->env == nullptr ||
      wrap->parent_env_closing.load(std::memory_order_acquire) ||
      wrap->parent_completion_queued.exchange(true, std::memory_order_acq_rel)) {
    return;
  }
  if (EdgeRuntimePlatformEnqueueForegroundTask(
          wrap->env,
          OnWorkerParentCompletionTask,
          wrap,
          nullptr,
          0) != napi_ok) {
    wrap->parent_completion_queued.store(false, std::memory_order_release);
  }
}

void OnWorkerStopAsync(uv_async_t* handle) {
  auto* thread_data = static_cast<WorkerThreadData*>(handle != nullptr ? handle->data : nullptr);
  Worker* wrap = thread_data != nullptr ? thread_data->worker : nullptr;
  if (wrap == nullptr || !wrap->stop_requested.load(std::memory_order_acquire)) return;
  uv_loop_t* loop = handle->loop;
  if (loop != nullptr) uv_stop(loop);
}

bool ShouldAbortWorkerBootstrap(Worker* wrap,
                                int* exit_code,
                                std::string* custom_err,
                                std::string* custom_err_reason) {
  if (wrap == nullptr || !wrap->stop_requested.load(std::memory_order_acquire)) {
    return false;
  }
  if (exit_code != nullptr) {
    *exit_code = wrap->requested_exit_code;
  }
  if (custom_err != nullptr) {
    custom_err->clear();
  }
  if (custom_err_reason != nullptr) {
    custom_err_reason->clear();
  }
  return true;
}

void FinalizeWorkerThread(Worker* wrap, int exit_code, const std::string& custom_err, const std::string& custom_err_reason) {
  if (wrap == nullptr) return;
  {
    std::lock_guard<std::mutex> lock(wrap->mutex);
    if (wrap->thread_data != nullptr) {
      wrap->thread_data->env = nullptr;
      wrap->thread_data->scope = nullptr;
    }
    wrap->exit_code = exit_code;
    wrap->custom_err = custom_err;
    wrap->custom_err_reason = custom_err_reason;
  }
  if (wrap->parent_env_closing.load(std::memory_order_acquire)) {
    return;
  }
  QueueWorkerParentCompletion(wrap);
}

size_t WorkerNearHeapLimit(napi_env /*env*/,
                           void* data,
                           size_t current_heap_limit,
                           size_t /*initial_heap_limit*/) {
  auto* wrap = static_cast<Worker*>(data);
  if (wrap == nullptr) return current_heap_limit;
  wrap->reached_heap_limit.store(true, std::memory_order_release);
  wrap->requested_exit_code = 1;
  RequestWorkerStop(wrap);
  constexpr size_t kExtraHeapAllowance = 16 * 1024 * 1024;
  return current_heap_limit + kExtraHeapAllowance;
}

void WorkerThreadMain(Worker* wrap, uintptr_t stack_top) {
  int exit_code = 0;
  std::string custom_err;
  std::string custom_err_reason;
  WorkerThreadData* thread_data = wrap != nullptr ? wrap->thread_data.get() : nullptr;
  napi_env worker_env = nullptr;
  void* worker_scope = nullptr;
  uv_loop_t* worker_loop = nullptr;
  if (wrap != nullptr) {
    wrap->stack_base = stack_top > (wrap->thread_stack_size - kWorkerStackBufferSize)
                           ? stack_top - (wrap->thread_stack_size - kWorkerStackBufferSize)
                           : 0;
  }
  worker_loop = new (std::nothrow) uv_loop_t();
  if (worker_loop == nullptr) {
    FinalizeWorkerThread(wrap, 1, "ERR_WORKER_INIT_FAILED", "Failed to allocate worker event loop");
    return;
  }
  int loop_rc = uv_loop_init(worker_loop);
  if (loop_rc != 0) {
    delete worker_loop;
    FinalizeWorkerThread(
        wrap,
        1,
        "ERR_WORKER_INIT_FAILED",
        uv_strerror(loop_rc) != nullptr ? uv_strerror(loop_rc) : "Failed to initialize worker event loop");
    return;
  }
  (void)uv_loop_configure(worker_loop, UV_METRICS_IDLE_TIME);
  if (wrap != nullptr) {
    wrap->worker_config.external_event_loop = worker_loop;
  }

  unofficial_napi_env_create_options create_options{};
  if (wrap != nullptr) {
    if (wrap->worker_config.resource_limits[0] > 0) {
      create_options.max_young_generation_size_in_bytes =
          static_cast<size_t>(wrap->worker_config.resource_limits[0] * kWorkerMb);
    }
    if (wrap->worker_config.resource_limits[1] > 0) {
      create_options.max_old_generation_size_in_bytes =
          static_cast<size_t>(wrap->worker_config.resource_limits[1] * kWorkerMb);
    }
    if (wrap->worker_config.resource_limits[2] > 0) {
      create_options.code_range_size_in_bytes =
          static_cast<size_t>(wrap->worker_config.resource_limits[2] * kWorkerMb);
    }
    if (wrap->stack_base != 0) {
      create_options.stack_limit = reinterpret_cast<void*>(wrap->stack_base);
    }
  }
  EdgeInstallNapiEmbedderHooks();
  if (unofficial_napi_create_env_with_options(8, &create_options, &worker_env, &worker_scope) != napi_ok ||
      worker_env == nullptr || worker_scope == nullptr) {
    EdgeEnvironmentDestroyReleasedEventLoop(worker_loop);
    FinalizeWorkerThread(wrap, 1, "ERR_WORKER_INIT_FAILED", "Failed to create worker env");
    return;
  }

  if (wrap != nullptr) {
    std::lock_guard<std::mutex> lock(wrap->mutex);
    wrap->worker_config.env_message_port_data = std::move(wrap->child_port_data);
  }
  if (!EdgeAttachEnvironmentForRuntime(worker_env, &wrap->worker_config)) {
    uv_loop_t* shutdown_loop = EdgeEnvironmentReleaseEventLoop(worker_env);
    if (shutdown_loop == nullptr) shutdown_loop = worker_loop;
    (void)unofficial_napi_release_env_with_loop(worker_scope, shutdown_loop);
    EdgeEnvironmentDestroyReleasedEventLoop(shutdown_loop);
    wrap->worker_config.external_event_loop = nullptr;
    FinalizeWorkerThread(wrap, 1, "ERR_WORKER_INIT_FAILED", "Failed to attach worker env");
    return;
  }
  if (wrap->stack_base != 0) {
    (void)unofficial_napi_set_stack_limit(worker_env, reinterpret_cast<void*>(wrap->stack_base));
  }
  (void)unofficial_napi_set_near_heap_limit_callback(worker_env, WorkerNearHeapLimit, wrap);
  {
    std::lock_guard<std::mutex> lock(wrap->mutex);
    wrap->worker_config.env_message_port_data.reset();
    if (thread_data != nullptr) {
      thread_data->env = worker_env;
      thread_data->scope = worker_scope;
    }
  }
  wrap->loop_start_time_ms.store(static_cast<double>(uv_hrtime()) / 1e6, std::memory_order_release);
  uv_loop_t* loop = EdgeGetEnvLoop(worker_env);
  if (loop == nullptr) {
    custom_err = "ERR_WORKER_INIT_FAILED";
    custom_err_reason = "Failed to initialize worker event loop";
    exit_code = 1;
  } else if (ShouldAbortWorkerBootstrap(wrap, &exit_code, &custom_err, &custom_err_reason)) {
    goto cleanup_worker_env;
  } else if (thread_data == nullptr ||
             uv_async_init(loop, &thread_data->stop_async, OnWorkerStopAsync) != 0) {
    custom_err = "ERR_WORKER_INIT_FAILED";
    custom_err_reason = "Failed to initialize worker stop handle";
    exit_code = 1;
  } else {
    thread_data->stop_async.data = thread_data;
    uv_unref(reinterpret_cast<uv_handle_t*>(&thread_data->stop_async));
    thread_data->stop_async_initialized.store(true, std::memory_order_release);

    if (!ShouldAbortWorkerBootstrap(wrap, &exit_code, &custom_err, &custom_err_reason)) {
      std::string run_error;
      exit_code = EdgeRunWorkerThreadMain(worker_env, wrap->exec_argv, &run_error);
      if (exit_code != 0 && !run_error.empty() && wrap->custom_err.empty()) {
        custom_err = "ERR_WORKER_INIT_FAILED";
        custom_err_reason = run_error;
      }
      if (wrap->reached_heap_limit.load(std::memory_order_acquire)) {
        exit_code = 1;
        custom_err = "ERR_WORKER_OUT_OF_MEMORY";
        custom_err_reason = "JS heap out of memory";
      } else if (wrap->stop_requested.load(std::memory_order_acquire)) {
        exit_code = wrap->requested_exit_code;
        custom_err.clear();
        custom_err_reason.clear();
      }
    }
  }

cleanup_worker_env:
  uv_loop_t* shutdown_loop = nullptr;
  if (thread_data != nullptr &&
      thread_data->stop_async_initialized.exchange(false, std::memory_order_acq_rel)) {
    uv_close(reinterpret_cast<uv_handle_t*>(&thread_data->stop_async), nullptr);
  }

  if (wrap->stop_requested.load(std::memory_order_acquire)) {
    bool has_pending = false;
    if (napi_is_exception_pending(worker_env, &has_pending) == napi_ok && has_pending) {
      napi_value ignored = nullptr;
      (void)napi_get_and_clear_last_exception(worker_env, &ignored);
    }
    (void)unofficial_napi_cancel_terminate_execution(worker_env);
  }

  (void)unofficial_napi_remove_near_heap_limit_callback(worker_env, 0);
  EdgeWorkerEnvRunCleanupPreserveLoop(worker_env);
  EdgeEnvironmentRunAtExitCallbacks(worker_env);
  if (exit_code == 0 && custom_err.empty() && custom_err_reason.empty()) {
    (void)unofficial_napi_low_memory_notification(worker_env);
  }
  shutdown_loop = EdgeWorkerEnvReleaseEventLoop(worker_env);
  if (shutdown_loop == nullptr) shutdown_loop = worker_loop;
  (void)unofficial_napi_release_env_with_loop(worker_scope, shutdown_loop);
  EdgeWorkerEnvDestroyReleasedEventLoop(shutdown_loop);
  wrap->worker_config.external_event_loop = nullptr;
  FinalizeWorkerThread(wrap, exit_code, custom_err, custom_err_reason);
}

void WorkerImplFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<Worker*>(data);
  if (wrap == nullptr) return;
  RemoveWorkerFromRegistry(wrap);
  if (wrap->started.load(std::memory_order_acquire) &&
      wrap->has_ref.load(std::memory_order_acquire)) {
    (void)EdgeRuntimePlatformReleaseRef(env);
  }
  RequestWorkerStop(wrap);
  (void)JoinWorkerThread(wrap);
  wrap->resource_alive.store(false, std::memory_order_release);
  if (wrap->active_handle_token != nullptr) {
    EdgeUnregisterActiveHandle(env, wrap->active_handle_token);
    wrap->active_handle_token = nullptr;
  }
  if (wrap->async_id > 0) {
    EdgeAsyncWrapQueueDestroyId(env, wrap->async_id);
    wrap->async_id = 0;
  }
  CloseWorkerMessagePort(wrap);
  DeleteRefIfPresent(env, &wrap->message_port_ref);
  DeleteRefIfPresent(env, &wrap->wrapper_ref);
  ClearCompletedWorkerTasks(env, wrap);
  CloseWorkerCompletionAsync(wrap);
  delete wrap;
}

napi_value CreateMessageChannel(napi_env env, napi_value* parent_port, EdgeMessagePortDataPtr* worker_port_data) {
  if (parent_port == nullptr || worker_port_data == nullptr) return nullptr;
  *parent_port = nullptr;
  *worker_port_data = nullptr;
  EdgeMessagePortDataPtr first = EdgeCreateMessagePortData();
  EdgeMessagePortDataPtr second = EdgeCreateMessagePortData();
  EdgeEntangleMessagePortData(first, second);
  napi_value parent = EdgeCreateMessagePortForData(env, first);
  if (parent == nullptr || IsNullOrUndefinedValue(env, parent)) return nullptr;
  *parent_port = parent;
  *worker_port_data = second;
  return parent;
}

napi_value WorkerImplCtor(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  auto* wrap = new Worker(env);
  wrap->thread_id = g_next_worker_thread_id.fetch_add(1, std::memory_order_relaxed);
  if (argc >= 7 && argv[6] != nullptr && !IsNullOrUndefinedValue(env, argv[6])) {
    const std::string name = ValueToUtf8(env, argv[6]);
    if (!name.empty()) wrap->thread_name = name;
  }

  wrap->worker_config.is_main_thread = false;
  wrap->worker_config.owns_process_state = false;
  wrap->worker_config.thread_id = wrap->thread_id;
  wrap->worker_config.thread_name = wrap->thread_name;
  {
    bool is_internal_thread = false;
    if (argc >= 6 && argv[5] != nullptr && !IsNullOrUndefinedValue(env, argv[5])) {
      (void)napi_get_value_bool(env, argv[5], &is_internal_thread);
    }
    wrap->worker_config.is_internal_thread = is_internal_thread;
  }
  if (argc >= 5 && argv[4] != nullptr && !IsNullOrUndefinedValue(env, argv[4])) {
    bool tracks_unmanaged_fds = false;
    (void)napi_get_value_bool(env, argv[4], &tracks_unmanaged_fds);
    wrap->worker_config.tracks_unmanaged_fds = tracks_unmanaged_fds;
  }
  if (argc >= 4 && argv[3] != nullptr) {
    wrap->worker_config.resource_limits = ReadResourceLimits(env, argv[3]);
  }

  const bool explicit_exec_argv = argc >= 3 && argv[2] != nullptr && !IsNullOrUndefinedValue(env, argv[2]);
  napi_value process = GetNamed(env, GetGlobal(env), "process");
  napi_value process_exec_argv = GetNamed(env, process, "execArgv");
  wrap->exec_argv = explicit_exec_argv ? ReadStringArray(env, argv[2]) : ReadStringArray(env, process_exec_argv);
  const std::vector<std::string> invalid_exec_argv = ValidateExecArgv(env, wrap->exec_argv, explicit_exec_argv);
  bool has_invalid_node_options = false;
  if (!invalid_exec_argv.empty()) {
    napi_value invalid = nullptr;
    if (napi_create_array_with_length(env, invalid_exec_argv.size(), &invalid) == napi_ok && invalid != nullptr) {
      for (size_t i = 0; i < invalid_exec_argv.size(); ++i) {
        napi_value item = nullptr;
        if (napi_create_string_utf8(env, invalid_exec_argv[i].c_str(), NAPI_AUTO_LENGTH, &item) == napi_ok && item != nullptr) {
          napi_set_element(env, invalid, static_cast<uint32_t>(i), item);
        }
      }
      napi_set_named_property(env, this_arg, "invalidExecArgv", invalid);
    }
  }

  if (argc >= 2 && argv[1] != nullptr) {
    napi_valuetype env_type = napi_undefined;
    if (napi_typeof(env, argv[1], &env_type) == napi_ok && env_type == napi_object && !IsNullOrUndefinedValue(env, argv[1])) {
      wrap->worker_config.share_env = false;
      (void)SnapshotStringMapFromObject(env, argv[1], &wrap->worker_config.env_vars);
      const std::vector<std::string> invalid_node_options = ValidateNodeOptionsEnv(env, wrap->worker_config.env_vars);
      if (!invalid_node_options.empty()) {
        has_invalid_node_options = true;
        napi_value invalid = nullptr;
        if (napi_create_array_with_length(env, invalid_node_options.size(), &invalid) == napi_ok && invalid != nullptr) {
          for (size_t i = 0; i < invalid_node_options.size(); ++i) {
            napi_value item = nullptr;
            if (napi_create_string_utf8(env, invalid_node_options[i].c_str(), NAPI_AUTO_LENGTH, &item) == napi_ok && item != nullptr) {
              napi_set_element(env, invalid, static_cast<uint32_t>(i), item);
            }
          }
          napi_set_named_property(env, this_arg, "invalidNodeOptions", invalid);
        }
      }
    } else if (IsNullValue(env, argv[1])) {
      wrap->worker_config.share_env = false;
      wrap->worker_config.env_vars = SnapshotCurrentProcessEnv(env);
    } else {
      wrap->worker_config.share_env = true;
    }
  } else {
    wrap->worker_config.share_env = false;
    wrap->worker_config.env_vars = SnapshotCurrentProcessEnv(env);
  }

  if (!invalid_exec_argv.empty() || has_invalid_node_options) {
    SetInt32(env, this_arg, "threadId", wrap->thread_id);
    SetString(env, this_arg, "threadName", wrap->thread_name.c_str());
    delete wrap;
    return this_arg;
  }

  napi_value parent_port = nullptr;
  if (CreateMessageChannel(env, &parent_port, &wrap->child_port_data) == nullptr ||
      parent_port == nullptr) {
    delete wrap;
    return nullptr;
  }
  SetRefValue(env, &wrap->message_port_ref, parent_port);

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  wrap->completion_async = new uv_async_t();
  if (loop == nullptr ||
      wrap->completion_async == nullptr ||
      uv_async_init(loop, wrap->completion_async, OnWorkerCompletionAsync) != 0) {
    delete wrap->completion_async;
    wrap->completion_async = nullptr;
    delete wrap;
    return nullptr;
  }
  wrap->completion_async->data = wrap;
  wrap->completion_async_initialized.store(true, std::memory_order_release);
  uv_unref(reinterpret_cast<uv_handle_t*>(wrap->completion_async));

  if (napi_wrap(env, this_arg, wrap, WorkerImplFinalize, nullptr, &wrap->wrapper_ref) != napi_ok) {
    CloseWorkerCompletionAsync(wrap);
    delete wrap;
    return nullptr;
  }

  wrap->async_id = EdgeAsyncWrapNextId(env);
  EdgeAsyncWrapEmitInit(env, wrap->async_id, kEdgeProviderWorker, EdgeAsyncWrapExecutionAsyncId(env), this_arg);

  napi_set_named_property(env, this_arg, "messagePort", parent_port);
  SetInt32(env, this_arg, "threadId", wrap->thread_id);
  SetString(env, this_arg, "threadName", wrap->thread_name.c_str());
  return this_arg;
}

napi_value WorkerImplStartThread(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  bool expected = false;
  if (!wrap->started.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
    return Undefined(env);
  }

  uint32_t ref_count = 0;
  if (wrap->wrapper_ref != nullptr) (void)napi_reference_ref(env, wrap->wrapper_ref, &ref_count);
  if (wrap->has_ref.load(std::memory_order_acquire) &&
      EdgeRuntimePlatformAddRef(env) != napi_ok) {
    wrap->started.store(false, std::memory_order_release);
    if (wrap->wrapper_ref != nullptr) (void)napi_reference_unref(env, wrap->wrapper_ref, &ref_count);
    napi_throw_error(env, "ERR_WORKER_INIT_FAILED", "Failed to retain parent event loop");
    return Undefined(env);
  }

  if (wrap->worker_config.resource_limits[3] > 0) {
    wrap->thread_stack_size =
        static_cast<size_t>(wrap->worker_config.resource_limits[3] * kWorkerMb);
    if (wrap->thread_stack_size < kWorkerStackBufferSize) {
      wrap->thread_stack_size = kWorkerStackBufferSize;
      wrap->worker_config.resource_limits[3] =
          static_cast<double>(kWorkerStackBufferSize) / kWorkerMb;
    }
  } else {
    wrap->thread_stack_size = kDefaultWorkerStackSize;
    wrap->worker_config.resource_limits[3] =
        static_cast<double>(wrap->thread_stack_size) / kWorkerMb;
  }

  uv_thread_options_t thread_options;
  thread_options.flags = UV_THREAD_HAS_STACK_SIZE;
  thread_options.stack_size = wrap->thread_stack_size;
  wrap->thread_joined.store(false, std::memory_order_release);
  const int thread_rc = uv_thread_create_ex(
      &wrap->thread,
      &thread_options,
      [](void* arg) {
        auto* wrap = static_cast<Worker*>(arg);
        const uintptr_t stack_top = reinterpret_cast<uintptr_t>(&arg);
        uv_thread_setname(wrap->thread_name.c_str());
        WorkerThreadMain(wrap, stack_top);
      },
      wrap);
  if (thread_rc != 0) {
    wrap->started.store(false, std::memory_order_release);
    wrap->thread_joined.store(true, std::memory_order_release);
    if (wrap->has_ref.load(std::memory_order_acquire)) {
      (void)EdgeRuntimePlatformReleaseRef(env);
    }
    if (wrap->wrapper_ref != nullptr) (void)napi_reference_unref(env, wrap->wrapper_ref, &ref_count);
    napi_throw_error(env, "ERR_WORKER_INIT_FAILED", uv_strerror(thread_rc));
  } else {
    wrap->resource_alive.store(true, std::memory_order_release);
    if (wrap->active_handle_token == nullptr) {
      wrap->active_handle_token =
          EdgeRegisterActiveHandle(env, this_arg, "WORKER", WorkerImplHasRefActive, WorkerImplGetActiveOwner, wrap);
    }
    AddWorkerToRegistry(wrap);
  }
  return Undefined(env);
}

napi_value WorkerImplStopThread(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap == nullptr || !wrap->started.load(std::memory_order_acquire)) return Undefined(env);
  UnrefWorkerMessagePort(wrap);
  RequestWorkerStop(wrap);
  return Undefined(env);
}

napi_value WorkerImplRef(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap != nullptr) {
    const bool was_refed = wrap->has_ref.exchange(true, std::memory_order_acq_rel);
    if (!was_refed && wrap->started.load(std::memory_order_acquire)) {
      (void)EdgeRuntimePlatformAddRef(env);
    }
  }
  return Undefined(env);
}

napi_value WorkerImplUnref(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap != nullptr) {
    const bool was_refed = wrap->has_ref.exchange(false, std::memory_order_acq_rel);
    if (was_refed && wrap->started.load(std::memory_order_acquire)) {
      (void)EdgeRuntimePlatformReleaseRef(env);
    }
  }
  return Undefined(env);
}

napi_value WorkerImplHasRef(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  napi_value out = nullptr;
  if (wrap == nullptr) return Undefined(env);
  napi_get_boolean(env, wrap->has_ref.load(std::memory_order_acquire), &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value WorkerImplGetResourceLimits(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  return BuildResourceLimitsArray(env, wrap->worker_config.resource_limits);
}

napi_value WorkerImplLoopStartTime(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  const double value = wrap != nullptr ? wrap->loop_start_time_ms.load(std::memory_order_acquire) : -1;
  napi_value out = nullptr;
  napi_create_double(env, value, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value WorkerImplLoopIdleTime(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  if (wrap == nullptr) return Undefined(env);
  napi_env worker_env = GetWorkerThreadEnv(wrap);
  if (worker_env == nullptr) {
    napi_value out = nullptr;
    napi_create_int32(env, -1, &out);
    return out != nullptr ? out : Undefined(env);
  }
  uv_loop_t* loop = EdgeGetEnvLoop(worker_env);
  napi_value out = nullptr;
  napi_create_double(
      env,
      loop != nullptr ? static_cast<double>(uv_metrics_idle_time(loop)) / 1e6 : -1,
      &out);
  return out != nullptr ? out : Undefined(env);
}

std::unique_ptr<WorkerTask> CreateWorkerTask(Worker* wrap, WorkerTaskType type) {
  auto task = std::make_unique<WorkerTask>();
  task->wrap = wrap;
  task->type = type;
  return task;
}

napi_value QueueWorkerTakerTask(napi_env env, Worker* wrap, std::unique_ptr<WorkerTask> task) {
  if (wrap == nullptr || task == nullptr) return Undefined(env);
  napi_value taker = CreateTakerObject(env);
  if (taker == nullptr) return Undefined(env);
  if (napi_create_reference(env, taker, 1, &task->taker_ref) != napi_ok || task->taker_ref == nullptr) {
    return Undefined(env);
  }
  WorkerTask* raw_task = task.get();
  if (!QueueWorkerTask(wrap, std::move(task))) {
    DeleteWorkerTaskRef(env, raw_task);
    return Undefined(env);
  }
  return taker;
}

napi_value WorkerImplCpuUsage(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  return QueueWorkerTakerTask(env, wrap, CreateWorkerTask(wrap, WorkerTaskType::kCpuUsage));
}

napi_value WorkerImplGetHeapStatistics(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  return QueueWorkerTakerTask(env, wrap, CreateWorkerTask(wrap, WorkerTaskType::kGetHeapStatistics));
}

napi_value WorkerImplStartCpuProfile(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  return QueueWorkerTakerTask(env, wrap, CreateWorkerTask(wrap, WorkerTaskType::kStartCpuProfile));
}

napi_value WorkerImplStopCpuProfile(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  auto task = CreateWorkerTask(wrap, WorkerTaskType::kStopCpuProfile);
  if (task && argc >= 1 && argv[0] != nullptr) {
    (void)napi_get_value_uint32(env, argv[0], &task->profile_id);
  }
  return QueueWorkerTakerTask(env, wrap, std::move(task));
}

napi_value WorkerImplStartHeapProfile(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  return QueueWorkerTakerTask(env, wrap, CreateWorkerTask(wrap, WorkerTaskType::kStartHeapProfile));
}

napi_value WorkerImplStopHeapProfile(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  return QueueWorkerTakerTask(env, wrap, CreateWorkerTask(wrap, WorkerTaskType::kStopHeapProfile));
}

napi_value WorkerImplTakeHeapSnapshot(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr);
  Worker* wrap = UnwrapWorker(env, this_arg);
  auto task = CreateWorkerTask(wrap, WorkerTaskType::kTakeHeapSnapshot);
  if (task && argc >= 1 && argv[0] != nullptr) {
    bool is_typed_array = false;
    napi_typedarray_type type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    napi_value arraybuffer = nullptr;
    size_t byte_offset = 0;
    if (napi_is_typedarray(env, argv[0], &is_typed_array) == napi_ok &&
        is_typed_array &&
        napi_get_typedarray_info(env, argv[0], &type, &length, &data, &arraybuffer, &byte_offset) == napi_ok &&
        type == napi_uint8_array &&
        data != nullptr &&
        length >= 2) {
      const uint8_t* values = static_cast<const uint8_t*>(data);
      task->heap_snapshot_options.expose_internals = values[0] != 0;
      task->heap_snapshot_options.expose_numeric_values = values[1] != 0;
    }
  }
  return QueueWorkerTakerTask(env, wrap, std::move(task));
}

napi_value CreateWorkerCtor(napi_env env) {
  static constexpr napi_property_descriptor kProps[] = {
      {"startThread", nullptr, WorkerImplStartThread, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"stopThread", nullptr, WorkerImplStopThread, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"hasRef", nullptr, WorkerImplHasRef, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"ref", nullptr, WorkerImplRef, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"unref", nullptr, WorkerImplUnref, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getResourceLimits", nullptr, WorkerImplGetResourceLimits, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"loopStartTime", nullptr, WorkerImplLoopStartTime, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"loopIdleTime", nullptr, WorkerImplLoopIdleTime, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"takeHeapSnapshot", nullptr, WorkerImplTakeHeapSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"getHeapStatistics", nullptr, WorkerImplGetHeapStatistics, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"cpuUsage", nullptr, WorkerImplCpuUsage, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"startCpuProfile", nullptr, WorkerImplStartCpuProfile, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"stopCpuProfile", nullptr, WorkerImplStopCpuProfile, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"startHeapProfile", nullptr, WorkerImplStartHeapProfile, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"stopHeapProfile", nullptr, WorkerImplStopHeapProfile, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "Worker",
                        NAPI_AUTO_LENGTH,
                        WorkerImplCtor,
                        nullptr,
                        sizeof(kProps) / sizeof(kProps[0]),
                        kProps,
                        &ctor) != napi_ok) {
    return nullptr;
  }
  return ctor;
}

napi_value GetOrCreateEnvMessagePort(napi_env env) {
  napi_value cached = EdgeWorkerEnvGetEnvMessagePort(env);
  if (cached != nullptr && !IsNullOrUndefinedValue(env, cached)) return cached;
  EdgeMessagePortDataPtr data = EdgeWorkerEnvGetEnvMessagePortData(env);
  if (!data) return Undefined(env);
  napi_value port = EdgeCreateMessagePortForData(env, data);
  if (port == nullptr || IsNullOrUndefinedValue(env, port)) return Undefined(env);
  EdgeWorkerEnvSetEnvMessagePort(env, port);
  return port;
}

napi_value WorkerGetEnvMessagePort(napi_env env, napi_callback_info /*info*/) {
  return GetOrCreateEnvMessagePort(env);
}

}  // namespace

napi_value InitWorker(napi_env env) {
  napi_value cached = EdgeWorkerEnvGetBinding(env);
  if (cached != nullptr) return cached;

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  const bool is_main_thread = EdgeWorkerEnvIsMainThread(env);
  SetBool(env, out, "isMainThread", is_main_thread);
  SetBool(env, out, "isInternalThread", EdgeWorkerEnvIsInternalThread(env));
  SetBool(env, out, "ownsProcessState", EdgeWorkerEnvOwnsProcessState(env));
  SetInt32(env, out, "threadId", EdgeWorkerEnvThreadId(env));
  SetString(env, out, "threadName", EdgeWorkerEnvThreadName(env).c_str());

  napi_value resource_limits = is_main_thread ? nullptr : BuildResourceLimitsArray(env, EdgeWorkerEnvResourceLimits(env));
  if (resource_limits == nullptr) {
    napi_create_object(env, &resource_limits);
  }
  if (resource_limits != nullptr) napi_set_named_property(env, out, "resourceLimits", resource_limits);

  SetInt32(env, out, "kMaxYoungGenerationSizeMb", 0);
  SetInt32(env, out, "kMaxOldGenerationSizeMb", 1);
  SetInt32(env, out, "kCodeRangeSizeMb", 2);
  SetInt32(env, out, "kStackSizeMb", 3);
  SetInt32(env, out, "kTotalResourceLimitCount", 4);

  napi_value worker_ctor = CreateWorkerCtor(env);
  if (worker_ctor != nullptr) napi_set_named_property(env, out, "Worker", worker_ctor);

  napi_value get_env_message_port = nullptr;
  if (napi_create_function(env,
                           "getEnvMessagePort",
                           NAPI_AUTO_LENGTH,
                           WorkerGetEnvMessagePort,
                           nullptr,
                           &get_env_message_port) == napi_ok &&
      get_env_message_port != nullptr) {
    napi_set_named_property(env, out, "getEnvMessagePort", get_env_message_port);
  }

  EdgeWorkerEnvSetBinding(env, out);
  return out;
}

std::vector<EdgeWorkerReportEntry> EdgeWorkerCollectReports(napi_env env) {
  std::vector<Worker*> wraps;
  {
    std::lock_guard<std::mutex> lock(g_worker_registry_mu);
    auto* state = GetWorkerParentStateLocked(env);
    if (state != nullptr) {
      wraps.assign(state->workers.begin(), state->workers.end());
    }
  }

  WorkerReportTaskState state;
  for (Worker* wrap : wraps) {
    if (wrap == nullptr || !wrap->started.load(std::memory_order_acquire)) continue;

    napi_env worker_env = GetWorkerThreadEnv(wrap);
    std::string thread_name;
    {
      std::lock_guard<std::mutex> lock(wrap->mutex);
      thread_name = wrap->thread_name;
    }
    if (worker_env == nullptr) continue;

    auto* task = new (std::nothrow) WorkerReportTask();
    if (task == nullptr) continue;
    task->state = &state;
    task->thread_id = wrap->thread_id;
    task->thread_name = std::move(thread_name);

    {
      std::lock_guard<std::mutex> lock(state.mutex);
      state.pending += 1;
    }
    napi_status status = napi_generic_failure;
    if (auto* environment = EdgeEnvironmentGet(worker_env); environment != nullptr) {
      status = environment->RequestInterrupt(OnWorkerReportInterrupt, task);
    } else {
      status = unofficial_napi_request_interrupt(worker_env, OnWorkerReportInterrupt, task);
    }
    if (status != napi_ok) {
      CompleteWorkerReportTask(task, {});
    }
  }

  std::unique_lock<std::mutex> lock(state.mutex);
  state.cv.wait(lock, [&state]() { return state.pending == 0; });
  return state.reports;
}

}  // namespace internal_binding

std::vector<EdgeWorkerReportEntry> EdgeWorkerCollectReports(napi_env env) {
  return internal_binding::EdgeWorkerCollectReports(env);
}

void EdgeWorkerStopAllForEnv(napi_env env) {
  internal_binding::StopWorkersForEnv(env);
}
