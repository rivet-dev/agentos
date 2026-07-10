#include "internal_binding/binding_initializers.h"
#include "internal_binding/watchdog.h"

#include <algorithm>
#include <atomic>
#include <cstdio>
#include <mutex>
#include <string>
#include <vector>

#include <uv.h>

#if !defined(_WIN32)
#include <pthread.h>
#include <signal.h>
#endif

#include "edge_environment.h"
#include "internal_binding/helpers.h"
#include "unofficial_napi.h"

#if !defined(_WIN32)
namespace node {
void RegisterSignalHandler(int signal,
                           void (*handler)(int signal, siginfo_t* info, void* ucontext),
                           bool reset_handler = false);
void SignalExit(int signal, siginfo_t* info, void* ucontext);
}  // namespace node
#endif

namespace internal_binding {

namespace {

struct TraceSigintWatchdogWrap {
  napi_env env = nullptr;
  std::atomic<bool> started{false};
  std::atomic<bool> interrupt_requested{false};
  std::atomic<bool> interrupting{false};
};

#if !defined(_WIN32)
struct SigintWatchdogState {
  std::mutex action_mutex;
  std::mutex list_mutex;
  std::vector<TraceSigintWatchdogWrap*> watchdogs;
  int start_stop_count = 0;
  bool has_pending_signal = false;
  bool stopping = false;
  bool has_running_thread = false;
  bool sem_initialized = false;
  pthread_t thread{};
  uv_sem_t sem{};
};

SigintWatchdogState g_sigint_watchdog_state;

void EnsureSigintSemaphoreInitialized() {
  if (g_sigint_watchdog_state.sem_initialized) return;
  if (uv_sem_init(&g_sigint_watchdog_state.sem, 0) == 0) {
    g_sigint_watchdog_state.sem_initialized = true;
  }
}

bool GetNamedValue(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  bool has_property = false;
  if (napi_has_named_property(env, obj, key, &has_property) != napi_ok || !has_property) return false;
  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

bool GetStringProperty(napi_env env, napi_value obj, const char* key, std::string* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  if (!GetNamedValue(env, obj, key, &value) || value == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

bool GetUint32Property(napi_env env, napi_value obj, const char* key, uint32_t* out) {
  if (out == nullptr) return false;
  napi_value value = nullptr;
  return GetNamedValue(env, obj, key, &value) &&
         value != nullptr &&
         napi_get_value_uint32(env, value, out) == napi_ok;
}

std::string FormatCallSiteLine(napi_env env, napi_value callsite) {
  std::string function_name;
  std::string script_name;
  uint32_t line = 0;
  uint32_t column = 0;
  (void)GetStringProperty(env, callsite, "functionName", &function_name);
  (void)GetStringProperty(env, callsite, "scriptName", &script_name);
  (void)GetUint32Property(env, callsite, "lineNumber", &line);
  (void)GetUint32Property(env, callsite, "columnNumber", &column);

  if (script_name.empty()) script_name = "<anonymous>";
  const std::string location = script_name + ":" + std::to_string(line) + ":" + std::to_string(column);
  if (function_name.empty()) {
    return "    at " + location;
  }
  return "    at " + function_name + " (" + location + ")";
}

void PrintTraceSigintStack(napi_env env) {
  std::fputs("KEYBOARD_INTERRUPT: Script execution was interrupted by `SIGINT`\n", stderr);

  napi_value callsites = nullptr;
  if (unofficial_napi_get_call_sites(env, 10, &callsites) != napi_ok || callsites == nullptr) {
    std::fflush(stderr);
    return;
  }

  bool is_array = false;
  if (napi_is_array(env, callsites, &is_array) != napi_ok || !is_array) {
    std::fflush(stderr);
    return;
  }

  uint32_t length = 0;
  if (napi_get_array_length(env, callsites, &length) != napi_ok) {
    std::fflush(stderr);
    return;
  }

  for (uint32_t i = 0; i < length; ++i) {
    napi_value callsite = nullptr;
    if (napi_get_element(env, callsites, i, &callsite) != napi_ok || callsite == nullptr) continue;
    const std::string line = FormatCallSiteLine(env, callsite);
    std::fputs(line.c_str(), stderr);
    std::fputc('\n', stderr);
  }
  std::fflush(stderr);
}

void UnregisterWatchdogLocked(TraceSigintWatchdogWrap* wrap) {
  std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
  auto& watchdogs = g_sigint_watchdog_state.watchdogs;
  const auto it = std::find(watchdogs.begin(), watchdogs.end(), wrap);
  if (it != watchdogs.end()) {
    watchdogs.erase(it);
  }
}

void RegisterWatchdogLocked(TraceSigintWatchdogWrap* wrap) {
  std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
  auto& watchdogs = g_sigint_watchdog_state.watchdogs;
  if (std::find(watchdogs.begin(), watchdogs.end(), wrap) == watchdogs.end()) {
    watchdogs.push_back(wrap);
  }
}

void RestoreDefaultSigintHandler() {
  node::RegisterSignalHandler(SIGINT, node::SignalExit, true);
}

void HandleWatchdogInterrupt(napi_env env, void* data);

bool InformWatchdogsAboutSignal() {
  std::vector<TraceSigintWatchdogWrap*> watchdogs;
  bool stopping = false;
  {
    std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
    stopping = g_sigint_watchdog_state.stopping;
    if (g_sigint_watchdog_state.watchdogs.empty() && !stopping) {
      g_sigint_watchdog_state.has_pending_signal = true;
    }
    watchdogs = g_sigint_watchdog_state.watchdogs;
  }

  for (auto it = watchdogs.rbegin(); it != watchdogs.rend(); ++it) {
    TraceSigintWatchdogWrap* wrap = *it;
    if (wrap == nullptr || !wrap->started.load()) continue;
    bool expected = false;
    if (!wrap->interrupt_requested.compare_exchange_strong(expected, true)) continue;
    napi_status status = napi_generic_failure;
    if (auto* environment = EdgeEnvironmentGet(wrap->env); environment != nullptr) {
      status = environment->RequestInterrupt(HandleWatchdogInterrupt, wrap);
    } else {
      status = unofficial_napi_request_interrupt(wrap->env, HandleWatchdogInterrupt, wrap);
    }
    if (status != napi_ok) {
      wrap->interrupt_requested.store(false);
    }
  }

  return stopping;
}

void* RunSigintWatchdogThread(void* /*arg*/) {
  bool stopping = false;
  do {
    uv_sem_wait(&g_sigint_watchdog_state.sem);
    stopping = InformWatchdogsAboutSignal();
  } while (!stopping);
  return nullptr;
}

void HandleSignal(int /*signal*/, siginfo_t* /*info*/, void* /*ucontext*/) {
  if (g_sigint_watchdog_state.sem_initialized) {
    uv_sem_post(&g_sigint_watchdog_state.sem);
  }
}

bool StartSigintWatchdogHelperLocked() {
  if (g_sigint_watchdog_state.start_stop_count++ > 0) return true;

  EnsureSigintSemaphoreInitialized();
  if (!g_sigint_watchdog_state.sem_initialized) {
    g_sigint_watchdog_state.start_stop_count--;
    return false;
  }

  {
    std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
    g_sigint_watchdog_state.has_pending_signal = false;
    g_sigint_watchdog_state.stopping = false;
  }

  if (pthread_create(&g_sigint_watchdog_state.thread, nullptr, RunSigintWatchdogThread, nullptr) != 0) {
    g_sigint_watchdog_state.start_stop_count--;
    return false;
  }

  g_sigint_watchdog_state.has_running_thread = true;
  node::RegisterSignalHandler(SIGINT, HandleSignal, false);
  return true;
}

bool StopSigintWatchdogHelperLocked() {
  if (g_sigint_watchdog_state.start_stop_count <= 0) return false;
  bool had_pending_signal = false;
  if (--g_sigint_watchdog_state.start_stop_count > 0) {
    std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
    had_pending_signal = g_sigint_watchdog_state.has_pending_signal;
    g_sigint_watchdog_state.has_pending_signal = false;
    return had_pending_signal;
  }

  {
    std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
    had_pending_signal = g_sigint_watchdog_state.has_pending_signal;
    g_sigint_watchdog_state.stopping = true;
    g_sigint_watchdog_state.watchdogs.clear();
  }

  if (g_sigint_watchdog_state.has_running_thread) {
    uv_sem_post(&g_sigint_watchdog_state.sem);
    (void)pthread_join(g_sigint_watchdog_state.thread, nullptr);
    g_sigint_watchdog_state.has_running_thread = false;
  }

  g_sigint_watchdog_state.has_pending_signal = false;
  RestoreDefaultSigintHandler();
  return had_pending_signal;
}

void HandleWatchdogInterrupt(napi_env env, void* data) {
  auto* wrap = static_cast<TraceSigintWatchdogWrap*>(data);
  if (wrap == nullptr) return;
  wrap->interrupt_requested.store(false);
  if (!wrap->started.load()) return;
  if (wrap->interrupting.exchange(true)) return;

  PrintTraceSigintStack(env);

  {
    std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
    if (wrap->started.exchange(false)) {
      UnregisterWatchdogLocked(wrap);
      (void)StopSigintWatchdogHelperLocked();
    }
  }

  raise(SIGINT);
}
#endif

void TraceSigintWatchdogFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  auto* wrap = static_cast<TraceSigintWatchdogWrap*>(data);
  if (wrap == nullptr) return;
#if !defined(_WIN32)
  if (wrap->started.exchange(false)) {
    std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
    UnregisterWatchdogLocked(wrap);
    (void)StopSigintWatchdogHelperLocked();
  }
#endif
  delete wrap;
}

napi_value TraceSigintWatchdogCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok || self == nullptr) {
    return Undefined(env);
  }

  auto* wrap = new TraceSigintWatchdogWrap();
  wrap->env = env;
  if (napi_wrap(env, self, wrap, TraceSigintWatchdogFinalize, nullptr, nullptr) != napi_ok) {
    delete wrap;
    return Undefined(env);
  }
  return self;
}

bool GetThis(napi_env env, napi_callback_info info, TraceSigintWatchdogWrap** out) {
  if (out == nullptr) return false;
  *out = nullptr;
  napi_value self = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok || self == nullptr) {
    return false;
  }
  return napi_unwrap(env, self, reinterpret_cast<void**>(out)) == napi_ok && *out != nullptr;
}

napi_value TraceSigintWatchdogStart(napi_env env, napi_callback_info info) {
  TraceSigintWatchdogWrap* wrap = nullptr;
  if (!GetThis(env, info, &wrap) || wrap == nullptr) return Undefined(env);
  if (wrap->started.exchange(true)) return Undefined(env);
  wrap->interrupt_requested.store(false);
  wrap->interrupting.store(false);

#if !defined(_WIN32)
  std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
  RegisterWatchdogLocked(wrap);
  if (!StartSigintWatchdogHelperLocked()) {
    UnregisterWatchdogLocked(wrap);
    wrap->started.store(false);
  }
#endif
  return Undefined(env);
}

napi_value TraceSigintWatchdogStop(napi_env env, napi_callback_info info) {
  TraceSigintWatchdogWrap* wrap = nullptr;
  if (!GetThis(env, info, &wrap) || wrap == nullptr) return Undefined(env);
  if (!wrap->started.exchange(false)) return Undefined(env);
  wrap->interrupt_requested.store(false);
  wrap->interrupting.store(false);

#if !defined(_WIN32)
  std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
  UnregisterWatchdogLocked(wrap);
  (void)StopSigintWatchdogHelperLocked();
#endif
  return Undefined(env);
}

}  // namespace

