#include "edge_async_wrap.h"

#include <vector>

#include "binding_registry/binding_registry.h"
#include "edge_environment.h"
#include "internal_binding/binding_async_wrap.h"
#include "unofficial_napi.h"

#include "internal_binding/helpers.h"
#include "edge_module_loader.h"
#include "edge_runtime.h"

namespace {

void ResetRef(napi_env env, napi_ref* ref);

struct AsyncWrapCache {
  explicit AsyncWrapCache(napi_env env_in) : env(env_in) {}
  ~AsyncWrapCache() {
    for (auto& entry : context_frame_refs) {
      ResetRef(env, &entry.second);
    }
    context_frame_refs.clear();
    ResetRef(env, &binding_ref);
    ResetRef(env, &async_id_fields_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref async_id_fields_ref = nullptr;
  std::unordered_map<int64_t, napi_ref> context_frame_refs;
};

void ResetRef(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (env == nullptr || ref == nullptr) return nullptr;
  napi_value value = nullptr;
  if (napi_get_reference_value(env, ref, &value) != napi_ok || value == nullptr) {
    return nullptr;
  }
  return value;
}

napi_value ResolveInternalBinding(napi_env env, const char* name) {
  if (env == nullptr || name == nullptr) return nullptr;
  return edge::binding_registry::Get(env, name);
}

AsyncWrapCache& GetCache(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<AsyncWrapCache>(
      env, kEdgeEnvironmentSlotAsyncWrapCache);
}

bool GetPerIsolateSymbol(napi_env env, const char* name, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (env == nullptr || name == nullptr) return false;
  napi_value symbols = EdgeGetPerIsolateSymbols(env);
  return symbols != nullptr &&
         napi_get_named_property(env, symbols, name, out) == napi_ok &&
         *out != nullptr;
}

bool SetResourceAsyncIds(napi_env env, napi_value resource, int64_t async_id, int64_t trigger_async_id) {
  if (env == nullptr || resource == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, resource, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return false;
  }

  napi_value async_id_symbol = nullptr;
  napi_value trigger_async_id_symbol = nullptr;
  if (!GetPerIsolateSymbol(env, "async_id_symbol", &async_id_symbol) ||
      !GetPerIsolateSymbol(env, "trigger_async_id_symbol", &trigger_async_id_symbol)) {
    return false;
  }

  napi_value async_id_value = nullptr;
  napi_value trigger_async_id_value = nullptr;
  if (napi_create_int64(env, async_id, &async_id_value) != napi_ok ||
      napi_create_int64(env, trigger_async_id, &trigger_async_id_value) != napi_ok ||
      async_id_value == nullptr ||
      trigger_async_id_value == nullptr) {
    return false;
  }

  napi_property_descriptor descriptors[2] = {
      napi_property_descriptor{
          nullptr,
          async_id_symbol,
          nullptr,
          nullptr,
          nullptr,
          async_id_value,
          napi_default,
          nullptr,
      },
      napi_property_descriptor{
          nullptr,
          trigger_async_id_symbol,
          nullptr,
          nullptr,
          nullptr,
          trigger_async_id_value,
          napi_default,
          nullptr,
      },
  };
  return napi_define_properties(env, resource, 2, descriptors) == napi_ok;
}

int64_t GetResourceAsyncId(napi_env env, napi_value resource, const char* symbol_name) {
  if (env == nullptr || resource == nullptr || symbol_name == nullptr) return 0;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, resource, &type) != napi_ok || (type != napi_object && type != napi_function)) {
    return 0;
  }

