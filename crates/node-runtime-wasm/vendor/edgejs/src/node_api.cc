#include "node_api.h"
#include "node_api_types.h"
#include "edge_async_wrap.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_module_loader.h"
#include "edge_runtime.h"

#include <atomic>
#include <new>
#include <string>
#include <unordered_map>
#include <vector>

#include <uv.h>

struct napi_async_context__ {
  napi_env env = nullptr;
  napi_ref resource_ref = nullptr;
  std::string resource_type;
  int64_t async_id = 0;
  int64_t trigger_async_id = 0;
  bool externally_managed_resource = false;
  bool destroyed = false;
};

enum class AsyncWorkState : uint8_t {
  kCreated = 0,
  kQueued = 1,
  kCompleting = 2,
  kSettled = 3,
};

struct napi_async_work__ {
  napi_env env = nullptr;
  napi_async_execute_callback execute = nullptr;
  napi_async_complete_callback complete = nullptr;
  void* data = nullptr;
  napi_async_context async_context = nullptr;
  uv_work_t req{};
  AsyncWorkState state = AsyncWorkState::kCreated;
  bool delete_pending_from_complete = false;
};

struct napi_threadsafe_function__ {
  napi_env env = nullptr;
  napi_threadsafe_function_call_js call_js_cb = nullptr;
  napi_finalize finalize_cb = nullptr;
  void* finalize_data = nullptr;
  void* context = nullptr;
  std::atomic<uint32_t> refcount{0};
  std::atomic<bool> finalized{false};
};

struct napi_callback_scope__ {
  napi_env env = nullptr;
  napi_async_context async_context = nullptr;
  bool entered = false;
};

void napi_v8_run_async_cleanup_hooks(napi_env env);

