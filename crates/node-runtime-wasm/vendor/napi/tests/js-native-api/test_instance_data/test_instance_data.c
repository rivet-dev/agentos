#include <js_native_api.h>
#include <node_api_types.h>
#include <stdio.h>
#include <stdlib.h>
#include "../common.h"
#include "../entry_point.h"

NAPI_EXTERN napi_status NAPI_CDECL napi_add_env_cleanup_hook(
    node_api_basic_env env, napi_cleanup_hook fun, void* arg);
NAPI_EXTERN napi_status NAPI_CDECL napi_remove_env_cleanup_hook(
    node_api_basic_env env, napi_cleanup_hook fun, void* arg);

typedef struct {
  size_t value;
  bool print;
  napi_ref js_cb_ref;
} AddonData;

typedef struct {
  napi_env env;
} CleanupHookState;

static CleanupHookState cleanup_hook_state;
static int cleanup_order_state = 0;

static void RemovedCleanupHook(void* arg) {
  (void)arg;
  abort();
}

static void RemovingCleanupHook(void* arg) {
  CleanupHookState* state = arg;
  NODE_API_BASIC_CALL_RETURN_VOID(
      state->env,
      napi_remove_env_cleanup_hook(
          state->env, RemovedCleanupHook, state));
}

static void CleanupHookOrderA(void* arg) {
  (void)arg;
  if (cleanup_order_state != 2) {
    abort();
  }
  cleanup_order_state = 3;
}

static void CleanupHookOrderB(void* arg) {
  (void)arg;
  if (cleanup_order_state != 1) {
    abort();
  }
  cleanup_order_state = 2;
}

static void CleanupHookOrderC(void* arg) {
  (void)arg;
  if (cleanup_order_state != 0) {
    abort();
  }
  cleanup_order_state = 1;
}

static napi_value Increment(napi_env env, napi_callback_info info) {
  AddonData* data;
  napi_value result;

  NODE_API_CALL(env, napi_get_instance_data(env, (void**)&data));
  NODE_API_CALL(env, napi_create_uint32(env, ++data->value, &result));

  return result;
}

static void DeleteAddonData(napi_env env, void* raw_data, void* hint) {
  AddonData* data = raw_data;
  if (data->print) {
    printf("deleting addon data\n");
  }
  if (data->js_cb_ref != NULL) {
    NODE_API_CALL_RETURN_VOID(env, napi_delete_reference(env, data->js_cb_ref));
  }
  free(data);
}

static napi_value SetPrintOnDelete(napi_env env, napi_callback_info info) {
  AddonData* data;

  NODE_API_CALL(env, napi_get_instance_data(env, (void**)&data));
  data->print = true;

  return NULL;
}

static void TestFinalizer(napi_env env, void* raw_data, void* hint) {
  (void) raw_data;
  (void) hint;

  AddonData* data;
  NODE_API_CALL_RETURN_VOID(env, napi_get_instance_data(env, (void**)&data));
  napi_value js_cb, undefined;
  NODE_API_CALL_RETURN_VOID(env,
      napi_get_reference_value(env, data->js_cb_ref, &js_cb));
  NODE_API_CALL_RETURN_VOID(env, napi_get_undefined(env, &undefined));
  NODE_API_CALL_RETURN_VOID(env,
      napi_call_function(env, undefined, js_cb, 0, NULL, NULL));
  NODE_API_CALL_RETURN_VOID(env, napi_delete_reference(env, data->js_cb_ref));
  data->js_cb_ref = NULL;
}

static napi_value ObjectWithFinalizer(napi_env env, napi_callback_info info) {
  AddonData* data;
  napi_value result, js_cb;
  size_t argc = 1;

  NODE_API_CALL(env, napi_get_instance_data(env, (void**)&data));
  NODE_API_ASSERT(env, data->js_cb_ref == NULL, "reference must be NULL");
  NODE_API_CALL(env, napi_get_cb_info(env, info, &argc, &js_cb, NULL, NULL));
  NODE_API_CALL(env, napi_create_object(env, &result));
  NODE_API_CALL(env,
      napi_add_finalizer(env, result, NULL, TestFinalizer, NULL, NULL));
  NODE_API_CALL(env, napi_create_reference(env, js_cb, 1, &data->js_cb_ref));

  return result;
}

static napi_value RegisterCleanupHookRemoval(napi_env env,
                                             napi_callback_info info) {
  (void)info;

  cleanup_hook_state.env = env;
  NODE_API_CALL(
      env,
      napi_add_env_cleanup_hook(
          env, RemovedCleanupHook, &cleanup_hook_state));
  NODE_API_CALL(
      env,
      napi_add_env_cleanup_hook(
          env, RemovingCleanupHook, &cleanup_hook_state));

  return NULL;
}

static napi_value RegisterCleanupHookOrdering(napi_env env,
                                              napi_callback_info info) {
  (void)info;

  cleanup_order_state = 0;
  NODE_API_CALL(env, napi_add_env_cleanup_hook(env, CleanupHookOrderA, NULL));
  NODE_API_CALL(env, napi_add_env_cleanup_hook(env, CleanupHookOrderB, NULL));
  NODE_API_CALL(env, napi_add_env_cleanup_hook(env, CleanupHookOrderC, NULL));

  return NULL;
}

EXTERN_C_START
napi_value Init(napi_env env, napi_value exports) {
  AddonData* data = malloc(sizeof(*data));
  data->value = 41;
  data->print = false;
  data->js_cb_ref = NULL;

  NODE_API_CALL(env, napi_set_instance_data(env, data, DeleteAddonData, NULL));

  napi_property_descriptor props[] = {
    DECLARE_NODE_API_PROPERTY("increment", Increment),
    DECLARE_NODE_API_PROPERTY("setPrintOnDelete", SetPrintOnDelete),
    DECLARE_NODE_API_PROPERTY("objectWithFinalizer", ObjectWithFinalizer),
    DECLARE_NODE_API_PROPERTY(
        "registerCleanupHookRemoval", RegisterCleanupHookRemoval),
    DECLARE_NODE_API_PROPERTY(
        "registerCleanupHookOrdering", RegisterCleanupHookOrdering),
  };

  NODE_API_CALL(env,
      napi_define_properties(
          env, exports, sizeof(props) / sizeof(*props), props));

  return exports;
}
EXTERN_C_END