  napi_value symbol = nullptr;
  napi_value value = nullptr;
  int64_t out = 0;
  if (!GetPerIsolateSymbol(env, symbol_name, &symbol) ||
      napi_get_property(env, resource, symbol, &value) != napi_ok ||
      value == nullptr ||
      napi_get_value_int64(env, value, &out) != napi_ok) {
    return 0;
  }
  return out;
}

napi_value GetCurrentAsyncContextFrame(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value value = nullptr;
  if (unofficial_napi_get_continuation_preserved_embedder_data(env, &value) != napi_ok) {
    return nullptr;
  }
  return value;
}

void SetCurrentAsyncContextFrame(napi_env env, napi_value value) {
  if (env == nullptr) return;
  if (value == nullptr) {
    napi_get_undefined(env, &value);
  }
  (void)unofficial_napi_set_continuation_preserved_embedder_data(env, value);
}

void StoreAsyncContextFrame(napi_env env, int64_t async_id) {
  if (env == nullptr || async_id <= 0) return;
  AsyncWrapCache& cache = GetCache(env);
  auto it = cache.context_frame_refs.find(async_id);
  if (it != cache.context_frame_refs.end()) {
    ResetRef(env, &it->second);
    cache.context_frame_refs.erase(it);
  }

  napi_value frame = GetCurrentAsyncContextFrame(env);
  if (frame == nullptr) return;
  napi_ref ref = nullptr;
  if (napi_create_reference(env, frame, 1, &ref) != napi_ok || ref == nullptr) return;
  cache.context_frame_refs.emplace(async_id, ref);
}

void DeleteStoredAsyncContextFrame(napi_env env, int64_t async_id) {
  if (env == nullptr || async_id <= 0) return;
  AsyncWrapCache& cache = GetCache(env);
  auto it = cache.context_frame_refs.find(async_id);
  if (it == cache.context_frame_refs.end()) return;
  ResetRef(env, &it->second);
  cache.context_frame_refs.erase(it);
}

napi_value GetStoredAsyncContextFrame(napi_env env, int64_t async_id) {
  if (env == nullptr || async_id <= 0) return nullptr;
  AsyncWrapCache& cache = GetCache(env);
  auto it = cache.context_frame_refs.find(async_id);
  if (it == cache.context_frame_refs.end()) return nullptr;
  return GetRefValue(env, it->second);
}

napi_value GetAsyncWrapBinding(napi_env env) {
  AsyncWrapCache& cache = GetCache(env);
  napi_value binding = GetRefValue(env, cache.binding_ref);
  if (binding != nullptr) return binding;

  binding = ResolveInternalBinding(env, "async_wrap");
  if (binding == nullptr) return nullptr;

  if (cache.binding_ref != nullptr) {
    napi_delete_reference(env, cache.binding_ref);
    cache.binding_ref = nullptr;
  }
  napi_create_reference(env, binding, 1, &cache.binding_ref);
  return binding;
}

double* GetAsyncIdFields(napi_env env) {
  AsyncWrapCache& cache = GetCache(env);
  napi_value fields = GetRefValue(env, cache.async_id_fields_ref);
  if (fields == nullptr) {
    napi_value binding = GetAsyncWrapBinding(env);
    if (binding == nullptr) return nullptr;
    if (napi_get_named_property(env, binding, "async_id_fields", &fields) != napi_ok ||
        fields == nullptr) {
      return nullptr;
    }
    if (cache.async_id_fields_ref != nullptr) {
      napi_delete_reference(env, cache.async_id_fields_ref);
      cache.async_id_fields_ref = nullptr;
    }
    napi_create_reference(env, fields, 1, &cache.async_id_fields_ref);
  }

  bool is_typedarray = false;
  if (napi_is_typedarray(env, fields, &is_typedarray) != napi_ok || !is_typedarray) {
    return nullptr;
  }

  napi_typedarray_type type = napi_uint8_array;
  size_t length = 0;
  void* data = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env,
                               fields,
                               &type,
                               &length,
                               &data,
                               &arraybuffer,
                               &byte_offset) != napi_ok ||
      data == nullptr ||
      type != napi_float64_array ||
      length < 4) {
    return nullptr;
  }

  return static_cast<double*>(data);
}

}  // namespace

int64_t EdgeAsyncWrapNextId(napi_env env) {
  double* fields = GetAsyncIdFields(env);
  if (fields == nullptr) return 1;

  constexpr size_t kAsyncIdCounter = 2;
  fields[kAsyncIdCounter] += 1;
  return static_cast<int64_t>(fields[kAsyncIdCounter]);
}

int64_t EdgeAsyncWrapExecutionAsyncId(napi_env env) {
  double* fields = GetAsyncIdFields(env);
  if (fields == nullptr) return 1;

  constexpr size_t kExecutionAsyncId = 0;
  constexpr size_t kDefaultTriggerAsyncId = 3;
  const int64_t execution_async_id = static_cast<int64_t>(fields[kExecutionAsyncId]);
  if (execution_async_id > 0) return execution_async_id;
  const int64_t default_trigger_async_id = static_cast<int64_t>(fields[kDefaultTriggerAsyncId]);
  return default_trigger_async_id > 0 ? default_trigger_async_id : 1;
}

int64_t EdgeAsyncWrapCurrentExecutionAsyncId(napi_env env) {
  double* fields = GetAsyncIdFields(env);
  if (fields == nullptr) return 0;

  constexpr size_t kExecutionAsyncId = 0;
  return static_cast<int64_t>(fields[kExecutionAsyncId]);
}

