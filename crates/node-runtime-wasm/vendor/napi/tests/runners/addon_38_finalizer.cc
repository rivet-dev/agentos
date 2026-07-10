#include <js_native_api.h>

#include "common.h"

namespace {

int finalizer_call_count = 0;

struct JsFinalizerData {
  napi_ref callback = nullptr;
};

void CountFinalizer(node_api_basic_env env, void* data, void* hint) {
  (void)env;
  (void)data;
  (void)hint;
  finalizer_call_count++;
}

void JsFinalizer(node_api_basic_env basic_env, void* data, void* hint) {
  (void)hint;
  finalizer_call_count++;

  napi_env env = const_cast<napi_env>(basic_env);
  auto* finalizer_data = static_cast<JsFinalizerData*>(data);
  napi_value callback;
  napi_value recv;
  if (napi_get_reference_value(env, finalizer_data->callback, &callback) == napi_ok &&
      callback != nullptr &&
      napi_get_undefined(env, &recv) == napi_ok) {
    napi_call_function(env, recv, callback, 0, nullptr, nullptr);
  }
  napi_delete_reference(env, finalizer_data->callback);
  delete finalizer_data;
}

napi_value AddFinalizer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_add_finalizer(env, argv[0], nullptr, CountFinalizer, nullptr, nullptr));
  return nullptr;
}

napi_value AddFinalizerWithJS(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 2, "Function expects two arguments");

  auto* data = new JsFinalizerData();
  napi_status status = napi_create_reference(env, argv[1], 1, &data->callback);
  if (status != napi_ok) {
    delete data;
    NODE_API_CALL(env, status);
  }

  status = napi_add_finalizer(env, argv[0], data, JsFinalizer, nullptr, nullptr);
  if (status != napi_ok) {
    napi_delete_reference(env, data->callback);
    delete data;
    NODE_API_CALL(env, status);
  }

  return nullptr;
}

napi_value GetFinalizerCallCount(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value result;
  NODE_API_CALL(env, napi_create_int32(env, finalizer_call_count, &result));
  return result;
}

}  // namespace

extern "C" napi_value Init(napi_env env, napi_value exports) {
  napi_property_descriptor descriptors[] = {
      DECLARE_NODE_API_PROPERTY("addFinalizer", AddFinalizer),
      DECLARE_NODE_API_PROPERTY("addFinalizerWithJS", AddFinalizerWithJS),
      DECLARE_NODE_API_PROPERTY("getFinalizerCallCount", GetFinalizerCallCount),
  };

  NODE_API_CALL(
      env,
      napi_define_properties(env,
                             exports,
                             sizeof(descriptors) / sizeof(*descriptors),
                             descriptors));
  return exports;
}