namespace {

struct DetachedEnvState {
  std::vector<napi_async_cleanup_hook_handle> async_cleanup_hooks;
  bool async_cleanup_hook_registered = false;
  uv_loop_t* loop = nullptr;
  bool loop_cleanup_hook_registered = false;
  size_t callback_scope_depth = 0;
  size_t open_callback_scopes = 0;
};

std::unordered_map<napi_env, DetachedEnvState> g_detached_env_state;

inline bool CheckEnv(napi_env env) {
  return env != nullptr;
}

edge::Environment* GetAttachedEnvironment(napi_env env) {
  return EdgeEnvironmentGet(env);
}

DetachedEnvState& GetOrCreateDetachedEnvState(napi_env env) {
  return g_detached_env_state[env];
}

DetachedEnvState* GetDetachedEnvState(napi_env env) {
  auto it = g_detached_env_state.find(env);
  if (it == g_detached_env_state.end()) return nullptr;
  return &it->second;
}

void MaybeEraseDetachedEnvState(napi_env env) {
  auto it = g_detached_env_state.find(env);
  if (it == g_detached_env_state.end()) return;
  const DetachedEnvState& state = it->second;
  if (!state.async_cleanup_hook_registered &&
      state.async_cleanup_hooks.empty() &&
      !state.loop_cleanup_hook_registered &&
      state.loop == nullptr &&
      state.callback_scope_depth == 0 &&
      state.open_callback_scopes == 0) {
    g_detached_env_state.erase(it);
  }
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

void DrainAndCloseEnvLoop(uv_loop_t* loop) {
  if (loop == nullptr) return;
  CloseEnvLoopHandles(loop);
  for (size_t guard = 0; guard < 1024; ++guard) {
    if (uv_run(loop, UV_RUN_NOWAIT) == 0) {
      if (uv_loop_close(loop) == 0) return;
    }
    CloseEnvLoopHandles(loop);
  }

  CloseEnvLoopHandles(loop);
  while (uv_run(loop, UV_RUN_NOWAIT) != 0) {
    CloseEnvLoopHandles(loop);
  }
  (void)uv_loop_close(loop);
}

void CleanupEnvLoopOnTeardown(void* arg) {
  auto* env = static_cast<napi_env>(arg);
  if (!CheckEnv(env)) return;
  auto* state = GetDetachedEnvState(env);
  if (state == nullptr) return;

  uv_loop_t* loop = state->loop;
  state->loop = nullptr;
  state->loop_cleanup_hook_registered = false;
  if (loop != nullptr) {
    DrainAndCloseEnvLoop(loop);
    delete loop;
  }
  EdgeFinalizeModuleLoaderEnv(env);
  MaybeEraseDetachedEnvState(env);
}

void RunAsyncCleanupHooksOnEnvTeardown(void* arg) {
  auto* env = static_cast<napi_env>(arg);
  if (!CheckEnv(env)) return;
  napi_v8_run_async_cleanup_hooks(env);
}

bool HasPendingException(napi_env env) {
  bool pending = false;
  return CheckEnv(env) &&
         napi_is_exception_pending(env, &pending) == napi_ok &&
         pending;
}

napi_status CopyStringValue(napi_env env, napi_value value, std::string* out) {
  if (!CheckEnv(env) || value == nullptr || out == nullptr) return napi_invalid_arg;
  size_t length = 0;
  napi_status status = napi_get_value_string_utf8(env, value, nullptr, 0, &length);
  if (status != napi_ok) return status;
  out->assign(length, '\0');
  size_t copied = 0;
  if (length == 0) return napi_ok;
  status = napi_get_value_string_utf8(env, value, out->data(), length + 1, &copied);
  if (status != napi_ok) return status;
  out->resize(copied);
  return napi_ok;
}

void ResetReference(napi_env env, napi_ref* ref) {
  if (!CheckEnv(env) || ref == nullptr || *ref == nullptr) return;
  (void)napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void DestroyAsyncContext(napi_async_context async_context) {
  if (async_context == nullptr || async_context->destroyed) return;
  async_context->destroyed = true;
  napi_env env = async_context->env;
  if (async_context->async_id > 0) {
    EdgeAsyncWrapQueueDestroyId(env, async_context->async_id);
  }
  ResetReference(env, &async_context->resource_ref);
}

napi_status CreateAsyncContext(napi_env env,
                               napi_value async_resource,
                               napi_value async_resource_name,
                               napi_async_context* result) {
  if (!CheckEnv(env) || async_resource_name == nullptr || result == nullptr) {
    return napi_invalid_arg;
  }

  napi_value resource = nullptr;
  bool externally_managed_resource = false;
  if (async_resource != nullptr) {
    napi_status status = napi_coerce_to_object(env, async_resource, &resource);
    if (status != napi_ok || resource == nullptr) return status;
    externally_managed_resource = true;
  } else {
    napi_status status = napi_create_object(env, &resource);
    if (status != napi_ok || resource == nullptr) return status;
  }

  napi_value resource_name = nullptr;
  napi_status status = napi_coerce_to_string(env, async_resource_name, &resource_name);
  if (status != napi_ok || resource_name == nullptr) return status;

  auto* async_context = new (std::nothrow) napi_async_context__();
  if (async_context == nullptr) return napi_generic_failure;
  async_context->env = env;
  async_context->async_id = EdgeAsyncWrapNextId(env);
  async_context->trigger_async_id = EdgeAsyncWrapCurrentExecutionAsyncId(env);
  async_context->externally_managed_resource = externally_managed_resource;

  status = CopyStringValue(env, resource_name, &async_context->resource_type);
  if (status != napi_ok) {
    delete async_context;
    return status;
  }

  const uint32_t initial_refcount = externally_managed_resource ? 0u : 1u;
  status = napi_create_reference(env, resource, initial_refcount, &async_context->resource_ref);
  if (status != napi_ok) {
    delete async_context;
    return status;
  }

  EdgeAsyncWrapEmitInitString(env,
                             async_context->async_id,
                             async_context->resource_type.c_str(),
                             async_context->trigger_async_id,
                             resource);
  *result = async_context;
  return napi_ok;
}

napi_status EnsureAsyncContextResource(napi_async_context async_context, napi_value* resource) {
  if (async_context == nullptr || resource == nullptr) return napi_invalid_arg;
  napi_env env = async_context->env;
  *resource = nullptr;

  if (async_context->resource_ref != nullptr) {
    napi_value existing = nullptr;
    napi_status status =
        napi_get_reference_value(env, async_context->resource_ref, &existing);
    if (status != napi_ok) return status;
    if (existing != nullptr) {
      *resource = existing;
      return napi_ok;
    }
  }

  napi_value replacement = nullptr;
  napi_status status = napi_create_object(env, &replacement);
  if (status != napi_ok || replacement == nullptr) return status;

  ResetReference(env, &async_context->resource_ref);
  status = napi_create_reference(env, replacement, 1, &async_context->resource_ref);
  if (status != napi_ok) return status;

  async_context->externally_managed_resource = false;
  *resource = replacement;
  return napi_ok;
}

napi_status EnterAsyncContextScope(napi_env env, napi_async_context async_context) {
  if (!CheckEnv(env) || async_context == nullptr || async_context->env != env) {
    return napi_invalid_arg;
  }

  napi_value resource = nullptr;
  napi_status status = EnsureAsyncContextResource(async_context, &resource);
  if (status != napi_ok) return status;

  EdgeAsyncWrapPushContext(
      env, async_context->async_id, async_context->trigger_async_id, resource);
  if (async_context->async_id > 0) {
    EdgeAsyncWrapEmitBefore(env, async_context->async_id);
  }
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    environment->IncrementCallbackScopeDepth();
  } else {
    GetOrCreateDetachedEnvState(env).callback_scope_depth++;
  }
  return napi_ok;
}

void ExitAsyncContextScope(napi_env env,
                           napi_async_context async_context,
                           bool failed) {
  if (!CheckEnv(env) || async_context == nullptr) return;

  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    environment->DecrementCallbackScopeDepth();
  } else if (auto* state = GetDetachedEnvState(env);
             state != nullptr && state->callback_scope_depth > 0) {
    state->callback_scope_depth--;
  }

  if (!failed && async_context->async_id > 0) {
    EdgeAsyncWrapEmitAfter(env, async_context->async_id);
  }
  (void)EdgeAsyncWrapPopContext(env, async_context->async_id);

  size_t callback_scope_depth = 0;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    callback_scope_depth = environment->callback_scope_depth();
  } else if (auto* state = GetDetachedEnvState(env); state != nullptr) {
    callback_scope_depth = state->callback_scope_depth;
  }
  if (failed || callback_scope_depth != 0 || HasPendingException(env)) {
    return;
  }
  (void)EdgeRunCallbackScopeCheckpoint(env);
}

