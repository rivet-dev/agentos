#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_valuetype vtype;

  // undefined
  napi_value undef_val;
  NAPI_CALL(env, napi_get_undefined(env, &undef_val));
  NAPI_CALL(env, napi_typeof(env, undef_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_undefined, "expected napi_undefined");

  // null
  napi_value null_val;
  NAPI_CALL(env, napi_get_null(env, &null_val));
  NAPI_CALL(env, napi_typeof(env, null_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_null, "expected napi_null");

  // boolean
  napi_value bool_val;
  NAPI_CALL(env, napi_get_boolean(env, true, &bool_val));
  NAPI_CALL(env, napi_typeof(env, bool_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_boolean, "expected napi_boolean");

  // number (int32)
  napi_value int_val;
  NAPI_CALL(env, napi_create_int32(env, 7, &int_val));
  NAPI_CALL(env, napi_typeof(env, int_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_number, "expected napi_number for int32");

  // number (double)
  napi_value dbl_val;
  NAPI_CALL(env, napi_create_double(env, 1.5, &dbl_val));
  NAPI_CALL(env, napi_typeof(env, dbl_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_number, "expected napi_number for double");

  // string
  napi_value str_val;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "test", NAPI_AUTO_LENGTH, &str_val));
  NAPI_CALL(env, napi_typeof(env, str_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_string, "expected napi_string");

  // object
  napi_value obj_val;
  NAPI_CALL(env, napi_create_object(env, &obj_val));
  NAPI_CALL(env, napi_typeof(env, obj_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_object, "expected napi_object");

  return PrintSuccess("TYPEOF_TEST");
}
