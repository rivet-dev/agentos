#ifndef UNOFFICIAL_NAPI_H_
#define UNOFFICIAL_NAPI_H_

#include <stdint.h>

#include "js_native_api.h"

#ifdef __cplusplus
extern "C" {
#endif

struct uv_loop_s;

// Unofficial/test-only helper APIs for creating and releasing an env scope.
typedef struct {
  size_t max_young_generation_size_in_bytes;
  size_t max_old_generation_size_in_bytes;
  size_t code_range_size_in_bytes;
  void* stack_limit;
} unofficial_napi_env_create_options;

typedef struct {
  uint64_t total_memory;
  uint64_t constrained_memory;
} unofficial_napi_embedder_memory_info;

typedef napi_status (*unofficial_napi_embedder_memory_info_callback)(
    void* target,
    unofficial_napi_embedder_memory_info* info_out);
typedef napi_status (*unofficial_napi_embedder_shutdown_pump_callback)(
    void* target,
    void* handle);

typedef struct {
  unofficial_napi_embedder_memory_info_callback memory_info_callback;
  void* memory_info_target;
  unofficial_napi_embedder_shutdown_pump_callback shutdown_pump_callback;
  void* shutdown_pump_target;
} unofficial_napi_embedder_hooks;

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_create_env(int32_t module_api_version,
                                                   napi_env* env_out,
                                                   void** scope_out);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_create_env_with_options(
    int32_t module_api_version,
    const unofficial_napi_env_create_options* options,
    napi_env* env_out,
    void** scope_out);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_embedder_hooks(
    const unofficial_napi_embedder_hooks* hooks);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_edge_environment(napi_env env, void* environment);
NAPI_EXTENSION_WASMER_EXTERN void* unofficial_napi_get_edge_environment(napi_env env);
using unofficial_napi_env_cleanup_callback = void (*)(napi_env env, void* data);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_env_cleanup_callback(
    napi_env env,
    unofficial_napi_env_cleanup_callback callback,
    void* data);
using unofficial_napi_env_destroy_callback = void (*)(napi_env env, void* data);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_env_destroy_callback(
    napi_env env,
    unofficial_napi_env_destroy_callback callback,
    void* data);
using unofficial_napi_context_token_callback = void (*)(napi_env env, void* token, void* data);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_context_token_callbacks(
    napi_env env,
    unofficial_napi_context_token_callback assign_callback,
    unofficial_napi_context_token_callback unassign_callback,
    void* data);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_destroy_env_instance(napi_env env);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_release_env(void* scope);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_release_env_with_loop(
    void* scope,
    struct uv_loop_s* loop);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_low_memory_notification(napi_env env);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_flags_from_string(
    const char* flags,
    size_t length);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_prepare_stack_trace_callback(
    napi_env env,
    napi_value callback);

// Unofficial/test-only helper. Requests a full GC cycle for testing.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_request_gc_for_testing(napi_env env);

// Unofficial/test-only helper. Runs a checkpoint on the current context's
// microtask queue.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_process_microtasks(napi_env env);

// Unofficial helper. Terminates current JS execution in the env's engine.
// This is used for worker-style shutdown semantics where the process must
// survive but the current env must stop executing JS immediately.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_terminate_execution(napi_env env);
// Clears a previously requested engine termination on the current env. This is
// used when embedder code intentionally stops a worker but still needs the
// current JS stack to unwind normally.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_cancel_terminate_execution(napi_env env);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_pending_exception(napi_env env,
                                                              napi_value error);

using unofficial_napi_interrupt_callback = void (*)(napi_env env, void* data);

// Unofficial helper. Requests execution of a callback on the target env's
// engine thread at the next interrupt point. The callback runs entered into
// that env's isolate/context.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_request_interrupt(
    napi_env env,
    unofficial_napi_interrupt_callback callback,
    void* data);

using unofficial_napi_foreground_task_callback = void (*)(napi_env env, void* data);
using unofficial_napi_foreground_task_cleanup = void (*)(napi_env env, void* data);
using unofficial_napi_enqueue_foreground_task_callback =
    napi_status (*)(void* target,
                    unofficial_napi_foreground_task_callback callback,
                    void* data,
                    unofficial_napi_foreground_task_cleanup cleanup,
                    uint64_t delay_millis);