const char* EdgeAsyncWrapProviderName(int32_t provider_type) {
  switch (provider_type) {
    case kEdgeProviderHttpClientRequest:
      return "HTTPCLIENTREQUEST";
    case kEdgeProviderHttpIncomingMessage:
      return "HTTPINCOMINGMESSAGE";
    case kEdgeProviderJsStream:
      return "JSSTREAM";
    case kEdgeProviderJsUdpWrap:
      return "JSUDPWRAP";
    case kEdgeProviderMessagePort:
      return "MESSAGEPORT";
    case kEdgeProviderWorker:
      return "WORKER";
    case kEdgeProviderPipeConnectWrap:
      return "PIPECONNECTWRAP";
    case kEdgeProviderPipeServerWrap:
      return "PIPESERVERWRAP";
    case kEdgeProviderPipeWrap:
      return "PIPEWRAP";
    case kEdgeProviderProcessWrap:
      return "PROCESSWRAP";
    case kEdgeProviderShutdownWrap:
      return "SHUTDOWNWRAP";
    case kEdgeProviderTcpConnectWrap:
      return "TCPCONNECTWRAP";
    case kEdgeProviderTcpServerWrap:
      return "TCPSERVERWRAP";
    case kEdgeProviderTcpWrap:
      return "TCPWRAP";
    case kEdgeProviderTtyWrap:
      return "TTYWRAP";
    case kEdgeProviderUdpSendWrap:
      return "UDPSENDWRAP";
    case kEdgeProviderUdpWrap:
      return "UDPWRAP";
    case kEdgeProviderWriteWrap:
      return "WRITEWRAP";
    case kEdgeProviderZlib:
      return "ZLIB";
    case kEdgeProviderTlsWrap:
      return "TLSWRAP";
    default:
      return "NONE";
  }
}

void CallHookWithAsyncId(napi_env env, napi_value hooks, const char* hook_name, int64_t async_id) {
  if (env == nullptr || hooks == nullptr || hook_name == nullptr || async_id <= 0) return;
  napi_value hook_fn = nullptr;
  if (napi_get_named_property(env, hooks, hook_name, &hook_fn) != napi_ok || hook_fn == nullptr) {
    return;
  }

  napi_valuetype fn_type = napi_undefined;
  if (napi_typeof(env, hook_fn, &fn_type) != napi_ok || fn_type != napi_function) return;

  napi_value async_id_v = nullptr;
  napi_create_int64(env, async_id, &async_id_v);
  napi_value ignored = nullptr;
  if (napi_call_function(env, hooks, hook_fn, 1, &async_id_v, &ignored) != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value ignored_error = nullptr;
      napi_get_and_clear_last_exception(env, &ignored_error);
    }
  }
}

void EdgeAsyncWrapEmitInitString(napi_env env,
                                int64_t async_id,
                                const char* type,
                                int64_t trigger_async_id,
                                napi_value resource) {
  if (env == nullptr || async_id <= 0 || type == nullptr) return;
  StoreAsyncContextFrame(env, async_id);
  if (resource != nullptr) {
    (void)SetResourceAsyncIds(env, resource, async_id, trigger_async_id);
  }
  napi_value hooks = internal_binding::AsyncWrapGetHooksObject(env);
  if (hooks == nullptr) return;

  napi_value init_fn = nullptr;
  if (napi_get_named_property(env, hooks, "init", &init_fn) != napi_ok || init_fn == nullptr) {
    return;
  }

  napi_valuetype fn_type = napi_undefined;
  if (napi_typeof(env, init_fn, &fn_type) != napi_ok || fn_type != napi_function) return;

  napi_value async_id_v = nullptr;
  napi_value type_v = nullptr;
  napi_value trigger_async_id_v = nullptr;
  napi_value promise_hook_v = nullptr;
  napi_create_int64(env, async_id, &async_id_v);
  napi_create_string_utf8(env, type, NAPI_AUTO_LENGTH, &type_v);
  napi_create_int64(env, trigger_async_id, &trigger_async_id_v);
  napi_get_boolean(env, false, &promise_hook_v);
  napi_value argv[5] = {
      async_id_v,
      type_v,
      trigger_async_id_v,
      resource != nullptr ? resource : internal_binding::Undefined(env),
      promise_hook_v,
  };
  napi_value ignored = nullptr;
  if (napi_call_function(env, hooks, init_fn, 5, argv, &ignored) != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(env, &pending) == napi_ok && pending) {
      napi_value ignored_error = nullptr;
      napi_get_and_clear_last_exception(env, &ignored_error);
    }
  }
}

void EdgeAsyncWrapEmitInit(napi_env env,
                          int64_t async_id,
                          int32_t provider_type,
                          int64_t trigger_async_id,
                          napi_value resource) {
  EdgeAsyncWrapEmitInitString(
      env, async_id, EdgeAsyncWrapProviderName(provider_type), trigger_async_id, resource);
}

