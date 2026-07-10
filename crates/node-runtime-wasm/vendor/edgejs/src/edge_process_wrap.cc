#include "edge_process_wrap.h"

#include <chrono>
#include <csignal>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include <uv.h>

#include "internal_binding/helpers.h"
#include "edge_active_resource.h"
#include "edge_async_wrap.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_handle_wrap.h"
#include "edge_runtime.h"
#include "edge_stream_base.h"

namespace {

struct ProcessWrap {
  EdgeHandleWrap handle_wrap{};
  int32_t pid = 0;
  bool alive = false;
  bool destroy_queued = false;
  int64_t async_id = 0;
  uv_process_t process{};
};

std::mutex g_live_child_pids_mutex;
std::unordered_set<int32_t> g_live_child_pids;

napi_value MakeInt32(napi_env env, int32_t value) {
  napi_value out = nullptr;
  napi_create_int32(env, value, &out);
  return out;
}

napi_value MakeInt64(napi_env env, int64_t value) {
  napi_value out = nullptr;
  napi_create_int64(env, value, &out);
  return out;
}

napi_value MakeBool(napi_env env, bool value) {
  napi_value out = nullptr;
  napi_get_boolean(env, value, &out);
  return out;
}

void SetPidProperty(napi_env env, napi_value self, int32_t pid) {
  napi_value pid_value = MakeInt32(env, pid);
  if (pid_value != nullptr) napi_set_named_property(env, self, "pid", pid_value);
}

void SetPidUndefined(napi_env env, napi_value self) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  if (undefined != nullptr) napi_set_named_property(env, self, "pid", undefined);
}

void TrackLiveChildPid(int32_t pid) {
  if (pid <= 0) return;
  std::lock_guard<std::mutex> lock(g_live_child_pids_mutex);
  g_live_child_pids.insert(pid);
}

void UntrackLiveChildPid(int32_t pid) {
  if (pid <= 0) return;
  std::lock_guard<std::mutex> lock(g_live_child_pids_mutex);
  g_live_child_pids.erase(pid);
}

bool DebugProcessWrapEnabled() {
  const char* value = std::getenv("EDGE_DEBUG_PROCESS_WRAP");
  return value != nullptr && value[0] != '\0' && value[0] != '0';
}

bool ProcessExists(int32_t pid) {
  if (pid <= 0) return false;
  if (kill(pid, 0) == 0) return true;
  return errno == EPERM;
}

void ThrowIllegalInvocation(napi_env env) {
  napi_throw_type_error(env, nullptr, "Illegal invocation");
}

bool UnwrapProcess(napi_env env, napi_value self, ProcessWrap** out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (self == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  ProcessWrap* wrap = nullptr;
  if (napi_unwrap(env, self, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  *out = wrap;
  return true;
}

ProcessWrap* GetThis(napi_env env, napi_callback_info info, napi_value* self_out) {
  size_t argc = 0;
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok) return nullptr;
  ProcessWrap* wrap = nullptr;
  if (!UnwrapProcess(env, self, &wrap)) return nullptr;
  if (self_out != nullptr) *self_out = self;
  return wrap;
}

bool IsTruthyProperty(napi_env env, napi_value obj, const char* key) {
  bool has_property = false;
  if (napi_has_named_property(env, obj, key, &has_property) != napi_ok || !has_property) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, obj, key, &value) != napi_ok || value == nullptr) return false;
  bool out = false;
  if (napi_get_value_bool(env, value, &out) == napi_ok) return out;
  return false;
}

bool GetNamedValue(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  bool has_property = false;
  if (napi_has_named_property(env, obj, key, &has_property) != napi_ok || !has_property) return false;
  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

bool GetInt32Property(napi_env env, napi_value obj, const char* key, int32_t* out) {
  napi_value value = nullptr;
  if (!GetNamedValue(env, obj, key, &value) || value == nullptr) return false;
  return napi_get_value_int32(env, value, out) == napi_ok;
}

bool GetStringProperty(napi_env env, napi_value obj, const char* key, std::string* out) {
  napi_value value = nullptr;
  if (!GetNamedValue(env, obj, key, &value) || value == nullptr) return false;
  napi_valuetype value_type = napi_undefined;
  if (napi_typeof(env, value, &value_type) != napi_ok || value_type == napi_undefined || value_type == napi_null) {
    return false;
  }
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) return false;
  size_t len = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &len) != napi_ok) return false;
  std::string text(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, text.data(), text.size(), &copied) != napi_ok) return false;
  text.resize(copied);
  *out = std::move(text);
  return true;
}

