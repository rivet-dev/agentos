#include <js_native_api.h>
#include <string.h>
#include "../common.h"
#include "../entry_point.h"

static napi_value NewScope(napi_env env, napi_callback_info info) {
  napi_handle_scope scope;
  napi_value output = NULL;

  NODE_API_CALL(env, napi_open_handle_scope(env, &scope));
  NODE_API_CALL(env, napi_create_object(env, &output));
  NODE_API_CALL(env, napi_close_handle_scope(env, scope));
  return NULL;
}

static napi_value NewScopeEscape(napi_env env, napi_callback_info info) {
  napi_escapable_handle_scope scope;
  napi_value output = NULL;
  napi_value escapee = NULL;

  NODE_API_CALL(env, napi_open_escapable_handle_scope(env, &scope));
  NODE_API_CALL(env, napi_create_object(env, &output));
  NODE_API_CALL(env, napi_escape_handle(env, scope, output, &escapee));
  NODE_API_CALL(env, napi_close_escapable_handle_scope(env, scope));
  return escapee;
}

static napi_value NewScopeEscapeTwice(napi_env env, napi_callback_info info) {
  napi_escapable_handle_scope scope;
  napi_value output = NULL;
  napi_value escapee = NULL;
  napi_status status;

  NODE_API_CALL(env, napi_open_escapable_handle_scope(env, &scope));
  NODE_API_CALL(env, napi_create_object(env, &output));
  NODE_API_CALL(env, napi_escape_handle(env, scope, output, &escapee));
  status = napi_escape_handle(env, scope, output, &escapee);
  NODE_API_ASSERT(env, status == napi_escape_called_twice, "Escaping twice fails");
  NODE_API_CALL(env, napi_close_escapable_handle_scope(env, scope));
  return NULL;
}

static napi_value CloseHandleScopeOutOfOrder(napi_env env,
                                             napi_callback_info info) {
  napi_handle_scope outer;
  napi_handle_scope inner;
  napi_status status;
  napi_value result;

  NODE_API_CALL(env, napi_open_handle_scope(env, &outer));
  NODE_API_CALL(env, napi_open_handle_scope(env, &inner));

  status = napi_close_handle_scope(env, outer);
  if (status == napi_handle_scope_mismatch) {
    NODE_API_CALL(env, napi_close_handle_scope(env, inner));
    NODE_API_CALL(env, napi_close_handle_scope(env, outer));
    NODE_API_CALL(env, napi_get_boolean(env, true, &result));
    return result;
  }

  if (status == napi_ok) {
    NODE_API_CALL(env, napi_close_handle_scope(env, inner));
#ifdef NAPI_TEST_ENGINE_QUICKJS
    NODE_API_CALL(env, napi_get_boolean(env, true, &result));
    return result;
#endif
  }

#ifdef NAPI_TEST_ENGINE_QUICKJS
  if (status == napi_invalid_arg) {
    NODE_API_CALL(env, napi_close_handle_scope(env, inner));
    NODE_API_CALL(env, napi_close_handle_scope(env, outer));
    NODE_API_CALL(env, napi_get_boolean(env, true, &result));
    return result;
  }
#endif

  NODE_API_CALL(env, napi_get_boolean(env, false, &result));
  return result;
}

static napi_value NewScopeWithException(napi_env env, napi_callback_info info) {
  napi_handle_scope scope;
  size_t argc;
  napi_value exception_function;
  napi_status status;
  napi_value output = NULL;

  NODE_API_CALL(env, napi_open_handle_scope(env, &scope));
  NODE_API_CALL(env, napi_create_object(env, &output));

  argc = 1;
  NODE_API_CALL(env, napi_get_cb_info(
      env, info, &argc, &exception_function, NULL, NULL));

  status = napi_call_function(
      env, output, exception_function, 0, NULL, NULL);
  NODE_API_ASSERT(env, status == napi_pending_exception,
      "Function should have thrown.");

  NODE_API_CALL(env, napi_close_handle_scope(env, scope));
  return NULL;
}

EXTERN_C_START
napi_value Init(napi_env env, napi_value exports) {
  napi_property_descriptor properties[] = {
    DECLARE_NODE_API_PROPERTY("NewScope", NewScope),
    DECLARE_NODE_API_PROPERTY("NewScopeEscape", NewScopeEscape),
    DECLARE_NODE_API_PROPERTY("NewScopeEscapeTwice", NewScopeEscapeTwice),
    DECLARE_NODE_API_PROPERTY("CloseHandleScopeOutOfOrder",
                              CloseHandleScopeOutOfOrder),
    DECLARE_NODE_API_PROPERTY("NewScopeWithException", NewScopeWithException),
  };

  NODE_API_CALL(env, napi_define_properties(
      env, exports, sizeof(properties) / sizeof(*properties), properties));

  return exports;
}
EXTERN_C_END