void DeleteAsyncWork(napi_async_work work) {
  if (work == nullptr) return;
  if (work->async_context != nullptr) {
    DestroyAsyncContext(work->async_context);
    delete work->async_context;
    work->async_context = nullptr;
  }
  delete work;
}

void UvExecute(uv_work_t* req) {
  auto* work = static_cast<napi_async_work__*>(req->data);
  if (work == nullptr || work->execute == nullptr) return;
  work->execute(work->env, work->data);
}

void UvAfterWork(uv_work_t* req, int status) {
  auto* work = static_cast<napi_async_work__*>(req->data);
  if (work == nullptr) return;
  work->state = AsyncWorkState::kCompleting;
  napi_status napi_status_code = (status == UV_ECANCELED) ? napi_cancelled : napi_ok;
  if (work->complete != nullptr && work->env != nullptr) {
    edge::HandleScope scope(work->env);
    if (!scope.is_open()) {
      work->state = AsyncWorkState::kSettled;
      if (work->delete_pending_from_complete) {
        DeleteAsyncWork(work);
      }
      return;
    }
    const napi_status scope_status = EnterAsyncContextScope(work->env, work->async_context);
    const bool scope_entered = (scope_status == napi_ok);
    work->complete(work->env, napi_status_code, work->data);
    if (scope_entered) {
      ExitAsyncContextScope(work->env, work->async_context, HasPendingException(work->env));
    }
  }
  work->state = AsyncWorkState::kSettled;
  if (work->delete_pending_from_complete) {
    DeleteAsyncWork(work);
  }
}

}  // namespace