napi_value GetActiveOwner(napi_env env, void* data) {
  auto* wrap = static_cast<ProcessWrap*>(data);
  return wrap != nullptr ? EdgeHandleWrapGetActiveOwner(env, wrap->handle_wrap.wrapper_ref) : nullptr;
}

bool HasRef(void* data) {
  auto* wrap = static_cast<ProcessWrap*>(data);
  return wrap != nullptr &&
         EdgeHandleWrapHasRef(&wrap->handle_wrap, reinterpret_cast<const uv_handle_t*>(&wrap->process));
}

void QueueDestroy(ProcessWrap* wrap) {
  if (wrap == nullptr || wrap->destroy_queued || wrap->async_id <= 0) return;
  wrap->destroy_queued = true;
  EdgeAsyncWrapQueueDestroyId(wrap->handle_wrap.env, wrap->async_id);
}

void DeleteProcessWrap(void* data) {
  auto* wrap = static_cast<ProcessWrap*>(data);
  if (wrap == nullptr) return;
  QueueDestroy(wrap);
  delete wrap;
}

const char* SignalNameFromNumber(int sig) {
  switch (sig) {
    case 0:
      return "";
    case SIGTERM:
      return "SIGTERM";
    case SIGKILL:
      return "SIGKILL";
    case SIGINT:
      return "SIGINT";
    case SIGHUP:
      return "SIGHUP";
    case SIGQUIT:
      return "SIGQUIT";
    case SIGABRT:
      return "SIGABRT";
    case SIGALRM:
      return "SIGALRM";
    case SIGUSR1:
      return "SIGUSR1";
    case SIGUSR2:
      return "SIGUSR2";
    default:
      return "";
  }
}

bool ParseStringArray(napi_env env, napi_value value, std::vector<std::string>* storage, std::vector<char*>* out) {
  if (storage == nullptr || out == nullptr) return false;
  storage->clear();
  out->clear();
  if (value == nullptr) return true;

  bool is_array = false;
  if (napi_is_array(env, value, &is_array) != napi_ok || !is_array) return false;
  uint32_t len = 0;
  if (napi_get_array_length(env, value, &len) != napi_ok) return false;
  storage->reserve(len);
  out->reserve(static_cast<size_t>(len) + 1);
  for (uint32_t i = 0; i < len; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, value, i, &item) != napi_ok || item == nullptr) return false;
    napi_value string_value = nullptr;
    if (napi_coerce_to_string(env, item, &string_value) != napi_ok || string_value == nullptr) return false;
    size_t slen = 0;
    if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &slen) != napi_ok) return false;
    std::string text(slen + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, string_value, text.data(), text.size(), &copied) != napi_ok) return false;
    text.resize(copied);
    storage->push_back(std::move(text));
  }
  for (std::string& text : *storage) {
    out->push_back(const_cast<char*>(text.c_str()));
  }
  out->push_back(nullptr);
  return true;
}

void RebuildEnvPointers(std::vector<std::string>* storage, std::vector<char*>* out) {
  if (storage == nullptr || out == nullptr) return;
  out->clear();
  out->reserve(storage->size() + 1);
  for (std::string& text : *storage) {
    out->push_back(const_cast<char*>(text.c_str()));
  }
  out->push_back(nullptr);
}

void UpdateEnvPairInt(std::vector<std::string>* storage,
                      std::vector<char*>* out,
                      const char* key,
                      int value) {
  if (storage == nullptr || out == nullptr || key == nullptr) return;
  const std::string prefix = std::string(key) + "=";
  const std::string replacement = prefix + std::to_string(value);
  for (std::string& text : *storage) {
    if (text.rfind(prefix, 0) == 0) {
      text = replacement;
      RebuildEnvPointers(storage, out);
      return;
    }
  }
}

