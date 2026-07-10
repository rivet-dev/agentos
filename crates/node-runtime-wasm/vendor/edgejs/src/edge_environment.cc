#include "edge_environment.h"

#include <algorithm>
#include <cmath>
#include <memory>
#include <new>
#include <utility>

#include "edge_runtime_platform.h"
#include "edge_handle_scope.h"
#include "edge_timers_host.h"
#include "edge_worker_env.h"
#include "unofficial_napi.h"

namespace {

std::mutex g_environment_mu;
std::unordered_map<napi_env, std::unique_ptr<edge::Environment>> g_environments;
constexpr int kImmediateCount = 0;
constexpr int kImmediateRefCount = 1;
constexpr int kImmediateHasOutstanding = 2;

struct DetachedSlotState {
  std::unordered_map<size_t, edge::SlotEntry> slots;
  bool cleanup_hook_registered = false;
};

std::unordered_map<napi_env, DetachedSlotState> g_detached_slots;

void AttachedEnvCleanup(napi_env env, void* /*data*/) {
  if (auto* environment = edge::Environment::Get(env); environment != nullptr) {
    environment->RunCleanup();
    environment->RunAtExitCallbacks();
  }
}

void AttachedEnvDestroy(napi_env env, void* /*data*/) {
  edge::Environment::Detach(env);
}

void AttachedEnvAssignContextToken(napi_env env, void* token, void* /*data*/) {
  if (auto* environment = edge::Environment::Get(env); environment != nullptr) {
    environment->AssignToContext(token);
  }
}

void AttachedEnvUnassignContextToken(napi_env env, void* token, void* /*data*/) {
  if (auto* environment = edge::Environment::Get(env); environment != nullptr) {
    environment->UnassignFromContext(token);
  }
}

bool RegisterAttachedEnvHooks(napi_env env) {
#if defined(EDGE_EMBEDDED_NAPI_PROVIDER)
  if (unofficial_napi_set_edge_environment(env, edge::Environment::Get(env)) != napi_ok) {
    return false;
  }
  if (unofficial_napi_set_env_cleanup_callback(env, AttachedEnvCleanup, nullptr) != napi_ok) {
    (void)unofficial_napi_set_edge_environment(env, nullptr);
    return false;
  }
  if (unofficial_napi_set_env_destroy_callback(env, AttachedEnvDestroy, nullptr) != napi_ok) {
    (void)unofficial_napi_set_env_cleanup_callback(env, nullptr, nullptr);
    (void)unofficial_napi_set_edge_environment(env, nullptr);
    return false;
  }
  if (unofficial_napi_set_context_token_callbacks(
          env, AttachedEnvAssignContextToken, AttachedEnvUnassignContextToken, nullptr) != napi_ok) {
    (void)unofficial_napi_set_env_destroy_callback(env, nullptr, nullptr);
    (void)unofficial_napi_set_env_cleanup_callback(env, nullptr, nullptr);
    (void)unofficial_napi_set_edge_environment(env, nullptr);
    return false;
  }
#endif
  return true;
}

void RunSlotDeleters(std::unordered_map<size_t, edge::SlotEntry>* slots) {
  if (slots == nullptr) return;
  for (auto& [_, slot] : *slots) {
    if (slot.deleter != nullptr && slot.data != nullptr) {
      slot.deleter(slot.data);
    }
  }
  slots->clear();
}

void OnDetachedEnvSlotsCleanup(void* data) {
  napi_env env = static_cast<napi_env>(data);
  std::unordered_map<size_t, edge::SlotEntry> slots;
  {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto it = g_detached_slots.find(env);
    if (it == g_detached_slots.end()) return;
    slots.swap(it->second.slots);
    g_detached_slots.erase(it);
  }
  RunSlotDeleters(&slots);
}

void CloseEnvLoopHandles(uv_loop_t* loop) {
  if (loop == nullptr) return;
  uv_walk(
      loop,
      [](uv_handle_t* handle, void*) {
        if (handle == nullptr || uv_is_closing(handle) != 0) return;
        uv_close(handle, nullptr);
      },
      nullptr);
}

bool DrainAndCloseEnvLoop(uv_loop_t* loop) {
  if (loop == nullptr) return true;

  for (size_t guard = 0; guard < 256; ++guard) {
    if (uv_run(loop, UV_RUN_NOWAIT) == 0 && uv_loop_close(loop) == 0) {
      return true;
    }
  }

  for (size_t guard = 0; guard < 64; ++guard) {
    CloseEnvLoopHandles(loop);
    (void)uv_run(loop, UV_RUN_NOWAIT);
    if (uv_loop_close(loop) == 0) {
      return true;
    }
  }

  return false;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) return nullptr;
  return value;
}

bool IsStrictEqual(napi_env env, napi_value lhs, napi_value rhs) {
  if (env == nullptr || lhs == nullptr || rhs == nullptr) return false;
  bool same = false;
  return napi_strict_equals(env, lhs, rhs, &same) == napi_ok && same;
}

bool IsProcessStdioStream(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_value global = nullptr;
  napi_value process = nullptr;
  if (napi_get_global(env, &global) != napi_ok ||
      global == nullptr ||
      napi_get_named_property(env, global, "process", &process) != napi_ok ||
      process == nullptr) {
    return false;
  }

  napi_value candidate = nullptr;
  if ((napi_get_named_property(env, process, "stdin", &candidate) == napi_ok && IsStrictEqual(env, value, candidate)) ||
      (napi_get_named_property(env, process, "stdout", &candidate) == napi_ok && IsStrictEqual(env, value, candidate)) ||
      (napi_get_named_property(env, process, "stderr", &candidate) == napi_ok && IsStrictEqual(env, value, candidate))) {
    return true;
  }
  return false;
}

napi_value CreateArray(napi_env env) {
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}

void AppendArrayValue(napi_env env, napi_value array, uint32_t* index, napi_value value) {
  if (env == nullptr || array == nullptr || index == nullptr || value == nullptr) return;
  napi_set_element(env, array, (*index)++, value);
}

void AppendStringValue(napi_env env, napi_value array, uint32_t* index, const std::string& value) {
  if (env == nullptr || array == nullptr || index == nullptr || value.empty()) return;
  napi_value str = nullptr;
  if (napi_create_string_utf8(env, value.c_str(), value.size(), &str) != napi_ok || str == nullptr) return;
  AppendArrayValue(env, array, index, str);
}

void StopLoopOnJsError(edge::Environment* env) {
  if (env == nullptr) return;
  if (uv_loop_t* loop = env->GetExistingEventLoop(); loop != nullptr) {
    uv_stop(loop);
  }
}

}  // namespace

namespace edge {

struct ActiveHandleEntry {
  napi_ref keepalive_ref = nullptr;
  std::string resource_name;
  EdgeEnvironmentHandleHasRef has_ref = nullptr;
  EdgeEnvironmentHandleGetOwner get_owner = nullptr;
  EdgeEnvironmentHandleClose close_callback = nullptr;
  void* data = nullptr;
};

struct ActiveRequestEntry {
  napi_ref owner_ref = nullptr;
  std::string resource_name;
  EdgeEnvironmentRequestCancel cancel = nullptr;
  EdgeEnvironmentRequestGetOwner get_owner = nullptr;
  void* data = nullptr;
};

}  // namespace edge