void napi_v8_run_async_cleanup_hooks(napi_env env) {
  if (!CheckEnv(env)) return;

  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    environment->RunAsyncCleanupHooks();
    return;
  }

  auto* state = GetDetachedEnvState(env);
  if (state == nullptr) return;

  std::vector<napi_async_cleanup_hook_handle> pending;
  pending.reserve(state->async_cleanup_hooks.size());
  for (auto* handle : state->async_cleanup_hooks) {
    if (handle != nullptr && !handle->removed) {
      pending.push_back(handle);
    }
  }

  for (auto* handle : pending) {
    if (handle != nullptr && !handle->removed && handle->hook != nullptr) {
      handle->hook(handle, handle->arg);
    }
  }

  size_t guard = 0;
  uv_loop_t* loop = EdgeGetExistingEnvLoop(env);
  while (!state->async_cleanup_hooks.empty() && guard++ < 128) {
    if (loop != nullptr) {
      uv_run(loop, UV_RUN_DEFAULT);
    }
    bool any_left = false;
    for (auto* handle : state->async_cleanup_hooks) {
      if (handle != nullptr && !handle->removed) {
        any_left = true;
        break;
      }
    }
    if (!any_left) break;
  }

  for (auto* handle : state->async_cleanup_hooks) {
    delete handle;
  }
  state->async_cleanup_hooks.clear();
  state->async_cleanup_hook_registered = false;
  MaybeEraseDetachedEnvState(env);
}

extern "C" {

napi_status NAPI_CDECL node_api_post_finalizer(node_api_basic_env env,
                                               napi_finalize finalize_cb,
                                               void* finalize_data,
                                               void* finalize_hint) {
  napi_env napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || finalize_cb == nullptr) return napi_invalid_arg;
  finalize_cb(napiEnv, finalize_data, finalize_hint);
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_node_version(
    node_api_basic_env env, const napi_node_version** version) {
#ifdef NODE_MAJOR_VERSION
  static const uint32_t kMajor = NODE_MAJOR_VERSION;
  static const uint32_t kMinor = NODE_MINOR_VERSION;
  static const uint32_t kPatch = NODE_PATCH_VERSION;
  static const char* kRelease = NODE_RELEASE;
#else
  static const uint32_t kMajor = 0;
  static const uint32_t kMinor = 0;
  static const uint32_t kPatch = 0;
  static const char* kRelease = "node";
#endif
  static const napi_node_version kVersion = {
      kMajor, kMinor, kPatch, kRelease};
  if (version == nullptr) return napi_invalid_arg;
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv)) return napi_invalid_arg;
  *version = &kVersion;
  return napi_ok;
}

napi_status NAPI_CDECL node_api_get_module_file_name(
    node_api_basic_env env, const char** result) {
  static const char kModuleUrl[] = "file:///napi-v8-addon.node";
  if (result == nullptr) return napi_invalid_arg;
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv)) return napi_invalid_arg;
  *result = kModuleUrl;
  return napi_ok;
}

napi_status NAPI_CDECL napi_create_async_work(napi_env env,
                                              napi_value async_resource,
                                              napi_value async_resource_name,
                                              napi_async_execute_callback execute,
                                              napi_async_complete_callback complete,
                                              void* data,
                                              napi_async_work* result) {
  if (!CheckEnv(env) ||
      async_resource_name == nullptr ||
      execute == nullptr ||
      result == nullptr) {
    return napi_invalid_arg;
  }
  auto* work = new (std::nothrow) napi_async_work__();
  if (work == nullptr) return napi_generic_failure;

  napi_async_context async_context = nullptr;
  const napi_status status =
      CreateAsyncContext(env, async_resource, async_resource_name, &async_context);
  if (status != napi_ok || async_context == nullptr) {
    delete work;
    return status == napi_ok ? napi_generic_failure : status;
  }

  work->env = env;
  work->execute = execute;
  work->complete = complete;
  work->data = data;
  work->async_context = async_context;
  work->req.data = work;
  *result = work;
  return napi_ok;
}