#if defined(__wasi__)
// WASIX (Wasmer) preopens directories on FDs 3 through 5 (virtual root,
// "/", and "."). proc_spawn refuses dup2 onto preopened FDs, which breaks
// Node's default IPC stdio slot at index 3 with ENOTSUP. Use FD 10 to stay
// clear of the reserved range with extra headroom.
constexpr int kWasixIpcFd = 10;

void SetIgnoreStdio(uv_stdio_container_t* entry) {
  if (entry == nullptr) return;
  std::memset(entry, 0, sizeof(*entry));
  entry->flags = UV_IGNORE;
  entry->data.fd = -1;
}

void RelocateWasixIpcStdio(std::vector<uv_stdio_container_t>* stdio, int ipc_index) {
  if (stdio == nullptr || ipc_index < 0 || ipc_index == kWasixIpcFd) return;

  const size_t ipc_entry_index = static_cast<size_t>(ipc_index);
  if (ipc_entry_index >= stdio->size()) return;

  uv_stdio_container_t ipc_entry = (*stdio)[ipc_entry_index];
  SetIgnoreStdio(&(*stdio)[ipc_entry_index]);

  while (stdio->size() < static_cast<size_t>(kWasixIpcFd)) {
    uv_stdio_container_t ignore{};
    SetIgnoreStdio(&ignore);
    stdio->push_back(ignore);
  }

  if (stdio->size() == static_cast<size_t>(kWasixIpcFd)) {
    stdio->push_back(ipc_entry);
  } else {
    (*stdio)[static_cast<size_t>(kWasixIpcFd)] = ipc_entry;
  }
}
#endif

bool ParseStdioOptions(napi_env env,
                       napi_value js_options,
                       std::vector<uv_stdio_container_t>* options_stdio,
                       int* ipc_index) {
  if (options_stdio == nullptr) return false;
  options_stdio->clear();
  if (ipc_index != nullptr) *ipc_index = -1;

  napi_value stdio_value = nullptr;
  if (!GetNamedValue(env, js_options, "stdio", &stdio_value) || stdio_value == nullptr) return true;

  bool is_array = false;
  if (napi_is_array(env, stdio_value, &is_array) != napi_ok || !is_array) return false;

  uint32_t len = 0;
  if (napi_get_array_length(env, stdio_value, &len) != napi_ok) return false;
  options_stdio->resize(len);

  for (uint32_t i = 0; i < len; ++i) {
    uv_stdio_container_t& entry = (*options_stdio)[i];
    std::memset(&entry, 0, sizeof(entry));
    entry.flags = UV_IGNORE;
    entry.data.fd = -1;

    napi_value stdio = nullptr;
    if (napi_get_element(env, stdio_value, i, &stdio) != napi_ok || stdio == nullptr) return false;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, stdio, &type) != napi_ok || type != napi_object) return false;

    napi_value type_value = nullptr;
    if (!GetNamedValue(env, stdio, "type", &type_value) || type_value == nullptr) return false;

    std::string stdio_type;
    if (!GetStringProperty(env, stdio, "type", &stdio_type)) return false;

    if (stdio_type == "ignore") {
      entry.flags = UV_IGNORE;
      continue;
    }

    if (stdio_type == "pipe" || stdio_type == "overlapped") {
      napi_value handle_value = nullptr;
      if (!GetNamedValue(env, stdio, "handle", &handle_value) || handle_value == nullptr) return false;
      uv_stream_t* stream = EdgeStreamBaseGetLibuvStream(env, handle_value);
      if (stream == nullptr) return false;
      uv_stdio_flags flags = static_cast<uv_stdio_flags>(UV_CREATE_PIPE | UV_READABLE_PIPE | UV_WRITABLE_PIPE);
      if (stdio_type == "overlapped") {
        flags = static_cast<uv_stdio_flags>(flags | UV_OVERLAPPED_PIPE);
      }
      entry.flags = flags;
      entry.data.stream = stream;
      if (ipc_index != nullptr && IsTruthyProperty(env, stdio, "ipc")) {
        *ipc_index = static_cast<int>(i);
      }
      continue;
    }

    if (stdio_type == "wrap") {
      napi_value handle_value = nullptr;
      if (!GetNamedValue(env, stdio, "handle", &handle_value) || handle_value == nullptr) return false;
      uv_stream_t* stream = EdgeStreamBaseGetLibuvStream(env, handle_value);
      if (stream == nullptr) return false;
      entry.flags = UV_INHERIT_STREAM;
      entry.data.stream = stream;
      continue;
    }

    napi_value fd_value = nullptr;
    if (!GetNamedValue(env, stdio, "fd", &fd_value) || fd_value == nullptr) return false;
    int32_t fd = -1;
    if (napi_get_value_int32(env, fd_value, &fd) != napi_ok || fd < 0) return false;
    entry.flags = UV_INHERIT_FD;
    entry.data.fd = fd;
  }

  return true;
}

