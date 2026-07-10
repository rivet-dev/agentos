#include "internal_binding/binding_initializers.h"

#include "internal_binding/helpers.h"

#include <string>

namespace internal_binding {

namespace {

napi_value MakeUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value HeapUtilsReadStart(napi_env env, napi_callback_info /*info*/) {
  napi_value zero = nullptr;
  napi_create_int32(env, 0, &zero);
  return zero != nullptr ? zero : MakeUndefined(env);
}

napi_value HeapUtilsCreateHeapSnapshotStream(napi_env env, napi_callback_info /*info*/) {
  napi_value handle = nullptr;
  if (napi_create_object(env, &handle) != napi_ok || handle == nullptr) {
    return MakeUndefined(env);
  }

  napi_value read_start = nullptr;
  if (napi_create_function(env,
                           "readStart",
                           NAPI_AUTO_LENGTH,
                           HeapUtilsReadStart,
                           nullptr,
                           &read_start) == napi_ok &&
      read_start != nullptr) {
    napi_set_named_property(env, handle, "readStart", read_start);
  }
  return handle;
}

napi_value HeapUtilsTriggerHeapSnapshot(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, argv[0], &t) == napi_ok && t == napi_string) {
      return argv[0];
    }
  }

  napi_value out = nullptr;
  napi_create_string_utf8(env, "HeapSnapshot.heapsnapshot", NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : MakeUndefined(env);
}

}  // namespace

napi_value InitHeapUtils(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  napi_value create_stream = nullptr;
  if (napi_create_function(env,
                           "createHeapSnapshotStream",
                           NAPI_AUTO_LENGTH,
                           HeapUtilsCreateHeapSnapshotStream,
                           nullptr,
                           &create_stream) == napi_ok &&
      create_stream != nullptr) {
    napi_set_named_property(env, out, "createHeapSnapshotStream", create_stream);
  }

  napi_value trigger_snapshot = nullptr;
  if (napi_create_function(env,
                           "triggerHeapSnapshot",
                           NAPI_AUTO_LENGTH,
                           HeapUtilsTriggerHeapSnapshot,
                           nullptr,
                           &trigger_snapshot) == napi_ok &&
      trigger_snapshot != nullptr) {
    napi_set_named_property(env, out, "triggerHeapSnapshot", trigger_snapshot);
  }

  return out;
}

}  // namespace internal_binding