namespace edge {

Environment* Environment::Get(napi_env env) {
  if (env == nullptr) return nullptr;
  std::lock_guard<std::mutex> lock(g_environment_mu);
  auto it = g_environments.find(env);
  return it == g_environments.end() ? nullptr : it->second.get();
}

Environment* Environment::Attach(napi_env env, const EdgeEnvironmentConfig& config) {
  if (env == nullptr) return nullptr;
  Environment* environment = nullptr;
  bool created = false;
  {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto& slot = g_environments[env];
    if (slot == nullptr) {
      slot = std::make_unique<Environment>(env);
      created = true;
    }
    slot->Configure(config);
    environment = slot.get();
  }
  if (created && !RegisterAttachedEnvHooks(env)) {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto it = g_environments.find(env);
    if (it != g_environments.end() && it->second.get() == environment) {
      g_environments.erase(it);
    }
    return nullptr;
  }
  return environment;
}

void Environment::Detach(napi_env env) {
  if (env == nullptr) return;
  std::unique_ptr<Environment> detached;
  {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto it = g_environments.find(env);
    if (it == g_environments.end()) return;
    detached = std::move(it->second);
    g_environments.erase(it);
  }
}

Environment::Environment(napi_env env) : env_(env) {
  principal_realm_.env = env;
}

Environment::~Environment() {
  ResetTrackedRefs();
  CloseTrackedUnmanagedFds();
  CloseAndDestroyEventLoop();
  CleanupActiveRegistryEntries();
  RunSlotDeleters(&slots_);
}

uint64_t Environment::DeriveFlags(const EdgeEnvironmentConfig& config) {
  uint64_t flags = config.flags;
  if ((flags & EnvironmentFlags::kDefaultFlags) != 0) {
    flags |= EnvironmentFlags::kOwnsProcessState | EnvironmentFlags::kOwnsInspector;
  }
  if (config.owns_process_state) {
    flags |= EnvironmentFlags::kOwnsProcessState;
  } else {
    flags &= ~static_cast<uint64_t>(EnvironmentFlags::kOwnsProcessState);
  }
  if (config.tracks_unmanaged_fds) {
    flags |= EnvironmentFlags::kTrackUnmanagedFds;
  } else {
    flags &= ~static_cast<uint64_t>(EnvironmentFlags::kTrackUnmanagedFds);
  }
  if (!config.is_main_thread || config.is_internal_thread) {
    flags &= ~static_cast<uint64_t>(EnvironmentFlags::kOwnsInspector);
  }
  return flags;
}

void Environment::Configure(const EdgeEnvironmentConfig& config) {
  std::lock_guard<std::mutex> lock(mutex_);
  config_ = config;
  flags_ = DeriveFlags(config);
  if (loop_ == nullptr && config.external_event_loop != nullptr) {
    loop_ = config.external_event_loop;
  }
}

EdgeEnvironmentConfig Environment::config() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_;
}

uint64_t Environment::flags() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return flags_;
}

bool Environment::is_main_thread() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.is_main_thread;
}

bool Environment::is_internal_thread() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.is_internal_thread;
}

bool Environment::owns_process_state() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return (flags_ & EnvironmentFlags::kOwnsProcessState) != 0;
}

bool Environment::shares_environment() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.share_env;
}

bool Environment::tracks_unmanaged_fds() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return (flags_ & EnvironmentFlags::kTrackUnmanagedFds) != 0;
}

bool Environment::stop_requested() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return stop_requested_;
}

bool Environment::exiting() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return exiting_;
}

void Environment::set_exiting(bool exiting) {
  std::lock_guard<std::mutex> lock(mutex_);
  exiting_ = exiting;
}

bool Environment::has_exit_code() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return has_exit_code_;
}

int Environment::exit_code(int default_code) const {
  std::lock_guard<std::mutex> lock(mutex_);
  return has_exit_code_ ? exit_code_ : default_code;
}

void Environment::set_exit_code(int code) {
  std::lock_guard<std::mutex> lock(mutex_);
  has_exit_code_ = true;
  exit_code_ = code;
}

void Environment::clear_exit_code() {
  std::lock_guard<std::mutex> lock(mutex_);
  has_exit_code_ = false;
  exit_code_ = 0;
}

int32_t Environment::thread_id() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.thread_id;
}

std::string Environment::thread_name() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.thread_name;
}

std::array<double, 4> Environment::resource_limits() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.resource_limits;
}

std::string Environment::process_title() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.local_process_title;
}

void Environment::set_process_title(const std::string& title) {
  std::lock_guard<std::mutex> lock(mutex_);
  config_.local_process_title = title;
}

uint32_t Environment::debug_port() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.local_debug_port;
}

void Environment::set_debug_port(uint32_t port) {
  std::lock_guard<std::mutex> lock(mutex_);
  config_.local_debug_port = port;
}

std::map<std::string, std::string> Environment::snapshot_env_vars() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.env_vars;
}

void Environment::set_local_env_var(const std::string& key, const std::string& value) {
  if (key.empty()) return;
  std::lock_guard<std::mutex> lock(mutex_);
  config_.env_vars[key] = value;
}

void Environment::unset_local_env_var(const std::string& key) {
  if (key.empty()) return;
  std::lock_guard<std::mutex> lock(mutex_);
  config_.env_vars.erase(key);
}

void Environment::RequestStop() {
  std::lock_guard<std::mutex> lock(mutex_);
  stop_requested_ = true;
}

void Environment::Exit(int exit_code) {
  ProcessExitHandler handler;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    exiting_ = true;
    has_exit_code_ = true;
    exit_code_ = exit_code;
    stop_requested_ = true;
    handler = process_exit_handler_;
  }

  if (handler != nullptr) {
    handler(this, exit_code);
    return;
  }

  if (uv_loop_t* loop = GetExistingEventLoop(); loop != nullptr) {
    uv_stop(loop);
  }
  (void)unofficial_napi_terminate_execution(env_);
}

void Environment::SetProcessExitHandler(ProcessExitHandler handler) {
  std::lock_guard<std::mutex> lock(mutex_);
  process_exit_handler_ = std::move(handler);
}

napi_value Environment::binding() const {
  napi_ref ref = nullptr;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    ref = binding_ref_;
  }
  return GetRefValue(env_, ref);
}

void Environment::DeleteRefIfPresent(napi_ref* ref) {
  if (env_ == nullptr || ref == nullptr || *ref == nullptr) return;
  (void)napi_delete_reference(env_, *ref);
  *ref = nullptr;
}

void Environment::set_binding(napi_value binding) {
  std::lock_guard<std::mutex> lock(mutex_);
  DeleteRefIfPresent(&binding_ref_);
  if (binding != nullptr) {
    (void)napi_create_reference(env_, binding, 1, &binding_ref_);
  }
}

napi_value Environment::env_message_port() const {
  napi_ref ref = nullptr;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    ref = env_message_port_ref_;
  }
  return GetRefValue(env_, ref);
}

void Environment::set_env_message_port(napi_value port) {
  std::lock_guard<std::mutex> lock(mutex_);
  DeleteRefIfPresent(&env_message_port_ref_);
  if (port != nullptr) {
    (void)napi_create_reference(env_, port, 1, &env_message_port_ref_);
  }
}