bool StartSigintWatchdog() {
#if !defined(_WIN32)
  std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
  return StartSigintWatchdogHelperLocked();
#else
  return true;
#endif
}

bool StopSigintWatchdog() {
#if !defined(_WIN32)
  std::lock_guard<std::mutex> action_lock(g_sigint_watchdog_state.action_mutex);
  return StopSigintWatchdogHelperLocked();
#else
  return false;
#endif
}

bool WatchdogHasPendingSigint() {
#if !defined(_WIN32)
  std::lock_guard<std::mutex> lock(g_sigint_watchdog_state.list_mutex);
  return g_sigint_watchdog_state.has_pending_signal;
#else
  return false;
#endif
}

napi_value InitWatchdog(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return Undefined(env);

  napi_property_descriptor methods[] = {
      {"start", nullptr, TraceSigintWatchdogStart, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"stop", nullptr, TraceSigintWatchdogStop, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };

  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "TraceSigintWatchdog",
                        NAPI_AUTO_LENGTH,
                        TraceSigintWatchdogCtor,
                        nullptr,
                        sizeof(methods) / sizeof(methods[0]),
                        methods,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return Undefined(env);
  }

  napi_set_named_property(env, binding, "TraceSigintWatchdog", ctor);
  return binding;
}

}  // namespace internal_binding