void EdgeAsyncWrapEmitBefore(napi_env env, int64_t async_id) {
  napi_value hooks = internal_binding::AsyncWrapGetHooksObject(env);
  CallHookWithAsyncId(env, hooks, "before", async_id);
}

void EdgeAsyncWrapEmitAfter(napi_env env, int64_t async_id) {
  napi_value hooks = internal_binding::AsyncWrapGetHooksObject(env);
  CallHookWithAsyncId(env, hooks, "after", async_id);
}

void EdgeAsyncWrapPushContext(napi_env env,
                             int64_t async_id,
                             int64_t trigger_async_id,
                             napi_value resource) {
  if (env == nullptr) return;
  internal_binding::AsyncWrapPushContext(
      env, static_cast<double>(async_id), static_cast<double>(trigger_async_id), resource);
}

bool EdgeAsyncWrapPopContext(napi_env env, int64_t async_id) {
  if (env == nullptr) return false;
  return internal_binding::AsyncWrapPopContext(env, static_cast<double>(async_id));
}

napi_status EdgeAsyncWrapMakeCallback(napi_env env,
                                     int64_t async_id,
                                     napi_value resource,
                                     napi_value recv,
                                     napi_value callback,
                                     size_t argc,
                                     napi_value* argv,
                                     napi_value* result,
                                     int flags) {
  if (env == nullptr || recv == nullptr || callback == nullptr) return napi_invalid_arg;
  napi_value effective_resource = resource != nullptr ? resource : recv;
  bool pushed_async_context = false;
  napi_value prior_context_frame = nullptr;
  bool swapped_context_frame = false;
  if (async_id > 0 && effective_resource != nullptr) {
    int64_t trigger_async_id = GetResourceAsyncId(env, effective_resource, "trigger_async_id_symbol");
    if (trigger_async_id <= 0) trigger_async_id = EdgeAsyncWrapExecutionAsyncId(env);
    EdgeAsyncWrapPushContext(env, async_id, trigger_async_id, effective_resource);
    pushed_async_context = true;
  }
  if (async_id > 0) {
    prior_context_frame = GetCurrentAsyncContextFrame(env);
    SetCurrentAsyncContextFrame(env, GetStoredAsyncContextFrame(env, async_id));
    swapped_context_frame = true;
  }

  napi_value trampoline = internal_binding::AsyncWrapGetCallbackTrampoline(env);
  napi_status status = napi_ok;
  if (trampoline == nullptr) {
    status = EdgeMakeCallbackWithFlags(env, recv, callback, argc, argv, result, flags);
    if (swapped_context_frame) SetCurrentAsyncContextFrame(env, prior_context_frame);
    if (pushed_async_context) EdgeAsyncWrapPopContext(env, async_id);
    return status;
  }

  napi_valuetype trampoline_type = napi_undefined;
  if (napi_typeof(env, trampoline, &trampoline_type) != napi_ok || trampoline_type != napi_function) {
    status = EdgeMakeCallbackWithFlags(env, recv, callback, argc, argv, result, flags);
    if (swapped_context_frame) SetCurrentAsyncContextFrame(env, prior_context_frame);
    if (pushed_async_context) EdgeAsyncWrapPopContext(env, async_id);
    return status;
  }

  napi_value async_id_v = nullptr;
  napi_create_int64(env, async_id, &async_id_v);
  std::vector<napi_value> trampoline_argv;
  trampoline_argv.reserve(argc + 3);
  trampoline_argv.push_back(async_id_v);
  trampoline_argv.push_back(resource != nullptr ? resource : recv);
  trampoline_argv.push_back(callback);
  for (size_t i = 0; i < argc; ++i) {
    trampoline_argv.push_back(argv != nullptr ? argv[i] : internal_binding::Undefined(env));
  }
  status = EdgeMakeCallbackWithFlags(
      env, recv, trampoline, trampoline_argv.size(), trampoline_argv.data(), result, flags);
  if (swapped_context_frame) SetCurrentAsyncContextFrame(env, prior_context_frame);
  if (pushed_async_context) EdgeAsyncWrapPopContext(env, async_id);
  return status;
}

void EdgeAsyncWrapQueueDestroyId(napi_env env, int64_t async_id) {
  if (env == nullptr || async_id <= 0) return;
  DeleteStoredAsyncContextFrame(env, async_id);
  internal_binding::AsyncWrapQueueDestroyId(env, static_cast<double>(async_id));
}

void EdgeAsyncWrapReset(napi_env env, int64_t* async_id) {
  if (async_id == nullptr) return;
  if (*async_id > 0) EdgeAsyncWrapQueueDestroyId(env, *async_id);
  *async_id = EdgeAsyncWrapNextId(env);
}