internal_binding::EdgeMessagePortDataPtr Environment::env_message_port_data() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return config_.env_message_port_data;
}

napi_status Environment::EnsureEventLoop(uv_loop_t** loop_out) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (loop_ == nullptr) {
    auto* loop = new (std::nothrow) uv_loop_t();
    if (loop == nullptr) return napi_generic_failure;
    if (uv_loop_init(loop) != 0) {
      delete loop;
      return napi_generic_failure;
    }
    (void)uv_loop_configure(loop, UV_METRICS_IDLE_TIME);
    loop_ = loop;
  }
  if (loop_out != nullptr) *loop_out = loop_;
  return napi_ok;
}

uv_loop_t* Environment::event_loop() {
  uv_loop_t* loop = nullptr;
  return EnsureEventLoop(&loop) == napi_ok ? loop : nullptr;
}

uv_loop_t* Environment::GetExistingEventLoop() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return loop_;
}

uv_loop_t* Environment::ReleaseEventLoop() {
  uv_loop_t* loop = nullptr;
  bool wait_for_threadsafe_async_close = false;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    wait_for_threadsafe_async_close = threadsafe_immediate_async_initialized_;
    ClosePerEnvHandlesLocked();
    loop = loop_;
    loop_ = nullptr;
  }

  if (wait_for_threadsafe_async_close && loop != nullptr) {
    for (size_t guard = 0; guard < 256; ++guard) {
      {
        std::lock_guard<std::mutex> lock(mutex_);
        if (threadsafe_immediate_async_closed_) break;
      }
      (void)uv_run(loop, UV_RUN_NOWAIT);
    }
  }

  return loop;
}

void Environment::DestroyReleasedEventLoop(uv_loop_t* loop) {
  if (loop != nullptr && DrainAndCloseEnvLoop(loop)) {
    delete loop;
  }
}

bool Environment::EnsureThreadsafeImmediateHandleLocked() {
  if (threadsafe_immediate_async_initialized_) return true;
  if (loop_ == nullptr) return false;
  threadsafe_immediate_async_.data = this;
  threadsafe_immediate_async_closed_ = false;
  if (uv_async_init(loop_, &threadsafe_immediate_async_, OnThreadsafeImmediate) != 0) {
    threadsafe_immediate_async_closed_ = true;
    return false;
  }
  uv_unref(reinterpret_cast<uv_handle_t*>(&threadsafe_immediate_async_));
  threadsafe_immediate_async_initialized_ = true;
  return true;
}

bool Environment::EnsureTimerHandleLocked() {
  if (timer_handle_initialized_) return true;
  if (loop_ == nullptr) return false;
  if (uv_timer_init(loop_, &timer_handle_) != 0) return false;
  timer_handle_.data = this;
  uv_unref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
  timer_handle_initialized_ = true;
  return true;
}

bool Environment::EnsureImmediateCheckHandleLocked() {
  if (immediate_check_handle_initialized_) return true;
  if (loop_ == nullptr) return false;
  if (uv_check_init(loop_, &immediate_check_handle_) != 0) return false;
  immediate_check_handle_.data = this;
  uv_unref(reinterpret_cast<uv_handle_t*>(&immediate_check_handle_));
  if (uv_check_start(&immediate_check_handle_, OnImmediateCheck) != 0) {
    uv_close(reinterpret_cast<uv_handle_t*>(&immediate_check_handle_), nullptr);
    return false;
  }
  immediate_check_handle_initialized_ = true;
  immediate_check_handle_running_ = true;
  return true;
}

bool Environment::EnsureImmediateIdleHandleLocked() {
  if (immediate_idle_handle_initialized_) return true;
  if (loop_ == nullptr) return false;
  if (uv_idle_init(loop_, &immediate_idle_handle_) != 0) return false;
  immediate_idle_handle_.data = this;
  immediate_idle_handle_initialized_ = true;
  return true;
}

void Environment::CloseThreadsafeImmediateHandleLocked() {
  if (!threadsafe_immediate_async_initialized_) return;
  threadsafe_immediate_async_initialized_ = false;
  if (uv_is_closing(reinterpret_cast<uv_handle_t*>(&threadsafe_immediate_async_)) == 0) {
    threadsafe_immediate_async_closed_ = false;
    uv_close(reinterpret_cast<uv_handle_t*>(&threadsafe_immediate_async_),
             OnThreadsafeImmediateClosed);
  }
}

void Environment::ClosePerEnvHandlesLocked() {
  CloseThreadsafeImmediateHandleLocked();

  if (timer_handle_initialized_) {
    timer_handle_initialized_ = false;
    if (uv_is_closing(reinterpret_cast<uv_handle_t*>(&timer_handle_)) == 0) {
      uv_timer_stop(&timer_handle_);
      uv_close(reinterpret_cast<uv_handle_t*>(&timer_handle_), nullptr);
    }
  }

  if (immediate_check_handle_initialized_) {
    immediate_check_handle_initialized_ = false;
    immediate_check_handle_running_ = false;
    if (uv_is_closing(reinterpret_cast<uv_handle_t*>(&immediate_check_handle_)) == 0) {
      uv_check_stop(&immediate_check_handle_);
      uv_close(reinterpret_cast<uv_handle_t*>(&immediate_check_handle_), nullptr);
    }
  }

  if (immediate_idle_handle_initialized_) {
    immediate_idle_handle_initialized_ = false;
    immediate_idle_handle_running_ = false;
    if (uv_is_closing(reinterpret_cast<uv_handle_t*>(&immediate_idle_handle_)) == 0) {
      uv_idle_stop(&immediate_idle_handle_);
      uv_close(reinterpret_cast<uv_handle_t*>(&immediate_idle_handle_), nullptr);
    }
  }
}

void Environment::ScheduleTimerFromExpiry(double next_expiry, double now_ms) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (cleanup_started_ || !timer_handle_initialized_) return;

  if (next_expiry == 0 || !std::isfinite(next_expiry)) {
    uv_timer_stop(&timer_handle_);
    uv_unref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
    return;
  }

  const bool ref = next_expiry > 0;
  const double absolute_expiry = std::abs(next_expiry);
  const double delta = absolute_expiry - now_ms;
  const uint64_t timeout = static_cast<uint64_t>(delta > 1 ? delta : 1);
  uv_timer_start(&timer_handle_, OnTimer, timeout, 0);
  if (ref) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
  } else {
    uv_unref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
  }
}

void Environment::CloseAndDestroyEventLoop() {
  DestroyReleasedEventLoop(ReleaseEventLoop());
}

napi_status Environment::InitializeTimers() {
  if (EnsureEventLoop(nullptr) != napi_ok) return napi_generic_failure;
  std::lock_guard<std::mutex> lock(mutex_);
  if (!EnsureTimerHandleLocked()) return napi_generic_failure;
  if (!EnsureImmediateCheckHandleLocked()) return napi_generic_failure;
  if (!EnsureImmediateIdleHandleLocked()) return napi_generic_failure;
  return napi_ok;
}

double Environment::GetNowMs() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (loop_ == nullptr) return 0;
  uv_update_time(loop_);
  const double now = static_cast<double>(uv_now(loop_));
  if (timer_base_ms_ < 0) {
    timer_base_ms_ = now;
  }
  const double relative = now - timer_base_ms_;
  return relative >= 0 ? relative : 0;
}

