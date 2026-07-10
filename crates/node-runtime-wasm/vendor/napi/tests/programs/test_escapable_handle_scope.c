#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_value escaped_obj;

  // Open an escapable handle scope
  napi_escapable_handle_scope scope;
  NAPI_CALL(env, napi_open_escapable_handle_scope(env, &scope));

  // Create an object inside the escapable scope
  napi_value inner_obj;
  NAPI_CALL(env, napi_create_object(env, &inner_obj));

  // Set a property on the object so we can verify it later
  napi_value str_val;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "escaped", NAPI_AUTO_LENGTH, &str_val));
  NAPI_CALL(env, napi_set_named_property(env, inner_obj, "status", str_val));

  // Escape the handle so it survives scope closure
  NAPI_CALL(env, napi_escape_handle(env, scope, inner_obj, &escaped_obj));

  // Close the escapable handle scope
  NAPI_CALL(env, napi_close_escapable_handle_scope(env, scope));

  // Verify the escaped object is still accessible
  napi_valuetype vtype;
  NAPI_CALL(env, napi_typeof(env, escaped_obj, &vtype));
  CHECK_OR_FAIL(vtype == napi_object, "escaped value should be an object");

  // Verify the property on the escaped object is intact
  napi_value got_val;
  NAPI_CALL(env, napi_get_named_property(env, escaped_obj, "status", &got_val));
  char buf[256];
  size_t len;
  NAPI_CALL(env,
            napi_get_value_string_utf8(env, got_val, buf, sizeof(buf), &len));
  CHECK_OR_FAIL(strcmp(buf, "escaped") == 0,
                "escaped object property mismatch");

  return PrintSuccess("TEST_ESCAPABLE_HANDLE_SCOPE");
}