napi_status NAPI_CDECL napi_delete_async_work(napi_env env, napi_async_work work) {
  if (!CheckEnv(env) || work == nullptr) return napi_invalid_arg;
  switch (work->state) {
    case AsyncWorkState::kCreated:
    case AsyncWorkState::kSettled:
      DeleteAsyncWork(work);
      return napi_ok;
    case AsyncWorkState::kCompleting:
      work->delete_pending_from_complete = true;
      return napi_ok;
    case AsyncWorkState::kQueued:
      return napi_generic_failure;
  }
  return napi_generic_failure;
}

napi_status NAPI_CDECL napi_queue_async_work(node_api_basic_env env, napi_async_work work) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || work == nullptr) return napi_invalid_arg;
  if (work->state != AsyncWorkState::kCreated) return napi_generic_failure;
  uv_loop_t* loop = EdgeGetEnvLoop(napiEnv);
  if (loop == nullptr) return napi_generic_failure;
  int rc = uv_queue_work(loop, &work->req, UvExecute, UvAfterWork);
  if (rc != 0) return napi_generic_failure;
  work->state = AsyncWorkState::kQueued;
  return napi_ok;
}

napi_status NAPI_CDECL napi_cancel_async_work(node_api_basic_env env, napi_async_work work) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || work == nullptr) return napi_invalid_arg;
  if (work->state != AsyncWorkState::kQueued) return napi_generic_failure;
  int rc = uv_cancel(reinterpret_cast<uv_req_t*>(&work->req));
  return (rc == 0) ? napi_ok : napi_generic_failure;
}

napi_status NAPI_CDECL napi_create_threadsafe_function(
    napi_env env,
    napi_value func,
    napi_value async_resource,
    napi_value async_resource_name,
    size_t max_queue_size,
    size_t initial_thread_count,
    void* thread_finalize_data,
    napi_finalize thread_finalize_cb,
    void* context,
    napi_threadsafe_function_call_js call_js_cb,
    napi_threadsafe_function* result) {
  (void)func;
  (void)async_resource;
  (void)async_resource_name;
  (void)max_queue_size;
  if (!CheckEnv(env) || result == nullptr) return napi_invalid_arg;
  auto* tsfn = new (std::nothrow) napi_threadsafe_function__();
  if (tsfn == nullptr) return napi_generic_failure;
  tsfn->env = env;
  tsfn->call_js_cb = call_js_cb;
  tsfn->finalize_cb = thread_finalize_cb;
  tsfn->finalize_data = thread_finalize_data;
  tsfn->context = context;
  tsfn->refcount.store(static_cast<uint32_t>(initial_thread_count == 0 ? 1 : initial_thread_count));
  *result = tsfn;
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_threadsafe_function_context(
    napi_threadsafe_function func, void** result) {
  if (func == nullptr || result == nullptr) return napi_invalid_arg;
  *result = func->context;
  return napi_ok;
}

napi_status NAPI_CDECL napi_call_threadsafe_function(
    napi_threadsafe_function func, void* data, napi_threadsafe_function_call_mode is_blocking) {
  (void)data;
  (void)is_blocking;
  if (func == nullptr) return napi_invalid_arg;
  return napi_ok;
}

napi_status NAPI_CDECL napi_acquire_threadsafe_function(napi_threadsafe_function func) {
  if (func == nullptr) return napi_invalid_arg;
  func->refcount.fetch_add(1);
  return napi_ok;
}

napi_status NAPI_CDECL napi_release_threadsafe_function(
    napi_threadsafe_function func, napi_threadsafe_function_release_mode mode) {
  (void)mode;
  if (func == nullptr) return napi_invalid_arg;
  uint32_t current = func->refcount.load();
  if (current > 0) func->refcount.fetch_sub(1);
  return napi_ok;
}

napi_status NAPI_CDECL napi_unref_threadsafe_function(
    node_api_basic_env env, napi_threadsafe_function func) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || func == nullptr) return napi_invalid_arg;
  return napi_ok;
}

napi_status NAPI_CDECL napi_ref_threadsafe_function(
    node_api_basic_env env, napi_threadsafe_function func) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || func == nullptr) return napi_invalid_arg;
  return napi_ok;
}