void Environment::ScheduleTimer(int64_t duration_ms) {
  if (duration_ms < 1) duration_ms = 1;
  if (EnsureEventLoop(nullptr) != napi_ok) return;
  std::lock_guard<std::mutex> lock(mutex_);
  if (cleanup_started_ || !EnsureTimerHandleLocked()) return;
  uv_timer_start(&timer_handle_, OnTimer, static_cast<uint64_t>(duration_ms), 0);
}

void Environment::ToggleTimerRef(bool ref) {
  std::lock_guard<std::mutex> lock(mutex_);
  if (cleanup_started_ || !timer_handle_initialized_) return;
  if (ref) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
  } else {
    uv_unref(reinterpret_cast<uv_handle_t*>(&timer_handle_));
  }
}

void Environment::EnsureImmediatePump() {
  if (EnsureEventLoop(nullptr) != napi_ok) return;
  std::lock_guard<std::mutex> lock(mutex_);
  (void)EnsureImmediateCheckHandleLocked();
}

void Environment::ToggleImmediateRef(bool ref) {
  if (EnsureEventLoop(nullptr) != napi_ok) return;
  const bool has_refed_native_immediates = EdgeRuntimePlatformHasRefedImmediateTasks(env_);
  std::lock_guard<std::mutex> lock(mutex_);
  if (cleanup_started_ ||
      !EnsureImmediateCheckHandleLocked() ||
      !EnsureImmediateIdleHandleLocked()) {
    return;
  }

  const bool should_ref =
      ref ||
      (immediate_info_.fields != nullptr && immediate_info_.fields[kImmediateRefCount] > 0) ||
      has_refed_native_immediates;

  if (should_ref) {
    if (!immediate_idle_handle_running_ &&
        uv_idle_start(&immediate_idle_handle_, [](uv_idle_t* /*handle*/) {}) == 0) {
      immediate_idle_handle_running_ = true;
    }
  } else if (immediate_idle_handle_running_) {
    uv_idle_stop(&immediate_idle_handle_);
    immediate_idle_handle_running_ = false;
  }
}

int32_t Environment::active_timeout_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  if (timeout_info_.fields == nullptr) return 0;
  const int32_t count = timeout_info_.fields[0];
  return count > 0 ? count : 0;
}

uint32_t Environment::immediate_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  if (immediate_info_.fields == nullptr) return 0;
  const int32_t count = immediate_info_.fields[kImmediateCount];
  return count > 0 ? static_cast<uint32_t>(count) : 0;
}

uint32_t Environment::immediate_ref_count() const {
  std::lock_guard<std::mutex> lock(mutex_);
  if (immediate_info_.fields == nullptr) return 0;
  const int32_t count = immediate_info_.fields[kImmediateRefCount];
  return count > 0 ? static_cast<uint32_t>(count) : 0;
}

bool Environment::immediate_has_outstanding() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return immediate_info_.fields != nullptr &&
         immediate_info_.fields[kImmediateHasOutstanding] != 0;
}

void Environment::AddCleanupHook(CleanupHookCallback callback, void* arg) {
  if (callback == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  cleanup_hooks_.push_back(CleanupHookEntry{callback, arg});
}

void Environment::RemoveCleanupHook(CleanupHookCallback callback, void* arg) {
  std::lock_guard<std::mutex> lock(mutex_);
  cleanup_hooks_.erase(
      std::remove_if(
          cleanup_hooks_.begin(),
          cleanup_hooks_.end(),
          [&](const CleanupHookEntry& entry) {
            return entry.callback == callback && entry.arg == arg;
          }),
      cleanup_hooks_.end());
}

void Environment::AddCleanupStage(CleanupStageCallback callback, void* arg, int order) {
  if (callback == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  for (const auto& entry : cleanup_stages_) {
    if (entry.callback == callback && entry.arg == arg) return;
  }
  cleanup_stages_.push_back(CleanupStageEntry{callback, arg, order});
  std::stable_sort(
      cleanup_stages_.begin(),
      cleanup_stages_.end(),
      [](const CleanupStageEntry& a, const CleanupStageEntry& b) {
        return a.order < b.order;
      });
}

void Environment::RemoveCleanupStage(CleanupStageCallback callback, void* arg) {
  std::lock_guard<std::mutex> lock(mutex_);
  cleanup_stages_.erase(
      std::remove_if(
          cleanup_stages_.begin(),
          cleanup_stages_.end(),
          [&](const CleanupStageEntry& entry) {
            return entry.callback == callback && entry.arg == arg;
          }),
      cleanup_stages_.end());
}

void Environment::AtExit(AtExitCallback callback, void* arg) {
  if (callback == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  at_exit_callbacks_.push_front(AtExitEntry{callback, arg});
}

void Environment::RunAtExitCallbacks() {
  std::deque<AtExitEntry> callbacks;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (at_exit_ran_) return;
    at_exit_ran_ = true;
    callbacks.swap(at_exit_callbacks_);
  }
  for (const auto& entry : callbacks) {
    if (entry.callback != nullptr) entry.callback(entry.arg);
  }
}

bool Environment::cleanup_started() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return cleanup_started_;
}

bool Environment::can_call_into_js() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return can_call_into_js_ && !stopping_;
}

void Environment::set_can_call_into_js(bool can_call_into_js) {
  std::lock_guard<std::mutex> lock(mutex_);
  can_call_into_js_ = can_call_into_js;
}

bool Environment::is_stopping() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return stopping_;
}

void Environment::set_stopping(bool stopping) {
  std::lock_guard<std::mutex> lock(mutex_);
  stopping_ = stopping;
}

bool Environment::filehandle_close_warning() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return emit_filehandle_warning_;
}

void Environment::set_filehandle_close_warning(bool on) {
  std::lock_guard<std::mutex> lock(mutex_);
  emit_filehandle_warning_ = on;
}

void Environment::QueueFileHandleGcWarning(int fd, int close_status) {
  if (env_ == nullptr || fd < 0) return;
  const bool should_schedule =
      SetImmediateThreadsafe(
          [](napi_env env, void* data) {
            auto* payload = static_cast<std::pair<Environment*, std::pair<int, int>>*>(data);
            if (payload == nullptr) return;
            Environment* environment = payload->first;
            const int closed_fd = payload->second.first;
            const int status = payload->second.second;
            delete payload;
            if (environment == nullptr) return;
            if (status < 0) {
              environment->EmitProcessWarning(
                  "Closing file descriptor " + std::to_string(closed_fd) +
                  " on garbage collection failed");
              return;
            }
            environment->EmitProcessWarning(
                "Closing file descriptor " + std::to_string(closed_fd) + " on garbage collection");
            bool emit_deprecation = false;
            {
              std::lock_guard<std::mutex> lock(environment->mutex_);
              emit_deprecation = environment->emit_filehandle_warning_;
              environment->emit_filehandle_warning_ = false;
            }
            if (emit_deprecation) {
              environment->EmitProcessWarning(
                  "Closing a FileHandle object on garbage collection is deprecated. "
                  "Please close FileHandle objects explicitly using "
                  "FileHandle.prototype.close(). In the future, an error will be "
                  "thrown if a file descriptor is closed during garbage collection.",
                  "DeprecationWarning",
                  "DEP0137");
            }
          },
          new std::pair<Environment*, std::pair<int, int>>(this, {fd, close_status}),
          false) == napi_ok;
  if (!should_schedule) {
    EmitProcessWarning("Closing file descriptor " + std::to_string(fd) + " on garbage collection");
    bool emit_deprecation = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      emit_deprecation = emit_filehandle_warning_;
      emit_filehandle_warning_ = false;
    }
    if (emit_deprecation) {
      EmitProcessWarning(
          "Closing a FileHandle object on garbage collection is deprecated. "
          "Please close FileHandle objects explicitly using "
          "FileHandle.prototype.close(). In the future, an error will be "
          "thrown if a file descriptor is closed during garbage collection.",
          "DeprecationWarning",
          "DEP0137");
    }
  }
}

