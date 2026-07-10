#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- Test napi_coerce_to_bool: number -> bool ----
  {
    // Non-zero number coerces to true
    napi_value num;
    NAPI_CALL(env, napi_create_int32(env, 42, &num));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_bool(env, num, &result));
    bool bval;
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == true, "coerce_to_bool: 42 should be true");

    // Zero coerces to false
    napi_value zero;
    NAPI_CALL(env, napi_create_int32(env, 0, &zero));
    NAPI_CALL(env, napi_coerce_to_bool(env, zero, &result));
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == false, "coerce_to_bool: 0 should be false");
  }

  // ---- Test napi_coerce_to_bool: string -> bool ----
  {
    // Non-empty string coerces to true
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "hello", NAPI_AUTO_LENGTH, &str));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_bool(env, str, &result));
    bool bval;
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == true,
                  "coerce_to_bool: non-empty string should be true");

    // Empty string coerces to false
    napi_value empty;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty));
    NAPI_CALL(env, napi_coerce_to_bool(env, empty, &result));
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == false,
                  "coerce_to_bool: empty string should be false");
  }

  // ---- Test napi_coerce_to_bool: null -> bool ----
  {
    napi_value null_val;
    NAPI_CALL(env, napi_get_null(env, &null_val));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_bool(env, null_val, &result));
    bool bval;
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == false, "coerce_to_bool: null should be false");
  }

  // ---- Test napi_coerce_to_bool: undefined -> bool ----
  {
    napi_value undef;
    NAPI_CALL(env, napi_get_undefined(env, &undef));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_bool(env, undef, &result));
    bool bval;
    NAPI_CALL(env, napi_get_value_bool(env, result, &bval));
    CHECK_OR_FAIL(bval == false,
                  "coerce_to_bool: undefined should be false");
  }

  // ---- Test napi_coerce_to_number: string "42" -> number ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "42", NAPI_AUTO_LENGTH, &str));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_number(env, str, &result));
    double dval;
    NAPI_CALL(env, napi_get_value_double(env, result, &dval));
    CHECK_OR_FAIL(dval == 42.0,
                  "coerce_to_number: '42' should become 42.0");
  }

  // ---- Test napi_coerce_to_number: bool -> number ----
  {
    napi_value btrue;
    NAPI_CALL(env, napi_get_boolean(env, true, &btrue));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_number(env, btrue, &result));
    double dval;
    NAPI_CALL(env, napi_get_value_double(env, result, &dval));
    CHECK_OR_FAIL(dval == 1.0,
                  "coerce_to_number: true should become 1.0");

    napi_value bfalse;
    NAPI_CALL(env, napi_get_boolean(env, false, &bfalse));
    NAPI_CALL(env, napi_coerce_to_number(env, bfalse, &result));
    NAPI_CALL(env, napi_get_value_double(env, result, &dval));
    CHECK_OR_FAIL(dval == 0.0,
                  "coerce_to_number: false should become 0.0");
  }

  // ---- Test napi_coerce_to_number: null -> number ----
  {
    napi_value null_val;
    NAPI_CALL(env, napi_get_null(env, &null_val));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_number(env, null_val, &result));
    double dval;
    NAPI_CALL(env, napi_get_value_double(env, result, &dval));
    CHECK_OR_FAIL(dval == 0.0,
                  "coerce_to_number: null should become 0.0");
  }

  // ---- Test napi_coerce_to_string: number -> string ----
  {
    napi_value num;
    NAPI_CALL(env, napi_create_int32(env, 123, &num));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_string(env, num, &result));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "123") == 0,
                  "coerce_to_string: 123 should become '123'");
  }

  // ---- Test napi_coerce_to_string: bool -> string ----
  {
    napi_value btrue;
    NAPI_CALL(env, napi_get_boolean(env, true, &btrue));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_string(env, btrue, &result));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "true") == 0,
                  "coerce_to_string: true should become 'true'");

    napi_value bfalse;
    NAPI_CALL(env, napi_get_boolean(env, false, &bfalse));
    NAPI_CALL(env, napi_coerce_to_string(env, bfalse, &result));
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "false") == 0,
                  "coerce_to_string: false should become 'false'");
  }

  // ---- Test napi_coerce_to_string: null -> string ----
  {
    napi_value null_val;
    NAPI_CALL(env, napi_get_null(env, &null_val));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_string(env, null_val, &result));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "null") == 0,
                  "coerce_to_string: null should become 'null'");
  }

  // ---- Test napi_coerce_to_object: number -> Number object ----
  {
    napi_value num;
    NAPI_CALL(env, napi_create_int32(env, 7, &num));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_object(env, num, &result));
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, result, &vtype));
    CHECK_OR_FAIL(vtype == napi_object,
                  "coerce_to_object: number should become object");
  }

  // ---- Test napi_coerce_to_object: string -> String object ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "boxed", NAPI_AUTO_LENGTH, &str));
    napi_value result;
    NAPI_CALL(env, napi_coerce_to_object(env, str, &result));
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, result, &vtype));
    CHECK_OR_FAIL(vtype == napi_object,
                  "coerce_to_object: string should become object");
  }

  return PrintSuccess("TEST_COERCION");
}
