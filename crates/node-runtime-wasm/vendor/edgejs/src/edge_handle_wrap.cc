#include "edge_handle_wrap.h"

#include <mutex>
#include <vector>

#include "binding_registry/binding_registry.h"
#include "internal_binding/helpers.h"
#include "edge_active_resource.h"
#include "edge_environment.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_module_loader.h"
#include "edge_runtime.h"

namespace {

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct HandleSymbolCache {
  explicit HandleSymbolCache(napi_env env_in) : env(env_in) {}
  ~HandleSymbolCache() {
    DeleteRefIfPresent(env, &symbols_ref);
    DeleteRefIfPresent(env, &owner_symbol_ref);
    DeleteRefIfPresent(env, &handle_onclose_symbol_ref);
  }

  napi_env env = nullptr;
  napi_ref symbols_ref = nullptr;
  napi_ref owner_symbol_ref = nullptr;
  napi_ref handle_onclose_symbol_ref = nullptr;
};

struct HandleWrapEnvState {
  explicit HandleWrapEnvState(napi_env /*env_in*/) {}

  EdgeHandleWrap* head = nullptr;
  bool cleanup_started = false;
};

std::mutex g_handle_wrap_env_states_mutex;

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

HandleSymbolCache& GetHandleSymbolCache(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<HandleSymbolCache>(
      env, kEdgeEnvironmentSlotHandleSymbolCache);
}

HandleWrapEnvState* GetHandleWrapState(napi_env env) {
  return EdgeEnvironmentGetSlotData<HandleWrapEnvState>(
      env, kEdgeEnvironmentSlotHandleWrapEnvState);
}

HandleWrapEnvState& EnsureHandleWrapState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<HandleWrapEnvState>(
      env, kEdgeEnvironmentSlotHandleWrapEnvState);
}

void UnlinkHandleWrapLocked(HandleWrapEnvState* state, EdgeHandleWrap* wrap) {
  if (state == nullptr || wrap == nullptr || !wrap->attached) return;
  if (wrap->prev != nullptr) {
    wrap->prev->next = wrap->next;
  } else if (state->head == wrap) {
    state->head = wrap->next;
  }
  if (wrap->next != nullptr) {
    wrap->next->prev = wrap->prev;
  }
  wrap->prev = nullptr;
  wrap->next = nullptr;
  wrap->attached = false;
}

void RunHandleWrapEnvCleanup(napi_env env) {
  if (env == nullptr) return;
  std::vector<EdgeHandleWrap*> wraps_to_close;
  {
    std::lock_guard<std::mutex> lock(g_handle_wrap_env_states_mutex);
    auto* state = GetHandleWrapState(env);
    if (state == nullptr) return;
    state->cleanup_started = true;
    for (EdgeHandleWrap* wrap = state->head; wrap != nullptr; wrap = wrap->next) {
      wraps_to_close.push_back(wrap);
    }
  }

  for (EdgeHandleWrap* wrap : wraps_to_close) {
    if (wrap == nullptr ||
        !wrap->attached ||
        wrap->state != kEdgeHandleInitialized ||
        wrap->close_callback == nullptr) {
      continue;
    }
    wrap->close_callback(wrap->close_data);
  }

  uv_loop_t* loop = EdgeGetExistingEnvLoop(env);
  while (loop != nullptr) {
    bool empty = false;
    {
      std::lock_guard<std::mutex> lock(g_handle_wrap_env_states_mutex);
      auto* state = GetHandleWrapState(env);
      empty = state == nullptr || state->head == nullptr;
    }
    if (empty) break;
    (void)uv_run(loop, UV_RUN_ONCE);
  }
}

napi_value ResolveInternalBinding(napi_env env, const char* name) {
  if (env == nullptr || name == nullptr) return nullptr;
  return edge::binding_registry::Get(env, name);
}