void Environment::EmitProcessWarning(const std::string& message,
                                     const char* type,
                                     const char* code) const {
  if (env_ == nullptr || message.empty() || !can_call_into_js()) return;
  napi_value global = nullptr;
  napi_value process = nullptr;
  napi_value emit_warning = nullptr;
  napi_value message_value = nullptr;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  size_t argc = 1;
  if (napi_get_global(env_, &global) != napi_ok ||
      global == nullptr ||
      napi_get_named_property(env_, global, "process", &process) != napi_ok ||
      process == nullptr ||
      napi_get_named_property(env_, process, "emitWarning", &emit_warning) != napi_ok ||
      emit_warning == nullptr ||
      napi_create_string_utf8(env_, message.c_str(), NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      message_value == nullptr) {
    return;
  }
  argv[0] = message_value;
  if (type != nullptr) {
    if (napi_create_string_utf8(env_, type, NAPI_AUTO_LENGTH, &argv[1]) != napi_ok || argv[1] == nullptr) {
      return;
    }
    argc = 2;
  }
  if (code != nullptr) {
    if (argv[1] == nullptr &&
        (napi_create_string_utf8(env_, "Warning", NAPI_AUTO_LENGTH, &argv[1]) != napi_ok ||
         argv[1] == nullptr)) {
      return;
    }
    if (napi_create_string_utf8(env_, code, NAPI_AUTO_LENGTH, &argv[2]) != napi_ok || argv[2] == nullptr) {
      return;
    }
    argc = 3;
  }
  napi_value ignored = nullptr;
  (void)napi_call_function(env_, process, emit_warning, argc, argv, &ignored);
}

void Environment::AddUnmanagedFd(int fd) {
  if (fd < 0) return;
  std::string warning;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if ((flags_ & EnvironmentFlags::kTrackUnmanagedFds) == 0) return;
    auto [_, inserted] = unmanaged_fds_.emplace(fd);
    if (!inserted) {
      warning = "File descriptor " + std::to_string(fd) + " opened in unmanaged mode twice";
    }
  }
  if (!warning.empty()) EmitProcessWarning(warning);
}

void Environment::RemoveUnmanagedFd(int fd) {
  if (fd < 0) return;
  std::string warning;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if ((flags_ & EnvironmentFlags::kTrackUnmanagedFds) == 0) return;
    if (unmanaged_fds_.erase(fd) == 0) {
      warning = "File descriptor " + std::to_string(fd) + " closed but not opened in unmanaged mode";
    }
  }
  if (!warning.empty()) EmitProcessWarning(warning);
}

void Environment::CloseTrackedUnmanagedFds() {
  std::unordered_set<int> unmanaged_fds;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    unmanaged_fds.swap(unmanaged_fds_);
  }
  for (const int fd : unmanaged_fds) {
    if (fd < 0) continue;
    uv_fs_t req{};
    (void)uv_fs_close(nullptr, &req, fd, nullptr);
    uv_fs_req_cleanup(&req);
  }
}

void* Environment::RegisterActiveHandle(napi_value keepalive_owner,
                                        const char* resource_name,
                                        EdgeEnvironmentHandleHasRef has_ref,
                                        EdgeEnvironmentHandleGetOwner get_owner,
                                        void* data,
                                        EdgeEnvironmentHandleClose close_callback) {
  if (env_ == nullptr || keepalive_owner == nullptr || resource_name == nullptr || has_ref == nullptr) {
    return nullptr;
  }
  auto* entry = new (std::nothrow) ActiveHandleEntry();
  if (entry == nullptr) return nullptr;
  entry->resource_name = resource_name;
  entry->has_ref = has_ref;
  entry->get_owner = get_owner;
  entry->close_callback = close_callback;
  entry->data = data;
  if (napi_create_reference(env_, keepalive_owner, 1, &entry->keepalive_ref) != napi_ok ||
      entry->keepalive_ref == nullptr) {
    delete entry;
    return nullptr;
  }
  std::lock_guard<std::mutex> lock(mutex_);
  active_handles_.push_back(entry);
  return entry;
}

void Environment::UnregisterActiveHandle(void* token) {
  auto* entry = static_cast<ActiveHandleEntry*>(token);
  if (entry == nullptr) return;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = std::find(active_handles_.begin(), active_handles_.end(), entry);
    if (it == active_handles_.end()) return;
    active_handles_.erase(it);
  }
  DeleteRefIfPresent(&entry->keepalive_ref);
  delete entry;
}

void* Environment::RegisterActiveRequest(napi_value owner,
                                         const char* resource_name,
                                         void* data,
                                         EdgeEnvironmentRequestCancel cancel,
                                         EdgeEnvironmentRequestGetOwner get_owner) {
  if (env_ == nullptr || owner == nullptr || resource_name == nullptr) return nullptr;
  auto* entry = new (std::nothrow) ActiveRequestEntry();
  if (entry == nullptr) return nullptr;
  entry->resource_name = resource_name;
  entry->cancel = cancel;
  entry->get_owner = get_owner;
  entry->data = data;
  if (napi_create_reference(env_, owner, 1, &entry->owner_ref) != napi_ok || entry->owner_ref == nullptr) {
    delete entry;
    return nullptr;
  }
  std::lock_guard<std::mutex> lock(mutex_);
  active_requests_.push_back(entry);
  return entry;
}

void Environment::UnregisterActiveRequest(void* token) {
  auto* entry = static_cast<ActiveRequestEntry*>(token);
  if (entry == nullptr) return;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = std::find(active_requests_.begin(), active_requests_.end(), entry);
    if (it == active_requests_.end()) return;
    active_requests_.erase(it);
  }
  DeleteRefIfPresent(&entry->owner_ref);
  delete entry;
}

void Environment::UnregisterActiveRequestByOwner(napi_value owner) {
  if (owner == nullptr) return;
  ActiveRequestEntry* match = nullptr;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    for (ActiveRequestEntry* entry : active_requests_) {
      if (entry == nullptr) continue;
      napi_value current = GetRefValue(env_, entry->owner_ref);
      if (current == nullptr) continue;
      bool same = false;
      if (napi_strict_equals(env_, current, owner, &same) != napi_ok || !same) continue;
      match = entry;
      break;
    }
  }
  if (match != nullptr) {
    UnregisterActiveRequest(match);
  }
}

void Environment::CancelActiveRequests() {
  std::vector<ActiveRequestEntry*> requests;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    requests = active_requests_;
  }
  for (ActiveRequestEntry* entry : requests) {
    if (entry != nullptr && entry->cancel != nullptr) {
      entry->cancel(entry->data);
    }
  }
}