napi_status NAPI_CDECL napi_add_async_cleanup_hook(
    node_api_basic_env env,
    napi_async_cleanup_hook hook,
    void* arg,
    napi_async_cleanup_hook_handle* remove_handle) {
  auto* napiEnv = const_cast<napi_env>(env);
  if (!CheckEnv(napiEnv) || hook == nullptr) return napi_invalid_arg;
  (void)EdgeEnsureEnvLoop(napiEnv, nullptr);
  if (auto* environment = GetAttachedEnvironment(napiEnv); environment != nullptr) {
    environment->set_async_cleanup_hook_registered(true);
  } else {
    auto& state = GetOrCreateDetachedEnvState(napiEnv);
    if (!state.async_cleanup_hook_registered) {
      napi_status status =
          napi_add_env_cleanup_hook(napiEnv, RunAsyncCleanupHooksOnEnvTeardown, napiEnv);
      if (status != napi_ok) return status;
      state.async_cleanup_hook_registered = true;
    }
  }

  auto* handle = new (std::nothrow) napi_async_cleanup_hook_handle__();
  if (handle == nullptr) return napi_generic_failure;
  handle->env = napiEnv;
  handle->hook = hook;
  handle->arg = arg;

  if (auto* environment = GetAttachedEnvironment(napiEnv); environment != nullptr) {
    environment->AddAsyncCleanupHook(handle);
  } else {
    GetOrCreateDetachedEnvState(napiEnv).async_cleanup_hooks.push_back(handle);
  }
  if (remove_handle != nullptr) *remove_handle = handle;
  return napi_ok;
}

napi_status NAPI_CDECL napi_remove_async_cleanup_hook(
    napi_async_cleanup_hook_handle remove_handle) {
  if (remove_handle == nullptr || remove_handle->env == nullptr) return napi_invalid_arg;
  if (remove_handle->removed) return napi_invalid_arg;
  remove_handle->removed = true;

  auto* env = remove_handle->env;
  bool hooks_empty = false;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    if (!environment->RemoveAsyncCleanupHook(remove_handle)) return napi_invalid_arg;
    hooks_empty = environment->async_cleanup_hooks().empty();
  } else {
    auto* state = GetDetachedEnvState(env);
    if (state == nullptr) return napi_invalid_arg;
    auto& hooks = state->async_cleanup_hooks;
    for (auto it = hooks.begin(); it != hooks.end(); ++it) {
      if (*it == remove_handle) {
        hooks.erase(it);
        break;
      }
    }
    hooks_empty = hooks.empty();
    if (hooks_empty && state->async_cleanup_hook_registered) {
      napi_remove_env_cleanup_hook(env, RunAsyncCleanupHooksOnEnvTeardown, env);
      state->async_cleanup_hook_registered = false;
      MaybeEraseDetachedEnvState(env);
    }
  }
  delete remove_handle;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr && hooks_empty) {
    environment->set_async_cleanup_hook_registered(false);
  }
  return napi_ok;
}

napi_status NAPI_CDECL napi_get_uv_event_loop(node_api_basic_env env, uv_loop_t** loop) {
  return EdgeEnsureEnvLoop(const_cast<napi_env>(env), loop);
}

napi_status NAPI_CDECL napi_async_init(napi_env env,
                                       napi_value async_resource,
                                       napi_value async_resource_name,
                                       napi_async_context* result) {
  return CreateAsyncContext(env, async_resource, async_resource_name, result);
}

napi_status NAPI_CDECL napi_async_destroy(napi_env env,
                                          napi_async_context async_context) {
  if (!CheckEnv(env) || async_context == nullptr || async_context->env != env) {
    return napi_invalid_arg;
  }
  DestroyAsyncContext(async_context);
  delete async_context;
  return napi_ok;
}