void OnProcessClose(uv_handle_t* handle) {
  auto* wrap = static_cast<ProcessWrap*>(handle->data);
  if (wrap == nullptr) return;
  wrap->alive = false;
  UntrackLiveChildPid(wrap->pid);
  EdgeHandleWrapCompleteClose(&wrap->handle_wrap, wrap, DeleteProcessWrap);
}

void CloseProcessWrapForCleanup(void* data) {
  auto* wrap = static_cast<ProcessWrap*>(data);
  if (wrap == nullptr || wrap->handle_wrap.state != kEdgeHandleInitialized) return;
  uv_handle_t* handle = reinterpret_cast<uv_handle_t*>(&wrap->process);
  if (!uv_is_closing(handle)) {
    wrap->handle_wrap.state = kEdgeHandleClosing;
    uv_close(handle, OnProcessClose);
  }
}

void EmitOnExit(ProcessWrap* wrap, int64_t exit_status, int term_signal) {
  if (wrap == nullptr ||
      wrap->handle_wrap.env == nullptr ||
      wrap->handle_wrap.wrapper_ref == nullptr ||
      wrap->handle_wrap.finalized) {
    return;
  }
  edge::HandleScope scope(wrap->handle_wrap.env);
  if (!scope.is_open()) return;
  napi_value self = EdgeHandleWrapGetRefValue(wrap->handle_wrap.env, wrap->handle_wrap.wrapper_ref);
  if (self == nullptr) return;

  napi_value onexit = nullptr;
  bool has_onexit = false;
  if (napi_has_named_property(wrap->handle_wrap.env, self, "onexit", &has_onexit) != napi_ok || !has_onexit) return;
  if (napi_get_named_property(wrap->handle_wrap.env, self, "onexit", &onexit) != napi_ok || onexit == nullptr) return;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(wrap->handle_wrap.env, onexit, &type) != napi_ok || type != napi_function) return;

  napi_value argv[2] = {nullptr, nullptr};
  argv[0] = MakeInt32(wrap->handle_wrap.env, static_cast<int32_t>(exit_status));
  napi_create_string_utf8(wrap->handle_wrap.env, SignalNameFromNumber(term_signal), NAPI_AUTO_LENGTH, &argv[1]);

  napi_value ignored = nullptr;
  EdgeAsyncWrapMakeCallback(
      wrap->handle_wrap.env, wrap->async_id, self, self, onexit, 2, argv, &ignored, 0);
}

void OnProcessExit(uv_process_t* process, int64_t exit_status, int term_signal) {
  auto* wrap = static_cast<ProcessWrap*>(process->data);
  if (wrap == nullptr || wrap->handle_wrap.state == kEdgeHandleClosed) return;
  wrap->alive = false;
  UntrackLiveChildPid(wrap->pid);
  EmitOnExit(wrap, exit_status, term_signal);
}

void ProcessFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<ProcessWrap*>(data);
  if (wrap == nullptr) return;
  // If the wrapper is collected while the child is still tracked, stop tracking
  // it; the shared finalizer initiates close (-> OnProcessClose) which also
  // untracks, but doing it here keeps the live-child set tight.
  if (wrap->handle_wrap.state == kEdgeHandleInitialized) {
    wrap->alive = false;
    UntrackLiveChildPid(wrap->pid);
  }
  EdgeHandleWrapFinalizeNative(env, &wrap->handle_wrap, wrap, DeleteProcessWrap);
}