napi_value GetSymbolsBinding(napi_env env) {
  HandleSymbolCache& cache = GetHandleSymbolCache(env);
  napi_value binding = EdgeHandleWrapGetRefValue(env, cache.symbols_ref);
  if (binding != nullptr) return binding;

  binding = ResolveInternalBinding(env, "symbols");
  if (binding == nullptr) return nullptr;

  EdgeHandleWrapDeleteRefIfPresent(env, &cache.symbols_ref);
  napi_create_reference(env, binding, 1, &cache.symbols_ref);
  return binding;
}

napi_value GetNamedCachedSymbol(napi_env env, const char* key, napi_ref* slot) {
  if (slot == nullptr) return nullptr;
  napi_value symbol = EdgeHandleWrapGetRefValue(env, *slot);
  if (symbol != nullptr) return symbol;

  napi_value symbols = GetSymbolsBinding(env);
  if (symbols == nullptr) return nullptr;
  if (napi_get_named_property(env, symbols, key, &symbol) != napi_ok || symbol == nullptr) {
    return nullptr;
  }

  EdgeHandleWrapDeleteRefIfPresent(env, slot);
  napi_create_reference(env, symbol, 1, slot);
  return symbol;
}

napi_value GetOwnerSymbol(napi_env env) {
  HandleSymbolCache& cache = GetHandleSymbolCache(env);
  return GetNamedCachedSymbol(env, "owner_symbol", &cache.owner_symbol_ref);
}

napi_value GetHandleOnCloseSymbol(napi_env env) {
  HandleSymbolCache& cache = GetHandleSymbolCache(env);
  return GetNamedCachedSymbol(env, "handle_onclose", &cache.handle_onclose_symbol_ref);
}

void SetPropertyIfPresent(napi_env env, napi_value obj, napi_value key, napi_value value) {
  if (env == nullptr || obj == nullptr || key == nullptr || value == nullptr) return;
  napi_set_property(env, obj, key, value);
}

}  // namespace

void EdgeHandleWrapInit(EdgeHandleWrap* wrap, napi_env env) {
  if (wrap == nullptr) return;
  wrap->env = env;
  wrap->wrapper_ref = nullptr;
  wrap->active_handle_token = nullptr;
  wrap->close_data = nullptr;
  wrap->uv_handle = nullptr;
  wrap->close_callback = nullptr;
  wrap->prev = nullptr;
  wrap->next = nullptr;
  wrap->attached = false;
  wrap->finalized = false;
  wrap->delete_on_close = false;
  wrap->wrapper_ref_held = false;
  wrap->in_close_callback = false;
  wrap->state = kEdgeHandleUninitialized;
}

void EdgeHandleWrapAttach(EdgeHandleWrap* wrap,
                         void* close_data,
                         uv_handle_t* handle,
                         EdgeHandleWrapCloseCallback close_callback) {
  if (wrap == nullptr || wrap->env == nullptr || handle == nullptr || close_callback == nullptr) return;
  std::lock_guard<std::mutex> lock(g_handle_wrap_env_states_mutex);
  auto& state = EnsureHandleWrapState(wrap->env);
  if (state.cleanup_started) return;
  if (wrap->attached) {
    UnlinkHandleWrapLocked(&state, wrap);
  }
  wrap->close_data = close_data;
  wrap->uv_handle = handle;
  wrap->close_callback = close_callback;
  wrap->prev = nullptr;
  wrap->next = state.head;
  if (state.head != nullptr) {
    state.head->prev = wrap;
  }
  state.head = wrap;
  wrap->attached = true;
}

void EdgeHandleWrapDetach(EdgeHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || !wrap->attached) return;
  std::lock_guard<std::mutex> lock(g_handle_wrap_env_states_mutex);
  auto* state = GetHandleWrapState(wrap->env);
  if (state == nullptr) {
    wrap->close_data = nullptr;
    wrap->uv_handle = nullptr;
    wrap->close_callback = nullptr;
    wrap->prev = nullptr;
    wrap->next = nullptr;
    wrap->attached = false;
    return;
  }
  UnlinkHandleWrapLocked(state, wrap);
  wrap->close_data = nullptr;
  wrap->uv_handle = nullptr;
  wrap->close_callback = nullptr;
}

