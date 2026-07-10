#include <math.h>
#include <stdint.h>
#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- int32: basic values ----
  {
    napi_value val;
    int32_t result;

    NAPI_CALL(env, napi_create_int32(env, 42, &val));
    NAPI_CALL(env, napi_get_value_int32(env, val, &result));
    CHECK_OR_FAIL(result == 42, "int32: expected 42");

    NAPI_CALL(env, napi_create_int32(env, 0, &val));
    NAPI_CALL(env, napi_get_value_int32(env, val, &result));
    CHECK_OR_FAIL(result == 0, "int32: expected 0");

    NAPI_CALL(env, napi_create_int32(env, -1, &val));
    NAPI_CALL(env, napi_get_value_int32(env, val, &result));
    CHECK_OR_FAIL(result == -1, "int32: expected -1");
  }

  // ---- int32: edge cases ----
  {
    napi_value val;
    int32_t result;

    NAPI_CALL(env, napi_create_int32(env, INT32_MIN, &val));
    NAPI_CALL(env, napi_get_value_int32(env, val, &result));
    CHECK_OR_FAIL(result == INT32_MIN, "int32: expected INT32_MIN");

    NAPI_CALL(env, napi_create_int32(env, INT32_MAX, &val));
    NAPI_CALL(env, napi_get_value_int32(env, val, &result));
    CHECK_OR_FAIL(result == INT32_MAX, "int32: expected INT32_MAX");
  }

  // ---- uint32: basic and edge cases ----
  {
    napi_value val;
    uint32_t result;

    NAPI_CALL(env, napi_create_uint32(env, 0, &val));
    NAPI_CALL(env, napi_get_value_uint32(env, val, &result));
    CHECK_OR_FAIL(result == 0, "uint32: expected 0");

    NAPI_CALL(env, napi_create_uint32(env, 100, &val));
    NAPI_CALL(env, napi_get_value_uint32(env, val, &result));
    CHECK_OR_FAIL(result == 100, "uint32: expected 100");

    NAPI_CALL(env, napi_create_uint32(env, UINT32_MAX, &val));
    NAPI_CALL(env, napi_get_value_uint32(env, val, &result));
    CHECK_OR_FAIL(result == UINT32_MAX, "uint32: expected UINT32_MAX");
  }

  // ---- double: basic precision ----
  {
    napi_value val;
    double result;

    NAPI_CALL(env, napi_create_double(env, 3.14159, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    CHECK_OR_FAIL(result > 3.14158 && result < 3.14160,
                  "double: expected ~3.14159");

    NAPI_CALL(env, napi_create_double(env, 0.0, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    CHECK_OR_FAIL(result == 0.0, "double: expected 0.0");

    NAPI_CALL(env, napi_create_double(env, -1.5, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    CHECK_OR_FAIL(result == -1.5, "double: expected -1.5");
  }

  // ---- double: negative zero ----
  {
    napi_value val;
    double result;

    NAPI_CALL(env, napi_create_double(env, -0.0, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    // -0.0 == 0.0 is true in C, but 1/-0.0 == -Inf
    CHECK_OR_FAIL(result == 0.0, "double: -0.0 should equal 0.0");
    CHECK_OR_FAIL(1.0 / result < 0.0,
                  "double: expected negative zero (1/-0.0 should be -Inf)");
  }

  // ---- double: large values ----
  {
    napi_value val;
    double result;

    NAPI_CALL(env, napi_create_double(env, 1e300, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    CHECK_OR_FAIL(result == 1e300, "double: expected 1e300");

    NAPI_CALL(env, napi_create_double(env, -1e300, &val));
    NAPI_CALL(env, napi_get_value_double(env, val, &result));
    CHECK_OR_FAIL(result == -1e300, "double: expected -1e300");
  }

  // ---- int64: basic and large values ----
  {
    napi_value val;
    int64_t result;

    NAPI_CALL(env, napi_create_int64(env, 0, &val));
    NAPI_CALL(env, napi_get_value_int64(env, val, &result));
    CHECK_OR_FAIL(result == 0, "int64: expected 0");

    NAPI_CALL(env, napi_create_int64(env, -1, &val));
    NAPI_CALL(env, napi_get_value_int64(env, val, &result));
    CHECK_OR_FAIL(result == -1, "int64: expected -1");

    // Large int64 value within safe integer range
    NAPI_CALL(env, napi_create_int64(env, 9007199254740991LL, &val));
    NAPI_CALL(env, napi_get_value_int64(env, val, &result));
    CHECK_OR_FAIL(result == 9007199254740991LL,
                  "int64: expected 2^53 - 1 (MAX_SAFE_INTEGER)");

    NAPI_CALL(env, napi_create_int64(env, -9007199254740991LL, &val));
    NAPI_CALL(env, napi_get_value_int64(env, val, &result));
    CHECK_OR_FAIL(result == -9007199254740991LL,
                  "int64: expected -(2^53 - 1) (MIN_SAFE_INTEGER)");
  }

  // ---- typeof number values ----
  {
    napi_valuetype vtype;
    napi_value val;

    NAPI_CALL(env, napi_create_int32(env, 7, &val));
    NAPI_CALL(env, napi_typeof(env, val, &vtype));
    CHECK_OR_FAIL(vtype == napi_number, "typeof int32: expected napi_number");

    NAPI_CALL(env, napi_create_double(env, 1.5, &val));
    NAPI_CALL(env, napi_typeof(env, val, &vtype));
    CHECK_OR_FAIL(vtype == napi_number, "typeof double: expected napi_number");

    NAPI_CALL(env, napi_create_uint32(env, 10, &val));
    NAPI_CALL(env, napi_typeof(env, val, &vtype));
    CHECK_OR_FAIL(vtype == napi_number, "typeof uint32: expected napi_number");

    NAPI_CALL(env, napi_create_int64(env, 100, &val));
    NAPI_CALL(env, napi_typeof(env, val, &vtype));
    CHECK_OR_FAIL(vtype == napi_number, "typeof int64: expected napi_number");
  }

  return PrintSuccess("TEST_NUMBER");
}