// Installs the embedder-owned foreground task queue hook for a single env.
// Engine backends use this to forward engine-originated foreground work into
// the embedder-owned main-thread queue. Queue ownership and drain policy stay
// outside the backend.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_enqueue_foreground_task_callback(
    napi_env env,
    unofficial_napi_enqueue_foreground_task_callback callback,
    void* target);

// Unofficial helper. Enqueues a JS function into V8 microtask queue.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_enqueue_microtask(napi_env env, napi_value callback);

// Unofficial helper. Sets the per-env PromiseReject callback used by
// internal/process/promises via internalBinding('task_queue').
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_promise_reject_callback(napi_env env,
                                                                    napi_value callback);

// Unofficial helper. Sets the per-env Promise lifecycle hooks used by
// internal/promise_hooks via internalBinding('async_wrap').
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_promise_hooks(napi_env env,
                                                          napi_value init,
                                                          napi_value before,
                                                          napi_value after,
                                                          napi_value resolve);

using unofficial_napi_fatal_error_callback =
    void (*)(napi_env env, const char* location, const char* message);
using unofficial_napi_oom_error_callback =
    void (*)(napi_env env, const char* location, bool is_heap_oom, const char* detail);
using unofficial_napi_near_heap_limit_callback =
    size_t (*)(napi_env env, void* data, size_t current_heap_limit, size_t initial_heap_limit);

// Unofficial helpers for embedder-native fatal/OOM handling.
// These callbacks run from the engine's fatal error hooks.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_fatal_error_callbacks(
    napi_env env,
    unofficial_napi_fatal_error_callback fatal_callback,
    unofficial_napi_oom_error_callback oom_callback);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_near_heap_limit_callback(
    napi_env env,
    unofficial_napi_near_heap_limit_callback callback,
    void* data);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_remove_near_heap_limit_callback(
    napi_env env,
    size_t heap_limit);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_stack_limit(napi_env env, void* stack_limit);

// Unofficial helpers used by util/options parity work in edge.
// These expose engine-specific data that is not available in the public N-API.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_promise_details(napi_env env,
                                                            napi_value promise,
                                                            int32_t* state_out,
                                                            napi_value* result_out,
                                                            bool* has_result_out);

typedef struct {
  napi_value source_line;
  napi_value script_resource_name;
  int32_t line_number;
  int32_t start_column;
  int32_t end_column;
} unofficial_napi_error_source_positions;

// Unofficial helpers for Node-style exception/message parity.
// These expose engine message/source metadata that is not available in the
// public Node-API.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_error_source_positions(
    napi_env env,
    napi_value error,
    unofficial_napi_error_source_positions* out);

// Preserve the current engine-generated source arrow/message for an Error
// object so later rethrows do not overwrite it with the rethrow callsite.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_preserve_error_source_message(
    napi_env env,
    napi_value error);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_source_maps_enabled(
    napi_env env,
    bool enabled);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_get_source_map_error_source_callback(
    napi_env env,
    napi_value callback);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_error_source_line_for_stderr(
    napi_env env,
    napi_value error,
    napi_value* result_out);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_error_thrown_at(
    napi_env env,
    napi_value error,
    napi_value* result_out);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_take_preserved_error_formatting(
    napi_env env,
    napi_value error,
    napi_value* source_line_out,
    napi_value* thrown_at_out);