void Environment::CloseActiveHandles() {
  std::vector<ActiveHandleEntry*> handles;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    handles = active_handles_;
  }
  for (ActiveHandleEntry* entry : handles) {
    if (entry != nullptr && entry->close_callback != nullptr) {
      entry->close_callback(entry->data);
    }
  }
}

napi_value Environment::GetActiveHandlesArray() {
  napi_value out = CreateArray(env_);
  if (out == nullptr) return nullptr;
  std::vector<ActiveHandleEntry*> handles;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    handles = active_handles_;
  }
  uint32_t index = 0;
  for (ActiveHandleEntry* entry : handles) {
    if (entry == nullptr || entry->has_ref == nullptr || !entry->has_ref(entry->data)) continue;
    napi_value owner =
        entry->get_owner != nullptr ? entry->get_owner(env_, entry->data) : GetRefValue(env_, entry->keepalive_ref);
    if (owner == nullptr) owner = GetRefValue(env_, entry->keepalive_ref);
    if (owner == nullptr) continue;
    AppendArrayValue(env_, out, &index, owner);
  }
  return out;
}

napi_value Environment::GetActiveRequestsArray() {
  napi_value out = CreateArray(env_);
  if (out == nullptr) return nullptr;
  std::vector<ActiveRequestEntry*> requests;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    requests = active_requests_;
  }
  uint32_t index = 0;
  for (ActiveRequestEntry* entry : requests) {
    if (entry == nullptr) continue;
    napi_value owner =
        entry->get_owner != nullptr ? entry->get_owner(env_, entry->data) : GetRefValue(env_, entry->owner_ref);
    if (owner == nullptr) owner = GetRefValue(env_, entry->owner_ref);
    if (owner == nullptr) continue;
    AppendArrayValue(env_, out, &index, owner);
  }
  return out;
}

napi_value Environment::GetActiveResourcesInfoArray() {
  napi_value out = CreateArray(env_);
  if (out == nullptr) return nullptr;
  std::vector<ActiveRequestEntry*> requests;
  std::vector<ActiveHandleEntry*> handles;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    requests = active_requests_;
    handles = active_handles_;
  }
  uint32_t index = 0;
  for (ActiveRequestEntry* entry : requests) {
    if (entry == nullptr) continue;
    napi_value owner =
        entry->get_owner != nullptr ? entry->get_owner(env_, entry->data) : GetRefValue(env_, entry->owner_ref);
    if (owner == nullptr) owner = GetRefValue(env_, entry->owner_ref);
    if (owner == nullptr) continue;
    AppendStringValue(env_, out, &index, entry->resource_name);
  }
  for (ActiveHandleEntry* entry : handles) {
    if (entry == nullptr || entry->has_ref == nullptr || !entry->has_ref(entry->data)) continue;
    napi_value owner =
        entry->get_owner != nullptr ? entry->get_owner(env_, entry->data) : GetRefValue(env_, entry->keepalive_ref);
    if (owner == nullptr) owner = GetRefValue(env_, entry->keepalive_ref);
    if (owner == nullptr) continue;
    if (IsProcessStdioStream(env_, owner)) continue;
    AppendStringValue(env_, out, &index, entry->resource_name);
  }
  return out;
}

size_t Environment::callback_scope_depth() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return callback_scope_depth_;
}

void Environment::IncrementCallbackScopeDepth() {
  std::lock_guard<std::mutex> lock(mutex_);
  callback_scope_depth_ += 1;
}

void Environment::DecrementCallbackScopeDepth() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (callback_scope_depth_ > 0) callback_scope_depth_ -= 1;
}

size_t Environment::open_callback_scopes() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return open_callback_scopes_;
}

void Environment::IncrementOpenCallbackScopes() {
  std::lock_guard<std::mutex> lock(mutex_);
  open_callback_scopes_ += 1;
}

void Environment::DecrementOpenCallbackScopes() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (open_callback_scopes_ > 0) open_callback_scopes_ -= 1;
}

size_t Environment::async_callback_scope_depth() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return async_callback_scope_depth_;
}

void Environment::IncrementAsyncCallbackScopeDepth() {
  std::lock_guard<std::mutex> lock(mutex_);
  async_callback_scope_depth_ += 1;
}

void Environment::DecrementAsyncCallbackScopeDepth() {
  std::lock_guard<std::mutex> lock(mutex_);
  if (async_callback_scope_depth_ > 0) async_callback_scope_depth_ -= 1;
}

std::vector<napi_async_cleanup_hook_handle> Environment::async_cleanup_hooks() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return async_cleanup_hooks_;
}

void Environment::AddAsyncCleanupHook(napi_async_cleanup_hook_handle handle) {
  if (handle == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  async_cleanup_hooks_.push_back(handle);
}

bool Environment::RemoveAsyncCleanupHook(napi_async_cleanup_hook_handle handle) {
  if (handle == nullptr) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = std::find(async_cleanup_hooks_.begin(), async_cleanup_hooks_.end(), handle);
  if (it == async_cleanup_hooks_.end()) return false;
  async_cleanup_hooks_.erase(it);
  return true;
}

bool Environment::async_cleanup_hook_registered() const {
  std::lock_guard<std::mutex> lock(mutex_);
  return async_cleanup_hook_registered_;
}

void Environment::set_async_cleanup_hook_registered(bool registered) {
  std::lock_guard<std::mutex> lock(mutex_);
  async_cleanup_hook_registered_ = registered;
}

void Environment::RunAsyncCleanupHooks() {
  std::vector<napi_async_cleanup_hook_handle> pending;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    pending.reserve(async_cleanup_hooks_.size());
    for (auto* handle : async_cleanup_hooks_) {
      if (handle != nullptr && !handle->removed) {
        pending.push_back(handle);
      }
    }
  }

  for (auto* handle : pending) {
    if (handle != nullptr && !handle->removed && handle->hook != nullptr) {
      handle->hook(handle, handle->arg);
    }
  }

  size_t guard = 0;
  uv_loop_t* loop = GetExistingEventLoop();
  while (guard++ < 128) {
    bool any_left = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      for (auto* handle : async_cleanup_hooks_) {
        if (handle != nullptr && !handle->removed) {
          any_left = true;
          break;
        }
      }
    }
    if (!any_left) break;
    if (loop == nullptr) break;
    (void)uv_run(loop, UV_RUN_DEFAULT);
  }

  std::vector<napi_async_cleanup_hook_handle> remaining;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    remaining.swap(async_cleanup_hooks_);
    async_cleanup_hook_registered_ = false;
  }
  for (auto* handle : remaining) {
    delete handle;
  }
}

napi_status Environment::SetImmediateThreadsafe(ThreadsafeImmediateCallback callback,
                                                void* data,
                                                bool refed) {
  if (callback == nullptr) return napi_invalid_arg;
  if (EnsureEventLoop(nullptr) != napi_ok) return napi_generic_failure;
  int send_rc = UV_EINVAL;

  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (cleanup_started_) return napi_generic_failure;
    if (!EnsureThreadsafeImmediateHandleLocked()) return napi_generic_failure;
    threadsafe_immediates_.push_back(ThreadsafeImmediateEntry{callback, data, refed});
    send_rc = uv_async_send(&threadsafe_immediate_async_);
    if (send_rc != 0) {
      threadsafe_immediates_.pop_back();
    }
  }

  return send_rc == 0 ? napi_ok : napi_generic_failure;
}