napi_value ProcessCtor(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &self, nullptr) != napi_ok || self == nullptr) {
    return internal_binding::Undefined(env);
  }

  auto* wrap = new ProcessWrap();
  EdgeHandleWrapInit(&wrap->handle_wrap, env);
  wrap->async_id = EdgeAsyncWrapNextId(env);
  wrap->pid = 0;
  wrap->alive = false;
  wrap->destroy_queued = false;
  wrap->process = {};

  if (napi_wrap(env, self, wrap, ProcessFinalize, nullptr, &wrap->handle_wrap.wrapper_ref) != napi_ok) {
    delete wrap;
    return internal_binding::Undefined(env);
  }

  wrap->handle_wrap.active_handle_token =
      EdgeRegisterActiveHandle(
          env, self, "PROCESSWRAP", HasRef, GetActiveOwner, wrap, CloseProcessWrapForCleanup);

  SetPidUndefined(env, self);
  EdgeAsyncWrapEmitInit(
      env, wrap->async_id, kEdgeProviderProcessWrap, EdgeAsyncWrapExecutionAsyncId(env), self);
  return self;
}

napi_value ProcessSpawn(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr || argc < 1 ||
      argv[0] == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }

  ProcessWrap* wrap = nullptr;
  if (!UnwrapProcess(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state != kEdgeHandleUninitialized) return MakeInt32(env, UV_EINVAL);

  napi_valuetype options_type = napi_undefined;
  if (napi_typeof(env, argv[0], &options_type) != napi_ok || options_type != napi_object) {
    return MakeInt32(env, UV_EINVAL);
  }

  std::string file;
  if (!GetStringProperty(env, argv[0], "file", &file) || file.empty()) return MakeInt32(env, UV_EINVAL);

  napi_value args_value = nullptr;
  std::vector<std::string> args_storage;
  std::vector<char*> args;
  if (GetNamedValue(env, argv[0], "args", &args_value) && args_value != nullptr) {
    if (!ParseStringArray(env, args_value, &args_storage, &args)) return MakeInt32(env, UV_EINVAL);
  }
  if (args_storage.empty()) {
    args.clear();
    args_storage.push_back(file);
    args.push_back(const_cast<char*>(args_storage.back().c_str()));
    args.push_back(nullptr);
  }

  napi_value env_pairs_value = nullptr;
  std::vector<std::string> env_storage;
  std::vector<char*> envp;
  if (GetNamedValue(env, argv[0], "envPairs", &env_pairs_value) && env_pairs_value != nullptr) {
    if (!ParseStringArray(env, env_pairs_value, &env_storage, &envp)) return MakeInt32(env, UV_EINVAL);
  }

  std::string cwd;
  const bool has_cwd = GetStringProperty(env, argv[0], "cwd", &cwd);
  const bool detached = IsTruthyProperty(env, argv[0], "detached");
  const bool windows_hide = IsTruthyProperty(env, argv[0], "windowsHide");
  const bool windows_verbatim_arguments = IsTruthyProperty(env, argv[0], "windowsVerbatimArguments");

  int32_t uid = 0;
  int32_t gid = 0;
  const bool has_uid = GetInt32Property(env, argv[0], "uid", &uid);
  const bool has_gid = GetInt32Property(env, argv[0], "gid", &gid);

  std::vector<uv_stdio_container_t> stdio;
  int ipc_index = -1;
  if (!ParseStdioOptions(env, argv[0], &stdio, &ipc_index)) return MakeInt32(env, UV_EINVAL);
  if (stdio.empty()) {
    stdio.resize(3);
    for (uint32_t i = 0; i < 3; ++i) {
      stdio[i].flags = UV_INHERIT_FD;
      stdio[i].data.fd = static_cast<int>(i);
    }
  }
#if defined(__wasi__)
  if (ipc_index >= 0) {
    RelocateWasixIpcStdio(&stdio, ipc_index);
    if (!env_storage.empty()) {
      UpdateEnvPairInt(&env_storage, &envp, "NODE_CHANNEL_FD", kWasixIpcFd);
    }
  }
#endif

  uv_process_options_t options{};
  options.file = file.c_str();
  options.args = args.data();
  options.exit_cb = OnProcessExit;
  options.flags = 0;
  if (detached) options.flags |= UV_PROCESS_DETACHED;
  if (windows_hide) options.flags |= UV_PROCESS_WINDOWS_HIDE;
  if (windows_verbatim_arguments) options.flags |= UV_PROCESS_WINDOWS_VERBATIM_ARGUMENTS;
  if (has_uid) {
    options.flags |= UV_PROCESS_SETUID;
    options.uid = static_cast<uv_uid_t>(uid);
  }
  if (has_gid) {
    options.flags |= UV_PROCESS_SETGID;
    options.gid = static_cast<uv_gid_t>(gid);
  }
  if (has_cwd && !cwd.empty()) options.cwd = cwd.c_str();
  if (!envp.empty()) options.env = envp.data();
  options.stdio_count = static_cast<int>(stdio.size());
  options.stdio = stdio.data();

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  if (loop == nullptr) return MakeInt32(env, UV_EINVAL);

  wrap->process = {};
  wrap->process.data = wrap;
  wrap->alive = false;
  wrap->handle_wrap.delete_on_close = false;
  wrap->handle_wrap.state = kEdgeHandleUninitialized;

  const int rc = uv_spawn(loop, &wrap->process, &options);
  wrap->handle_wrap.state = kEdgeHandleInitialized;
  EdgeHandleWrapAttach(&wrap->handle_wrap,
                      wrap,
                      reinterpret_cast<uv_handle_t*>(&wrap->process),
                      CloseProcessWrapForCleanup);
  // No explicit wrapper pin needed: EdgeRegisterActiveHandle already keeps the
  // JS wrapper alive (a strong keepalive_ref) while the handle is active, and
  // it is released on close/unregister. The shared close/finalize helpers
  // handle the re-entrancy safety.

  if (rc != 0) {
    wrap->pid = 0;
    wrap->alive = false;
    SetPidUndefined(env, self);
    if (DebugProcessWrapEnabled()) {
      std::cerr << "[edge-process-wrap] spawn file=" << file << " rc=" << rc << "\n";
    }
    return MakeInt32(env, rc);
  }

  wrap->pid = static_cast<int32_t>(wrap->process.pid);
  wrap->alive = true;
  TrackLiveChildPid(wrap->pid);
  SetPidProperty(env, self, wrap->pid);

  if (DebugProcessWrapEnabled()) {
    std::cerr << "[edge-process-wrap] spawn file=" << file << " pid=" << wrap->pid << "\n";
  }
  return MakeInt32(env, 0);
}