napi_value EdgeHandleWrapGetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

void EdgeHandleWrapDeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void EdgeHandleWrapHoldWrapperRef(EdgeHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr || wrap->wrapper_ref_held) return;
  uint32_t ref_count = 0;
  if (napi_reference_ref(wrap->env, wrap->wrapper_ref, &ref_count) == napi_ok) {
    wrap->wrapper_ref_held = true;
  }
}

void EdgeHandleWrapReleaseWrapperRef(EdgeHandleWrap* wrap) {
  if (wrap == nullptr || wrap->env == nullptr || wrap->wrapper_ref == nullptr || !wrap->wrapper_ref_held) return;
  uint32_t ref_count = 0;
  if (napi_reference_unref(wrap->env, wrap->wrapper_ref, &ref_count) == napi_ok) {
    wrap->wrapper_ref_held = false;
  }
}

bool EdgeHandleWrapCancelFinalizer(EdgeHandleWrap* wrap, void* native_object) {
  if (wrap == nullptr ||
      wrap->env == nullptr ||
      wrap->wrapper_ref == nullptr ||
      native_object == nullptr ||
      wrap->finalized) {
    return false;
  }

  napi_value self = EdgeHandleWrapGetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return false;

  void* removed = nullptr;
  if (napi_remove_wrap(wrap->env, self, &removed) != napi_ok || removed != native_object) {
    return false;
  }

  EdgeHandleWrapDeleteRefIfPresent(wrap->env, &wrap->wrapper_ref);
  wrap->wrapper_ref_held = false;
  return true;
}

napi_value EdgeHandleWrapGetActiveOwner(napi_env env, napi_ref wrapper_ref) {
  napi_value wrapper = EdgeHandleWrapGetRefValue(env, wrapper_ref);
  if (wrapper == nullptr) return nullptr;

  napi_value owner_symbol = GetOwnerSymbol(env);
  if (owner_symbol != nullptr) {
    napi_value owner = nullptr;
    if (napi_get_property(env, wrapper, owner_symbol, &owner) == napi_ok && owner != nullptr) {
      napi_valuetype type = napi_undefined;
      if (napi_typeof(env, owner, &type) == napi_ok && type != napi_undefined && type != napi_null) {
        return owner;
      }
    }
  }
  return wrapper;
}

void EdgeHandleWrapSetOnCloseCallback(napi_env env, napi_value wrapper, napi_value callback) {
  if (env == nullptr || wrapper == nullptr || callback == nullptr) return;
  napi_value symbol = GetHandleOnCloseSymbol(env);
  if (symbol == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, callback, &type) != napi_ok || type != napi_function) return;
  SetPropertyIfPresent(env, wrapper, symbol, callback);
}

void EdgeHandleWrapMaybeCallOnClose(EdgeHandleWrap* wrap) {
  if (wrap == nullptr ||
      wrap->env == nullptr ||
      wrap->finalized ||
      EdgeHandleWrapEnvCleanupStarted(wrap->env)) {
    return;
  }
  edge::HandleScope scope(wrap->env);
  if (!scope.is_open()) return;
  napi_value self = EdgeHandleWrapGetRefValue(wrap->env, wrap->wrapper_ref);
  if (self == nullptr) return;

  napi_value symbol = GetHandleOnCloseSymbol(wrap->env);
  if (symbol == nullptr) return;

  bool has_callback = false;
  if (napi_has_property(wrap->env, self, symbol, &has_callback) != napi_ok || !has_callback) {
    return;
  }

  napi_value callback = nullptr;
  if (napi_get_property(wrap->env, self, symbol, &callback) != napi_ok || callback == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(wrap->env, callback, &type) != napi_ok || type != napi_function) return;

  napi_value ignored = nullptr;
  EdgeMakeCallback(wrap->env, self, callback, 0, nullptr, &ignored);

  napi_value undefined = nullptr;
  napi_get_undefined(wrap->env, &undefined);
  SetPropertyIfPresent(wrap->env, self, symbol, undefined);
}