napi_status Environment::RequestInterrupt(InterruptCallback callback, void* data) {
  if (callback == nullptr) return napi_invalid_arg;
  if (EnsureEventLoop(nullptr) != napi_ok) return napi_generic_failure;
  int send_rc = UV_EINVAL;

  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (cleanup_started_) return napi_generic_failure;
    if (!EnsureThreadsafeImmediateHandleLocked()) return napi_generic_failure;
    interrupts_.push_back(ThreadsafeImmediateEntry{callback, data, true});
    send_rc = uv_async_send(&threadsafe_immediate_async_);
    if (send_rc != 0) {
      interrupts_.pop_back();
    }
  }

  if (send_rc != 0) {
    return napi_generic_failure;
  }

  return unofficial_napi_request_interrupt(env_, OnInterruptFromV8, this);
}

size_t Environment::DrainInterrupts() {
  std::deque<ThreadsafeImmediateEntry> tasks;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    tasks.swap(interrupts_);
  }

  for (const auto& task : tasks) {
    if (task.callback != nullptr) {
      if (env_ == nullptr) continue;
      edge::HandleScope scope(env_);
      if (!scope.is_open()) continue;
      task.callback(env_, task.data);
    }
  }
  return tasks.size();
}

size_t Environment::DrainThreadsafeImmediates() {
  std::deque<ThreadsafeImmediateEntry> tasks;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    tasks.swap(threadsafe_immediates_);
  }

  for (const auto& task : tasks) {
    if (task.callback != nullptr) {
      if (env_ == nullptr) continue;
      edge::HandleScope scope(env_);
      if (!scope.is_open()) continue;
      task.callback(env_, task.data);
    }
  }
  return tasks.size();
}

void Environment::OnThreadsafeImmediate(uv_async_t* handle) {
  auto* env = static_cast<Environment*>(handle != nullptr ? handle->data : nullptr);
  if (env == nullptr) return;
  (void)env->DrainInterrupts();
  (void)env->DrainThreadsafeImmediates();
}

void Environment::OnThreadsafeImmediateClosed(uv_handle_t* handle) {
  auto* env = static_cast<Environment*>(handle != nullptr ? handle->data : nullptr);
  if (env == nullptr) return;
  std::lock_guard<std::mutex> lock(env->mutex_);
  env->threadsafe_immediate_async_closed_ = true;
}

void Environment::OnTimer(uv_timer_t* handle) {
  auto* env = static_cast<Environment*>(handle != nullptr ? handle->data : nullptr);
  if (env == nullptr || !env->can_call_into_js()) return;

  const double now_ms = env->GetNowMs();
  const double next_expiry = EdgeTimersHostCallTimersCallback(env->env(), now_ms);
  env->ScheduleTimerFromExpiry(next_expiry, now_ms);
  if (!EdgeTimersHostRunCallbackCheckpoint(env->env())) {
    StopLoopOnJsError(env);
  }
}

void Environment::OnImmediateCheck(uv_check_t* handle) {
  auto* env = static_cast<Environment*>(handle != nullptr ? handle->data : nullptr);
  if (env == nullptr) return;

  if (env->immediate_count() == 0 && !EdgeRuntimePlatformHasImmediateTasks(env->env())) {
    if (env->immediate_ref_count() == 0 &&
        !EdgeRuntimePlatformHasRefedImmediateTasks(env->env())) {
      env->ToggleImmediateRef(false);
    }
    return;
  }

  (void)EdgeRuntimePlatformDrainImmediateTasks(env->env());
  if (!env->can_call_into_js()) return;

  bool pending_exception = false;
  if (napi_is_exception_pending(env->env(), &pending_exception) == napi_ok &&
      pending_exception) {
    StopLoopOnJsError(env);
    return;
  }

  if (env->immediate_count() != 0 && !EdgeTimersHostCallImmediateCallback(env->env())) {
    StopLoopOnJsError(env);
    return;
  }

  if (env->immediate_ref_count() == 0 &&
      !EdgeRuntimePlatformHasRefedImmediateTasks(env->env())) {
    env->ToggleImmediateRef(false);
  }
}

void Environment::OnInterruptFromV8(napi_env env, void* data) {
  auto* environment = static_cast<Environment*>(data);
  if (environment == nullptr || environment->env_ != env) return;
  (void)environment->DrainInterrupts();
}

TickInfo* Environment::tick_info() {
  return &tick_info_;
}

ImmediateInfo* Environment::immediate_info() {
  return &immediate_info_;
}

TimeoutInfo* Environment::timeout_info() {
  return &timeout_info_;
}

StreamBaseState* Environment::stream_base_state() {
  return &stream_base_state_;
}

PrincipalRealmShim* Environment::principal_realm() {
  return &principal_realm_;
}

void Environment::AssignToContext(void* token) {
  if (token == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  attached_contexts_.emplace(token);
}

void Environment::UnassignFromContext(void* token) {
  if (token == nullptr) return;
  std::lock_guard<std::mutex> lock(mutex_);
  attached_contexts_.erase(token);
}

bool Environment::HasAttachedContext(void* token) const {
  if (token == nullptr) return false;
  std::lock_guard<std::mutex> lock(mutex_);
  return attached_contexts_.find(token) != attached_contexts_.end();
}

SlotEntry Environment::GetSlot(size_t slot_id) const {
  std::lock_guard<std::mutex> lock(mutex_);
  auto it = slots_.find(slot_id);
  return it == slots_.end() ? SlotEntry{} : it->second;
}

void Environment::SetSlot(size_t slot_id, void* data, void (*deleter)(void* data)) {
  SlotEntry old;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto& slot = slots_[slot_id];
    old = slot;
    slot.data = data;
    slot.deleter = deleter;
  }
  if (old.deleter != nullptr && old.data != nullptr && old.data != data) {
    old.deleter(old.data);
  }
}

void Environment::ClearSlot(size_t slot_id) {
  SlotEntry old;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = slots_.find(slot_id);
    if (it == slots_.end()) return;
    old = it->second;
    slots_.erase(it);
  }
  if (old.deleter != nullptr && old.data != nullptr) {
    old.deleter(old.data);
  }
}

void Environment::ResetTrackedRefs() {
  std::lock_guard<std::mutex> lock(mutex_);
  DeleteRefIfPresent(&binding_ref_);
  DeleteRefIfPresent(&env_message_port_ref_);
  DeleteRefIfPresent(&tick_info_.ref);
  tick_info_.fields = nullptr;
  DeleteRefIfPresent(&immediate_info_.ref);
  immediate_info_.fields = nullptr;
  DeleteRefIfPresent(&timeout_info_.ref);
  timeout_info_.fields = nullptr;
  DeleteRefIfPresent(&stream_base_state_.ref);
  stream_base_state_.fields = nullptr;
}