napi_value ProcessKill(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr) {
    return MakeInt32(env, UV_EINVAL);
  }

  ProcessWrap* wrap = nullptr;
  if (!UnwrapProcess(env, self, &wrap)) return nullptr;
  if (wrap->handle_wrap.state != kEdgeHandleInitialized) return MakeInt32(env, UV_ESRCH);

  int32_t signal = SIGTERM;
  if (argc >= 1 && argv[0] != nullptr && napi_get_value_int32(env, argv[0], &signal) != napi_ok) {
    return MakeInt32(env, UV_EINVAL);
  }
#ifdef _WIN32
  if (signal != SIGKILL && signal != SIGTERM && signal != SIGINT && signal != SIGQUIT && signal != 0) {
    signal = SIGKILL;
  }
#endif
  const int rc = uv_process_kill(&wrap->process, signal);
  if (rc == UV_ESRCH) {
    wrap->alive = false;
    UntrackLiveChildPid(wrap->pid);
  }
  if (DebugProcessWrapEnabled()) {
    std::cerr << "[edge-process-wrap] kill pid=" << wrap->pid << " signal=" << signal << " rc=" << rc << "\n";
  }
  return MakeInt32(env, rc);
}

napi_value ProcessClose(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value self = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &self, nullptr) != napi_ok || self == nullptr) {
    return internal_binding::Undefined(env);
  }

  ProcessWrap* wrap = nullptr;
  if (!UnwrapProcess(env, self, &wrap)) return nullptr;

  if (wrap->handle_wrap.state != kEdgeHandleInitialized) {
    return internal_binding::Undefined(env);
  }

  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      EdgeHandleWrapSetOnCloseCallback(env, self, argv[0]);
    }
  }

  wrap->alive = false;
  UntrackLiveChildPid(wrap->pid);

  CloseProcessWrapForCleanup(wrap);

  return internal_binding::Undefined(env);
}

napi_value ProcessRef(napi_env env, napi_callback_info info) {
  ProcessWrap* wrap = GetThis(env, info, nullptr);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_ref(reinterpret_cast<uv_handle_t*>(&wrap->process));
  }
  return internal_binding::Undefined(env);
}

