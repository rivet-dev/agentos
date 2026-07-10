#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create an array
  napi_value arr;
  NAPI_CALL(env, napi_create_array(env, &arr));

  // Set elements
  napi_value v0, v1, v2;
  NAPI_CALL(env, napi_create_int32(env, 10, &v0));
  NAPI_CALL(env, napi_create_int32(env, 20, &v1));
  NAPI_CALL(env, napi_create_int32(env, 30, &v2));
  NAPI_CALL(env, napi_set_element(env, arr, 0, v0));
  NAPI_CALL(env, napi_set_element(env, arr, 1, v1));
  NAPI_CALL(env, napi_set_element(env, arr, 2, v2));

  // Check length
  uint32_t length;
  NAPI_CALL(env, napi_get_array_length(env, arr, &length));
  CHECK_OR_FAIL(length == 3, "expected array length 3");

  // Read elements back
  napi_value g0, g1, g2;
  NAPI_CALL(env, napi_get_element(env, arr, 0, &g0));
  NAPI_CALL(env, napi_get_element(env, arr, 1, &g1));
  NAPI_CALL(env, napi_get_element(env, arr, 2, &g2));

  int32_t r0, r1, r2;
  NAPI_CALL(env, napi_get_value_int32(env, g0, &r0));
  NAPI_CALL(env, napi_get_value_int32(env, g1, &r1));
  NAPI_CALL(env, napi_get_value_int32(env, g2, &r2));
  CHECK_OR_FAIL(r0 == 10, "element 0 mismatch");
  CHECK_OR_FAIL(r1 == 20, "element 1 mismatch");
  CHECK_OR_FAIL(r2 == 30, "element 2 mismatch");

  // Create array with length
  napi_value arr2;
  NAPI_CALL(env, napi_create_array_with_length(env, 5, &arr2));
  uint32_t len2;
  NAPI_CALL(env, napi_get_array_length(env, arr2, &len2));
  CHECK_OR_FAIL(len2 == 5, "expected pre-allocated array length 5");

  return PrintSuccess("ARRAY_TEST");
}