void Environment::CleanupActiveRegistryEntries() {
  std::vector<ActiveHandleEntry*> handles;
  std::vector<ActiveRequestEntry*> requests;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    handles.swap(active_handles_);
    requests.swap(active_requests_);
  }
  for (ActiveHandleEntry* entry : handles) {
    if (entry == nullptr) continue;
    DeleteRefIfPresent(&entry->keepalive_ref);
    delete entry;
  }
  for (ActiveRequestEntry* entry : requests) {
    if (entry == nullptr) continue;
    DeleteRefIfPresent(&entry->owner_ref);
    delete entry;
  }
}

void Environment::RunCleanup(bool close_event_loop) {
  std::vector<CleanupStageEntry> stages;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    if (cleanup_started_) return;
    cleanup_started_ = true;
    stop_requested_ = true;
    stopping_ = true;
    can_call_into_js_ = false;
    stages = cleanup_stages_;
  }

  for (const auto& stage : stages) {
    if (stage.callback != nullptr) {
      stage.callback(env_, stage.arg);
    }
  }

  {
    std::lock_guard<std::mutex> lock(mutex_);
    ClosePerEnvHandlesLocked();
  }

  for (size_t guard = 0; guard < 256; ++guard) {
    bool did_work = false;

    {
      std::lock_guard<std::mutex> lock(mutex_);
      did_work = !active_requests_.empty() || !active_handles_.empty();
    }
    CancelActiveRequests();
    CloseActiveHandles();

    if (DrainInterrupts() > 0) {
      did_work = true;
    }

    if (DrainThreadsafeImmediates() > 0) {
      did_work = true;
    }

    std::vector<CleanupHookEntry> cleanup_hooks;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      if (!cleanup_hooks_.empty()) {
        cleanup_hooks.swap(cleanup_hooks_);
      }
    }
    if (!cleanup_hooks.empty()) {
      did_work = true;
      for (auto it = cleanup_hooks.rbegin(); it != cleanup_hooks.rend(); ++it) {
        if (it->callback != nullptr) it->callback(it->arg);
      }
    }

    bool has_async_cleanup = false;
    {
      std::lock_guard<std::mutex> lock(mutex_);
      for (auto* handle : async_cleanup_hooks_) {
        if (handle != nullptr && !handle->removed) {
          has_async_cleanup = true;
          break;
        }
      }
    }
    if (has_async_cleanup) {
      did_work = true;
      RunAsyncCleanupHooks();
    }

    if (uv_loop_t* loop = GetExistingEventLoop(); loop != nullptr) {
      if (uv_run(loop, UV_RUN_NOWAIT) != 0) {
        did_work = true;
      } else {
        bool has_pending_registry_entries = false;
        {
          std::lock_guard<std::mutex> lock(mutex_);
          has_pending_registry_entries = !active_requests_.empty() || !active_handles_.empty();
        }
        if (has_pending_registry_entries) {
          (void)uv_run(loop, UV_RUN_ONCE);
          did_work = true;
        }
      }
    }

    if (!did_work) break;
  }

  ResetTrackedRefs();
  CloseTrackedUnmanagedFds();
  if (close_event_loop) {
    CloseAndDestroyEventLoop();
  }
  CleanupActiveRegistryEntries();

  std::unordered_map<size_t, SlotEntry> slots;
  {
    std::lock_guard<std::mutex> lock(mutex_);
    slots.swap(slots_);
  }
  RunSlotDeleters(&slots);
}

}  // namespace edge

edge::Environment* EdgeEnvironmentGet(napi_env env) {
  return edge::Environment::Get(env);
}

bool EdgeEnvironmentAttach(napi_env env, const EdgeEnvironmentConfig* config) {
  EdgeEnvironmentConfig resolved;
  if (config != nullptr) resolved = *config;
  return edge::Environment::Attach(env, resolved) != nullptr;
}

void EdgeEnvironmentDetach(napi_env env) {
  edge::Environment::Detach(env);
}

bool EdgeEnvironmentGetConfig(napi_env env, EdgeEnvironmentConfig* out) {
  if (out == nullptr) return false;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    *out = environment->config();
    return true;
  }
  *out = EdgeEnvironmentConfig();
  return false;
}

uv_loop_t* EdgeEnvironmentReleaseEventLoop(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->ReleaseEventLoop();
  }
  return nullptr;
}

void EdgeEnvironmentDestroyReleasedEventLoop(uv_loop_t* loop) {
  edge::Environment::DestroyReleasedEventLoop(loop);
}

void EdgeEnvironmentRunCleanup(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RunCleanup();
  }
}

void EdgeEnvironmentRunCleanupPreserveLoop(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RunCleanup(false);
  }
}

void EdgeEnvironmentRunAtExitCallbacks(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RunAtExitCallbacks();
  }
}

bool EdgeEnvironmentCleanupStarted(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->cleanup_started();
  }
  return false;
}

bool EdgeEnvironmentCanCallIntoJs(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->can_call_into_js();
  }
  return false;
}

edge::SlotEntry EdgeEnvironmentGetOpaqueSlot(napi_env env, size_t slot_id) {
  if (env == nullptr) return edge::SlotEntry{};
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->GetSlot(slot_id);
  }

  std::lock_guard<std::mutex> lock(g_environment_mu);
  auto env_it = g_detached_slots.find(env);
  if (env_it == g_detached_slots.end()) return edge::SlotEntry{};
  auto slot_it = env_it->second.slots.find(slot_id);
  return slot_it == env_it->second.slots.end() ? edge::SlotEntry{} : slot_it->second;
}

void EdgeEnvironmentSetOpaqueSlot(napi_env env,
                                  size_t slot_id,
                                  void* data,
                                  void (*deleter)(void* data)) {
  if (env == nullptr) return;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->SetSlot(slot_id, data, deleter);
    return;
  }

  edge::SlotEntry old;
  {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto& state = g_detached_slots[env];
    if (!state.cleanup_hook_registered &&
        napi_add_env_cleanup_hook(env, OnDetachedEnvSlotsCleanup, env) == napi_ok) {
      state.cleanup_hook_registered = true;
    }
    auto& slot = state.slots[slot_id];
    old = slot;
    slot.data = data;
    slot.deleter = deleter;
  }

  if (old.deleter != nullptr && old.data != nullptr && old.data != data) {
    old.deleter(old.data);
  }
}

void EdgeEnvironmentClearOpaqueSlot(napi_env env, size_t slot_id) {
  if (env == nullptr) return;
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->ClearSlot(slot_id);
    return;
  }

  edge::SlotEntry old;
  {
    std::lock_guard<std::mutex> lock(g_environment_mu);
    auto env_it = g_detached_slots.find(env);
    if (env_it == g_detached_slots.end()) return;
    auto slot_it = env_it->second.slots.find(slot_id);
    if (slot_it == env_it->second.slots.end()) return;
    old = slot_it->second;
    env_it->second.slots.erase(slot_it);
  }
  if (old.deleter != nullptr && old.data != nullptr) {
    old.deleter(old.data);
  }
}

void EdgeEnvironmentRegisterCleanupStage(napi_env env,
                                         edge::Environment::CleanupStageCallback callback,
                                         void* arg,
                                         int order) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->AddCleanupStage(callback, arg, order);
  }
}

void EdgeEnvironmentUnregisterCleanupStage(napi_env env,
                                           edge::Environment::CleanupStageCallback callback,
                                           void* arg) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->RemoveCleanupStage(callback, arg);
  }
}