napi_value ProcessUnref(napi_env env, napi_callback_info info) {
  ProcessWrap* wrap = GetThis(env, info, nullptr);
  if (wrap != nullptr && wrap->handle_wrap.state == kEdgeHandleInitialized) {
    uv_unref(reinterpret_cast<uv_handle_t*>(&wrap->process));
  }
  return internal_binding::Undefined(env);
}

napi_value ProcessHasRef(napi_env env, napi_callback_info info) {
  ProcessWrap* wrap = GetThis(env, info, nullptr);
  return MakeBool(env, HasRef(wrap));
}

napi_value ProcessGetAsyncId(napi_env env, napi_callback_info info) {
  ProcessWrap* wrap = GetThis(env, info, nullptr);
  return MakeInt64(env, wrap != nullptr ? wrap->async_id : -1);
}

napi_value ProcessGetProviderType(napi_env env, napi_callback_info info) {
  ProcessWrap* wrap = GetThis(env, info, nullptr);
  return MakeInt32(env, wrap != nullptr ? kEdgeProviderProcessWrap : 0);
}

napi_value ProcessAsyncReset(napi_env env, napi_callback_info info) {
  napi_value self = nullptr;
  ProcessWrap* wrap = GetThis(env, info, &self);
  if (wrap == nullptr) return nullptr;
  EdgeAsyncWrapReset(env, &wrap->async_id);
  EdgeAsyncWrapEmitInit(
      env, wrap->async_id, kEdgeProviderProcessWrap, EdgeAsyncWrapExecutionAsyncId(env), self);
  return internal_binding::Undefined(env);
}

}  // namespace

napi_value EdgeInstallProcessWrapBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_property_descriptor methods[] = {
      {"spawn", nullptr, ProcessSpawn, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"kill", nullptr, ProcessKill, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"close", nullptr, ProcessClose, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"ref", nullptr, ProcessRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"unref", nullptr, ProcessUnref, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"hasRef", nullptr, ProcessHasRef, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getAsyncId", nullptr, ProcessGetAsyncId, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"getProviderType", nullptr, ProcessGetProviderType, nullptr, nullptr, nullptr, napi_default_method,
       nullptr},
      {"asyncReset", nullptr, ProcessAsyncReset, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };

  napi_value process_ctor = nullptr;
  if (napi_define_class(env,
                        "Process",
                        NAPI_AUTO_LENGTH,
                        ProcessCtor,
                        nullptr,
                        sizeof(methods) / sizeof(methods[0]),
                        methods,
                        &process_ctor) != napi_ok ||
      process_ctor == nullptr) {
    return nullptr;
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;
  napi_set_named_property(env, binding, "Process", process_ctor);
  return binding;
}

void EdgeProcessWrapForceKillTrackedChildren() {
  std::vector<int32_t> pids;
  {
    std::lock_guard<std::mutex> lock(g_live_child_pids_mutex);
    pids.reserve(g_live_child_pids.size());
    for (int32_t pid : g_live_child_pids) {
      if (pid > 0) pids.push_back(pid);
    }
  }

  for (int32_t pid : pids) {
    const int rc = uv_kill(pid, SIGKILL);
    if (DebugProcessWrapEnabled()) {
      std::cerr << "[edge-process-wrap] cleanup kill pid=" << pid << " rc=" << rc << "\n";
    }
    if (rc == 0 || rc == UV_ESRCH) {
      UntrackLiveChildPid(pid);
    }
  }

  for (int attempt = 0; attempt < 50; ++attempt) {
    bool any_alive = false;
    for (int32_t pid : pids) {
      if (!ProcessExists(pid)) {
        UntrackLiveChildPid(pid);
        continue;
      }
      any_alive = true;
      if ((attempt % 8) == 7) {
        const int rc = uv_kill(pid, SIGKILL);
        if (DebugProcessWrapEnabled()) {
          std::cerr << "[edge-process-wrap] cleanup retry pid=" << pid << " rc=" << rc << "\n";
        }
      }
    }
    if (!any_alive) break;
    std::this_thread::sleep_for(std::chrono::milliseconds(1));
  }
}
