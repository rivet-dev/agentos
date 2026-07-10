#include <js_native_api.h>

#include "common.h"

namespace {

bool deref_item_was_called = false;
bool finalize_was_called = false;
int wrapped_item = 0;
int finalize_item = 0;

napi_value BooleanResult(napi_env env, bool value) {
  napi_value result;
  NODE_API_CALL(env, napi_get_boolean(env, value, &result));
  return result;
}

napi_value StringResult(napi_env env, const char* value) {
  napi_value result;
  NODE_API_CALL(env, napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &result));
  return result;
}

void DerefItem(node_api_basic_env env, void* data, void* hint) {
  (void)env;
  (void)data;
  (void)hint;
  deref_item_was_called = true;
}

void FinalizeWrap(node_api_basic_env env, void* data, void* hint) {
  (void)env;
  (void)data;
  (void)hint;
  finalize_was_called = true;
}

napi_value TestStrictEquals(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  bool result;
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 2, "Function expects two arguments");
  NODE_API_CALL(env, napi_strict_equals(env, argv[0], argv[1], &result));
  return BooleanResult(env, result);
}

napi_value TestSetPrototype(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 2, "Function expects two arguments");
  NODE_API_CALL(env, node_api_set_prototype(env, argv[0], argv[1]));
  return nullptr;
}

napi_value TestGetPrototype(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_value result;
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_get_prototype(env, argv[0], &result));
  return result;
}

napi_value TestGetVersion(napi_env env, napi_callback_info info) {
  (void)info;
  uint32_t version;
  napi_value result;
  NODE_API_CALL(env, napi_get_version(env, &version));
  NODE_API_CALL(env, napi_create_uint32(env, version, &result));
  return result;
}

napi_value TestNapiTypeof(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_valuetype type;
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_typeof(env, argv[0], &type));

  switch (type) {
    case napi_undefined:
      return StringResult(env, "undefined");
    case napi_null:
      return StringResult(env, "null");
    case napi_boolean:
      return StringResult(env, "boolean");
    case napi_number:
      return StringResult(env, "number");
    case napi_string:
      return StringResult(env, "string");
    case napi_symbol:
      return StringResult(env, "symbol");
    case napi_function:
      return StringResult(env, "function");
    case napi_bigint:
      return StringResult(env, "bigint");
    case napi_external:
    case napi_object:
    default:
      return StringResult(env, "object");
  }
}

napi_value Wrap(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_wrap(env, argv[0], &wrapped_item, DerefItem, nullptr, nullptr));
  return nullptr;
}

napi_value RemoveWrap(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  void* data = nullptr;
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_remove_wrap(env, argv[0], &data));
  return nullptr;
}

napi_value DerefItemWasCalled(napi_env env, napi_callback_info info) {
  (void)info;
  return BooleanResult(env, deref_item_was_called);
}

napi_value TestFinalizeWrap(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr));
  NODE_API_ASSERT(env, argc == 1, "Function expects one argument");
  NODE_API_CALL(env, napi_wrap(env, argv[0], &finalize_item, FinalizeWrap, nullptr, nullptr));
  return nullptr;
}

napi_value FinalizeWasCalled(napi_env env, napi_callback_info info) {
  (void)info;
  return BooleanResult(env, finalize_was_called);
}

napi_value TestAdjustExternalMemory(napi_env env, napi_callback_info info) {
  (void)info;
  int64_t adjusted_value;
  napi_value result;
  NODE_API_CALL(env, napi_adjust_external_memory(env, 1024, &adjusted_value));
  NODE_API_CALL(env, napi_create_double(env, static_cast<double>(adjusted_value), &result));
  return result;
}

}  // namespace

extern "C" napi_value Init(napi_env env, napi_value exports) {
  napi_property_descriptor descriptors[] = {
      DECLARE_NODE_API_PROPERTY("testStrictEquals", TestStrictEquals),
      DECLARE_NODE_API_PROPERTY("testSetPrototype", TestSetPrototype),
      DECLARE_NODE_API_PROPERTY("testGetPrototype", TestGetPrototype),
      DECLARE_NODE_API_PROPERTY("testGetVersion", TestGetVersion),
      DECLARE_NODE_API_PROPERTY("testNapiTypeof", TestNapiTypeof),
      DECLARE_NODE_API_PROPERTY("wrap", Wrap),
      DECLARE_NODE_API_PROPERTY("removeWrap", RemoveWrap),
      DECLARE_NODE_API_PROPERTY("derefItemWasCalled", DerefItemWasCalled),
      DECLARE_NODE_API_PROPERTY("testFinalizeWrap", TestFinalizeWrap),
      DECLARE_NODE_API_PROPERTY("finalizeWasCalled", FinalizeWasCalled),
      DECLARE_NODE_API_PROPERTY("testAdjustExternalMemory", TestAdjustExternalMemory),
  };

  NODE_API_CALL(
      env,
      napi_define_properties(env,
                             exports,
                             sizeof(descriptors) / sizeof(*descriptors),
                             descriptors));
  return exports;
}