napi_status NAPI_CDECL napi_make_callback(napi_env env,
                                         napi_async_context async_context,
                                         napi_value recv,
                                         napi_value func,
                                         size_t argc,
                                         const napi_value* argv,
                                         napi_value* result) {
  if (!CheckEnv(env) || recv == nullptr || func == nullptr || (argc > 0 && argv == nullptr)) {
    return napi_invalid_arg;
  }
  if (async_context == nullptr) {
    return EdgeCallCallbackWithDomain(
        env, recv, func, argc, const_cast<napi_value*>(argv), result);
  }

  const napi_status scope_status = EnterAsyncContextScope(env, async_context);
  if (scope_status != napi_ok) return scope_status;

  napi_status call_status =
      EdgeCallCallbackWithDomain(env, recv, func, argc, const_cast<napi_value*>(argv), result);
  const bool failed = (call_status != napi_ok) || HasPendingException(env);
  ExitAsyncContextScope(env, async_context, failed);
  return call_status;
}

napi_status NAPI_CDECL napi_open_callback_scope(napi_env env,
                                                napi_value resource_object,
                                                napi_async_context context,
                                                napi_callback_scope* result) {
  (void)resource_object;
  if (!CheckEnv(env) || context == nullptr || result == nullptr) return napi_invalid_arg;
  auto* scope = new (std::nothrow) napi_callback_scope__();
  if (scope == nullptr) return napi_generic_failure;
  scope->env = env;
  scope->async_context = context;
  const napi_status status = EnterAsyncContextScope(env, context);
  if (status != napi_ok) {
    delete scope;
    return status;
  }
  scope->entered = true;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    environment->IncrementOpenCallbackScopes();
  } else {
    GetOrCreateDetachedEnvState(env).open_callback_scopes++;
  }
  *result = scope;
  return napi_ok;
}

napi_status NAPI_CDECL napi_close_callback_scope(napi_env env, napi_callback_scope scope) {
  if (!CheckEnv(env) || scope == nullptr) return napi_invalid_arg;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    if (environment->open_callback_scopes() == 0) {
      return napi_callback_scope_mismatch;
    }
  } else {
    auto* state = GetDetachedEnvState(env);
    if (state == nullptr || state->open_callback_scopes == 0) {
      return napi_callback_scope_mismatch;
    }
  }
  if (scope->entered) {
    ExitAsyncContextScope(env, scope->async_context, HasPendingException(env));
  }
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    environment->DecrementOpenCallbackScopes();
  } else {
    auto* state = GetDetachedEnvState(env);
    if (state == nullptr) {
      delete scope;
      return napi_callback_scope_mismatch;
    }
    state->open_callback_scopes--;
    MaybeEraseDetachedEnvState(env);
  }
  delete scope;
  return napi_ok;
}

}  // extern "C"

napi_status EdgeEnsureEnvLoop(napi_env env, uv_loop_t** loop_out) {
  if (!CheckEnv(env)) return napi_invalid_arg;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    return environment->EnsureEventLoop(loop_out);
  }

  auto& state = GetOrCreateDetachedEnvState(env);
  if (!state.loop_cleanup_hook_registered) {
    if (napi_add_env_cleanup_hook(env, CleanupEnvLoopOnTeardown, env) != napi_ok) {
      return napi_generic_failure;
    }
    state.loop_cleanup_hook_registered = true;
  }
  if (state.loop == nullptr) {
    auto* loop = new (std::nothrow) uv_loop_t();
    if (loop == nullptr) return napi_generic_failure;
    if (uv_loop_init(loop) != 0) {
      delete loop;
      return napi_generic_failure;
    }
    (void)uv_loop_configure(loop, UV_METRICS_IDLE_TIME);
    state.loop = loop;
  }
  if (loop_out != nullptr) *loop_out = state.loop;
  return napi_ok;
}

uv_loop_t* EdgeGetEnvLoop(napi_env env) {
  uv_loop_t* loop = nullptr;
  return EdgeEnsureEnvLoop(env, &loop) == napi_ok ? loop : nullptr;
}

uv_loop_t* EdgeGetExistingEnvLoop(napi_env env) {
  if (!CheckEnv(env)) return nullptr;
  if (auto* environment = GetAttachedEnvironment(env); environment != nullptr) {
    return environment->GetExistingEventLoop();
  }
  auto* state = GetDetachedEnvState(env);
  return state != nullptr ? state->loop : nullptr;
}
