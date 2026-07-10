#ifndef NAPI_V8_ENV_RECORDS_H_
#define NAPI_V8_ENV_RECORDS_H_

#include <cstdint>
#include <memory>

#include <v8.h>

#include "js_native_api.h"
#include "internal/napi_escapable_handle_scope_wrapper.h"
#include "internal/napi_handle_scope_wrapper.h"
#include "node_api_types.h"

typedef void(NAPI_CDECL* napi_cleanup_hook)(void* arg);

struct napi_env__;

struct napi_env_cleanup_hook__ {
  explicit napi_env_cleanup_hook__(napi_env env) : env(env) {}

  napi_env env = nullptr;
  napi_cleanup_hook hook = nullptr;
  void* arg = nullptr;
  uint64_t order = 0;
};

struct napi_deferred__ {
  explicit napi_deferred__(napi_env env) : env(env) {}

  napi_env env = nullptr;
  v8::Global<v8::Promise::Resolver> resolver;
};

struct napi_handle_scope__ {
  explicit napi_handle_scope__(napi_env env);

  napi_env env = nullptr;
  napi_handle_scope_wrapper__ wrapper;
};

struct napi_escapable_handle_scope__ {
  explicit napi_escapable_handle_scope__(napi_env env);

  napi_env env = nullptr;
  napi_escapable_handle_scope_wrapper__ wrapper;
};

struct napi_buffer_record__ {
  explicit napi_buffer_record__(napi_env env) : env(env) {}

  napi_env env = nullptr;
  v8::Global<v8::Object> holder;
  std::shared_ptr<v8::BackingStore> backing_store;
  void* external_data = nullptr;
  node_api_basic_finalize finalize_cb = nullptr;
  void* finalize_hint = nullptr;
  bool finalized = false;
};
using napi_buffer_record = napi_buffer_record__;

struct napi_external_backing_store_hint__ {
  explicit napi_external_backing_store_hint__(napi_env env) : env(env) {}

  napi_env env = nullptr;
  void* external_data = nullptr;
  node_api_basic_finalize finalize_cb = nullptr;
  void* finalize_hint = nullptr;
};
using napi_external_backing_store_hint = napi_external_backing_store_hint__;

#endif  // NAPI_V8_ENV_RECORDS_H_