bool EdgeHandleWrapHasRef(const EdgeHandleWrap* wrap, const uv_handle_t* handle) {
  if (wrap == nullptr || handle == nullptr || wrap->state != kEdgeHandleInitialized) return false;
  return uv_has_ref(handle) != 0;
}

bool EdgeHandleWrapEnvCleanupStarted(napi_env env) {
  return EdgeEnvironmentCleanupStarted(env);
}

namespace {

void UnregisterActiveHandleIfPresent(napi_env env, EdgeHandleWrap* wrap) {
  if (wrap->active_handle_token == nullptr) return;
  if (env != nullptr) EdgeUnregisterActiveHandle(env, wrap->active_handle_token);
  wrap->active_handle_token = nullptr;
}

}  // namespace

void EdgeHandleWrapCompleteClose(EdgeHandleWrap* wrap, void* native,
                                 EdgeHandleWrapDeleteFn delete_native) {
  if (wrap == nullptr || native == nullptr || delete_native == nullptr) return;

  // Claim ownership of deletion for the entire teardown. Detaching, unregistering
  // the active handle, and running the JS "onclose" callback can each re-enter
  // the GC and run this wrap's finalizer; while in_close_callback is set the
  // finalizer only records finalization and defers freeing to us, so the wrap is
  // never freed while this teardown (or the onclose callback) is on the stack.
  wrap->in_close_callback = true;

  wrap->state = kEdgeHandleClosed;
  EdgeHandleWrapDetach(wrap);
  UnregisterActiveHandleIfPresent(wrap->env, wrap);
  EdgeHandleWrapMaybeCallOnClose(wrap);

  wrap->in_close_callback = false;

  bool can_delete = wrap->finalized;
  if (!can_delete && wrap->delete_on_close) {
    can_delete = EdgeHandleWrapCancelFinalizer(wrap, native);
  }
  if (can_delete) {
    EdgeHandleWrapDeleteRefIfPresent(wrap->env, &wrap->wrapper_ref);
    delete_native(native);
  } else {
    // The handle is closed but the JS wrapper is still reachable; drop the
    // strong pin so the wrapper becomes collectable. The finalizer will free
    // the native object when the wrapper is eventually collected.
    EdgeHandleWrapReleaseWrapperRef(wrap);
  }
}

void EdgeHandleWrapFinalizeNative(napi_env env, EdgeHandleWrap* wrap, void* native,
                                  EdgeHandleWrapDeleteFn delete_native) {
  if (wrap == nullptr || native == nullptr || delete_native == nullptr) return;

  wrap->finalized = true;
  EdgeHandleWrapDeleteRefIfPresent(env, &wrap->wrapper_ref);

  // If a close callback is currently tearing this wrap down (this finalizer ran
  // re-entrantly via the onclose callback), that close path owns the deletion.
  if (wrap->in_close_callback) return;

  if (wrap->state == kEdgeHandleInitialized) {
    // The wrapper was collected while the handle is still open. Drop the pin and
    // start closing it; OnClose (-> EdgeHandleWrapCompleteClose) frees the wrap.
    wrap->delete_on_close = true;
    EdgeHandleWrapReleaseWrapperRef(wrap);
    if (wrap->close_callback != nullptr) wrap->close_callback(wrap->close_data);
    return;
  }
  if (wrap->state == kEdgeHandleClosing) {
    // A close is already in flight; let its OnClose free the wrap.
    wrap->delete_on_close = true;
    return;
  }

  // Uninitialized or already closed: free now.
  EdgeHandleWrapDetach(wrap);
  UnregisterActiveHandleIfPresent(env, wrap);
  delete_native(native);
}

void EdgeHandleWrapRunEnvCleanup(napi_env env) {
  (void)env;
}
