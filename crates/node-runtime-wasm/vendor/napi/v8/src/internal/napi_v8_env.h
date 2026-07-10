#ifndef NAPI_V8_ENV_H_
#define NAPI_V8_ENV_H_

#include <cstring>
#include <cstdint>
#include <string>
#include <unordered_set>
#include <utility>
#include <vector>

#include <v8.h>

#include "js_native_api.h"
#include "napi_error_state.h"
#include "napi_periodic_gate.h"
#include "internal/napi_factory.h"
#include "internal/napi_lifetime_tracker.h"
#include "internal/napi_ref_tracker.h"
#include "unofficial_napi.h"

typedef void(NAPI_CDECL* napi_cleanup_hook)(void* arg);

struct napi_buffer_record__;
struct napi_callback_payload__;
struct napi_deferred__;
struct napi_env__;
struct napi_env_cleanup_hook__;
struct napi_escapable_handle_scope__;
struct napi_external_backing_store_hint__;
struct napi_external_wrapper__;
struct napi_handle_scope__;
struct napi_ref__;
struct napi_ref_with_data__;
struct napi_ref_with_finalizer__;
static_assert(sizeof(v8::Local<v8::Value>) == sizeof(napi_value),
              "Cannot convert between v8::Local<v8::Value> and napi_value");

inline napi_value JsValueFromV8LocalValue(v8::Local<v8::Value> local) {
  return reinterpret_cast<napi_value>(*local);
}

inline v8::Local<v8::Value> V8LocalValueFromJsValue(napi_value value) {
  v8::Local<v8::Value> local;
  std::memcpy(static_cast<void*>(&local), &value, sizeof(value));
  return local;
}

struct napi_callback_info__ {
  virtual ~napi_callback_info__() = default;
  virtual size_t argc() const = 0;
  virtual void args(napi_value* argv, size_t argc) const = 0;
  virtual napi_value this_arg() const = 0;
  virtual napi_value new_target() const = 0;
  virtual void* data() const = 0;
};

struct napi_env__ {
  explicit napi_env__(v8::Local<v8::Context> context, int32_t module_api_version);
  ~napi_env__();

  v8::Local<v8::Context> context() const;
  void CallFinalizer(node_api_basic_finalize cb, void* data, void* hint);
  void InvokeFinalizerFromGC(napi_ref_tracker__* finalizer);
  void EnqueueFinalizer(napi_ref_tracker__* finalizer);
  void EnqueueBufferFinalizer(napi_buffer_record__* record);
  void DequeueFinalizer(napi_ref_tracker__* finalizer);
  void DrainFinalizerQueue();

  template <typename T, typename... Args>
  T* allocate(Args&&... args) {
    return factory_.allocate<T>(std::forward<Args>(args)...);
  }

  template <typename T>
  void release(T* value) {
    factory_.release<T>(value);
  }

#if defined(NAPI_ENABLE_LIFETIME_TRACKER) && defined(NAPI_ENABLE_LIFETIME_PERIODIC_STATS)
  bool should_dump_lifetime_stats(int64_t now_ms);
  bool should_dump_lifetime_string_symbol_values(int64_t now_ms);
#endif

  v8::Isolate* isolate = nullptr;
  v8::Global<v8::Context> context_ref;
  napi::error_state__ error_state;
  std::vector<void*> open_handle_scope_stack;
  v8::Global<v8::Value> last_exception;
  v8::Global<v8::Message> last_exception_message;
  std::string last_exception_source_line;
  std::string last_exception_thrown_at;
  v8::Global<v8::Private> wrap_private_key;
  v8::Global<v8::Private> buffer_private_key;
  v8::Global<v8::Private> type_tag_private_key;
  void* instance_data = nullptr;
  napi_finalize instance_data_finalize_cb = nullptr;
  void* instance_data_finalize_hint = nullptr;
  void* edge_environment = nullptr;
  std::vector<napi_env_cleanup_hook__*> env_cleanup_hooks;
  uint64_t env_cleanup_hook_counter = 0;
  std::vector<napi_buffer_record__*> buffer_records;
  std::unordered_set<napi_external_backing_store_hint__*> external_backing_store_hints;
  napi_ref_tracker__::RefList reflist;
  napi_ref_tracker__::RefList finalizing_reflist;
  std::unordered_set<napi_ref_tracker__*> pending_finalizers;
  std::unordered_set<napi_buffer_record__*> pending_buffer_finalizers;
  bool finalization_scheduled = false;
  unofficial_napi_env_cleanup_callback env_cleanup_callback = nullptr;
  void* env_cleanup_callback_data = nullptr;
  unofficial_napi_env_destroy_callback env_destroy_callback = nullptr;
  void* env_destroy_callback_data = nullptr;
  unofficial_napi_context_token_callback context_token_assign_callback = nullptr;
  unofficial_napi_context_token_callback context_token_unassign_callback = nullptr;
  void* context_token_callback_data = nullptr;
  unofficial_napi_enqueue_foreground_task_callback enqueue_foreground_task_callback = nullptr;
  void* enqueue_foreground_task_target = nullptr;

#if defined(NAPI_ENABLE_LIFETIME_TRACKER) && defined(NAPI_ENABLE_LIFETIME_PERIODIC_STATS)
  napi::periodic_gate__ lifetime_stats_gate_{2000};
  napi::periodic_gate__ lifetime_string_symbol_values_gate_{10000};
#endif

 private:
  v8impl::detail::napi_factory__ factory_;
};

napi_status napi_v8_set_last_error(napi_env env,
                                   napi_status status,
                                   const char* message);

napi_status napi_v8_clear_last_error(napi_env env);

napi_value napi_v8_wrap_value(napi_env env, v8::Local<v8::Value> value);
v8::Local<v8::Value> napi_v8_unwrap_value(napi_value value);
void napi_v8_finalize_buffer_records(napi_env env);

#endif  // NAPI_V8_ENV_H_