// Unofficial helper used by module_wrap parity paths to tell the runtime's
// PromiseReject callback machinery that a rejected promise is being handled
// synchronously, matching Node's native ThrowIfPromiseRejected() helper.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_mark_promise_as_handled(
    napi_env env,
    napi_value promise);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_proxy_details(napi_env env,
                                                          napi_value proxy,
                                                          napi_value* target_out,
                                                          napi_value* handler_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_preview_entries(napi_env env,
                                                        napi_value value,
                                                        napi_value* entries_out,
                                                        bool* is_key_value_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_call_sites(napi_env env,
                                                       uint32_t frames,
                                                       napi_value* callsites_out);
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_current_stack_trace(napi_env env,
                                                                uint32_t frames,
                                                                napi_value* callsites_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_caller_location(napi_env env,
                                                            napi_value* location_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_arraybuffer_view_has_buffer(napi_env env,
                                                                    napi_value value,
                                                                    bool* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_constructor_name(napi_env env,
                                                             napi_value value,
                                                             napi_value* name_out);

// Unofficial helper for Node's internalBinding('util').getOwnNonIndexProperties.
// Returns the target's own property names while skipping indexed elements at the
// engine level, matching Node's use of IndexFilter::kSkipIndices.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_own_non_index_properties(
    napi_env env,
    napi_value value,
    uint32_t filter_bits,
    napi_value* result_out);

// Unofficial helper for Node's internalBinding('util').privateSymbols.
// Returns a JS-visible private symbol value backed by the engine's hidden
// private property machinery.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_create_private_symbol(napi_env env,
                                                              const char* utf8description,
                                                              size_t length,
                                                              napi_value* result_out);

// Unofficial helper for internalBinding('messaging').structuredClone().
// This mirrors the engine's structured clone path closely enough to preserve
// SharedArrayBuffer backing stores during clone/deserialization.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_structured_clone(
    napi_env env,
    napi_value value,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_structured_clone_with_transfer(
    napi_env env,
    napi_value value,
    napi_value transfer_list,
    napi_value* result_out);

// Unofficial helpers for env-agnostic message payload queues.
// The returned opaque payload must be released with
// unofficial_napi_release_serialized_value().
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_serialize_value(
    napi_env env,
    napi_value value,
    void** payload_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_deserialize_value(
    napi_env env,
    void* payload,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN void unofficial_napi_release_serialized_value(void* payload);

// Unofficial helper for Node-style process.memoryUsage() parity.
// Returns V8 heap statistics plus allocator-tracked ArrayBuffer memory.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_process_memory_info(
    napi_env env,
    double* heap_total_out,
    double* heap_used_out,
    double* external_out,
    double* array_buffers_out);

// Unofficial helper for Node's internalBinding('v8').getHashSeed().
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_hash_seed(napi_env env,
                                                      uint64_t* hash_seed_out);

#define UNOFFICIAL_NAPI_HEAP_SPACE_NAME_MAX_LENGTH 64

typedef struct {
  uint64_t total_heap_size;
  uint64_t total_heap_size_executable;
  uint64_t total_physical_size;
  uint64_t total_available_size;
  uint64_t used_heap_size;
  uint64_t heap_size_limit;
  uint64_t does_zap_garbage;
  uint64_t malloced_memory;
  uint64_t peak_malloced_memory;
  uint64_t number_of_native_contexts;
  uint64_t number_of_detached_contexts;
  uint64_t total_global_handles_size;
  uint64_t used_global_handles_size;
  uint64_t external_memory;
} unofficial_napi_heap_statistics;

typedef struct {
  char space_name[UNOFFICIAL_NAPI_HEAP_SPACE_NAME_MAX_LENGTH];
  uint64_t space_size;
  uint64_t space_used_size;
  uint64_t space_available_size;
  uint64_t physical_space_size;
} unofficial_napi_heap_space_statistics;

typedef struct {
  uint64_t code_and_metadata_size;
  uint64_t bytecode_and_metadata_size;
  uint64_t external_script_source_size;
  uint64_t cpu_profiler_metadata_size;
} unofficial_napi_heap_code_statistics;

typedef enum {
  unofficial_napi_cpu_profile_start_ok = 0,
  unofficial_napi_cpu_profile_start_too_many = 1,
} unofficial_napi_cpu_profile_start_result;

typedef struct {
  bool expose_internals;
  bool expose_numeric_values;
} unofficial_napi_heap_snapshot_options;

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_heap_statistics(
    napi_env env,
    unofficial_napi_heap_statistics* stats_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_heap_space_count(
    napi_env env,
    uint32_t* count_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_heap_space_statistics(
    napi_env env,
    uint32_t space_index,
    unofficial_napi_heap_space_statistics* stats_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_heap_code_statistics(
    napi_env env,
    unofficial_napi_heap_code_statistics* stats_out);

// Unofficial helpers for worker-thread profiling/snapshot support.
// These must be called on the target env's engine thread, typically from
// unofficial_napi_request_interrupt().
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_start_cpu_profile(
    napi_env env,
    unofficial_napi_cpu_profile_start_result* result_out,
    uint32_t* profile_id_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_stop_cpu_profile(
    napi_env env,
    uint32_t profile_id,
    bool* found_out,
    char** json_out,
    size_t* json_len_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_start_heap_profile(
    napi_env env,
    bool* started_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_stop_heap_profile(
    napi_env env,
    bool* found_out,
    char** json_out,
    size_t* json_len_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_take_heap_snapshot(
    napi_env env,
    const unofficial_napi_heap_snapshot_options* options,
    char** json_out,
    size_t* json_len_out);

NAPI_EXTENSION_WASMER_EXTERN void unofficial_napi_free_buffer(void* data);

// Unofficial helpers for Node's async_context_frame parity. These expose the
// engine continuation-preserved embedder data used by AsyncContextFrame.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_get_continuation_preserved_embedder_data(
    napi_env env,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_set_continuation_preserved_embedder_data(
    napi_env env,
    napi_value value);

// Unofficial helper. Refreshes V8 date/timezone configuration after TZ changes.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_notify_datetime_configuration_change(napi_env env);

// Unofficial helper. Creates the native internalBinding('serdes') object
// containing Serializer and Deserializer constructors.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_create_serdes_binding(napi_env env,
                                                              napi_value* result_out);

// Unofficial helpers for implementing internalBinding('contextify') on embedders.
// These are engine-specific APIs and are not part of the public Node-API.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_make_context(
    napi_env env,
    napi_value sandbox_or_symbol,
    napi_value name,
    napi_value origin_or_undefined,
    bool allow_code_gen_strings,
    bool allow_code_gen_wasm,
    bool own_microtask_queue,
    napi_value host_defined_option_id,
    napi_value* result_out);

// A JS source for compile/eval APIs: exactly one of `text` (a JS string) or
// `bytecode` (an opaque handle from unofficial_napi_bytecode_compile /
// unofficial_napi_bytecode_deserialize) is set.
typedef struct unofficial_napi_js_source {
  napi_value text;
  void* bytecode;
} unofficial_napi_js_source;

// Compile shape of a bytecode artifact; bytecode is only usable by APIs that
// compile the same shape.
typedef enum {
  unofficial_napi_bytecode_shape_script = 0,        // whole-script eval
  unofficial_napi_bytecode_shape_cjs_function = 1,  // function compiled with params
  unofficial_napi_bytecode_shape_module = 2,        // ES module
} unofficial_napi_bytecode_shape;

// Eagerly compiles source text into bytecode. The returned handle holds the
// live compiled artifact plus serializable engine bytes, and retains the
// text/filename/shape/params (V8 code caches are consumed against the
// original source). On compile FAILURE with a non-module shape,
// *can_parse_as_module_out (nullable) reports whether the source parses as an
// ES module — the detect-module signal — while the compile exception stays
// pending.
// host_defined_option_id (Symbol or nullish) is baked into the compiled
// artifact's origin so consumers with the same symbol can reuse it directly;
// a consumer with different options re-consumes the serialized bytes instead.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_bytecode_compile(
    napi_env env,
    napi_value source_text,
    napi_value filename,
    int32_t shape,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    int32_t line_offset,
    int32_t column_offset,
    void** bytecode_out,
    bool* can_parse_as_module_out);

// Deserializes engine bytes (e.g. a sidecar payload) into a live compiled
// artifact. source_text is the source the bytes were produced from. On
// stale/incompatible bytes the call succeeds with *bytecode_out = NULL and
// *rejected_out = true; the caller falls back to compiling the text.
// Payloads are fully self-validating against (shape, source_text, params,
// filename) — V8 natively via CachedData; QuickJS via the provider's QJSB
// header written at serialize time (which also guards payload integrity,
// since JS_ReadObject is not hardened). A stored filename hash of 0 is
// "unenforced" (vm.SourceTextModule#createCachedData writes 0 because Node
// numbers vm module identifiers). So untrusted cachedData can be handed
// straight in: a mismatch on any of those dimensions is reported as rejected
// without the caller needing its own validation wrapper.

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_bytecode_deserialize(
    napi_env env,
    const uint8_t* bytes,
    size_t byte_length,
    napi_value source_text,
    napi_value filename,
    int32_t shape,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    void** bytecode_out,
    bool* rejected_out);

// Engine bytes for persistence, as a Uint8Array (V8: raw CachedData;
// QuickJS: self-validating QJSB header [shape, source, params, filename,
// payload hashes] + JS_WriteObject output).
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_bytecode_serialize(
    napi_env env,
    void* bytecode,
    napi_value* buffer_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_bytecode_release(
    napi_env env,
    void* bytecode);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_run_script(
    napi_env env,
    napi_value sandbox_or_null,
    const unofficial_napi_js_source* source,
    napi_value filename,
    int32_t line_offset,
    int32_t column_offset,
    int64_t timeout,
    bool display_errors,
    bool break_on_sigint,
    bool break_on_first_line,
    napi_value host_defined_option_id,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_dispose_context(
    napi_env env,
    napi_value sandbox_or_context_global);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_compile_function(
    napi_env env,
    const unofficial_napi_js_source* source,
    napi_value filename,
    int32_t line_offset,
    int32_t column_offset,
    napi_value parsing_context_or_undefined,
    napi_value context_extensions_or_undefined,
    napi_value params_or_undefined,
    napi_value host_defined_option_id,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_compile_function_for_cjs_loader(
    napi_env env,
    const unofficial_napi_js_source* source,
    napi_value filename,
    bool is_sea_main,
    bool should_detect_module,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_contains_module_syntax(
    napi_env env,
    napi_value code,
    napi_value filename,
    napi_value resource_name_or_undefined,
    bool cjs_var_in_scope,
    bool* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_start_sigint_watchdog(
    napi_env env,
    bool* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_stop_sigint_watchdog(
    napi_env env,
    bool* had_pending_signal_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_contextify_watchdog_has_pending_sigint(
    napi_env env,
    bool* result_out);

// Unofficial helpers for implementing internalBinding('module_wrap') on embedders.
// These keep V8 module objects behind an opaque native handle so bindings stay N-API only.
NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_create_source_text(
    napi_env env,
    napi_value wrapper,
    napi_value url,
    napi_value context_or_undefined,
    const unofficial_napi_js_source* source,
    int32_t line_offset,
    int32_t column_offset,
    napi_value host_defined_option_id,
    void** handle_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_create_synthetic(
    napi_env env,
    napi_value wrapper,
    napi_value url,
    napi_value context_or_undefined,
    napi_value export_names,
    napi_value synthetic_eval_steps,
    void** handle_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_destroy(
    napi_env env,
    void* handle);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_get_module_requests(
    napi_env env,
    void* handle,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_link(
    napi_env env,
    void* handle,
    size_t count,
    void* const* linked_handles);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_instantiate(
    napi_env env,
    void* handle);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_evaluate(
    napi_env env,
    void* handle,
    int64_t timeout,
    bool break_on_sigint,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_evaluate_sync(
    napi_env env,
    void* handle,
    napi_value filename,
    napi_value parent_filename,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_get_namespace(
    napi_env env,
    void* handle,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_get_status(
    napi_env env,
    void* handle,
    int32_t* status_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_get_error(
    napi_env env,
    void* handle,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_has_top_level_await(
    napi_env env,
    void* handle,
    bool* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_has_async_graph(
    napi_env env,
    void* handle,
    bool* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_check_unsettled_top_level_await(
    napi_env env,
    napi_value module_wrap,
    bool warnings,
    bool* settled_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_set_export(
    napi_env env,
    void* handle,
    napi_value export_name,
    napi_value export_value);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_set_module_source_object(
    napi_env env,
    void* handle,
    napi_value source_object);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_get_module_source_object(
    napi_env env,
    void* handle,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_create_cached_data(
    napi_env env,
    void* handle,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_set_import_module_dynamically_callback(
    napi_env env,
    napi_value callback);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_set_initialize_import_meta_object_callback(
    napi_env env,
    napi_value callback);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_import_module_dynamically(
    napi_env env,
    size_t argc,
    napi_value* argv,
    napi_value* result_out);

NAPI_EXTENSION_WASMER_EXTERN napi_status unofficial_napi_module_wrap_create_required_module_facade(
    napi_env env,
    void* handle,
    napi_value* result_out);

#ifdef __cplusplus
}  // extern "C"
#endif

#endif  // UNOFFICIAL_NAPI_H_
