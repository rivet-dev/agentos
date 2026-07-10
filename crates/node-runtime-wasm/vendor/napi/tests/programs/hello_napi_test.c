#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create a string
  napi_value str_val;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "Hello, NAPI!", NAPI_AUTO_LENGTH,
                                    &str_val));

  // Read it back
  char buf[256];
  size_t len;
  NAPI_CALL(env,
            napi_get_value_string_utf8(env, str_val, buf, sizeof(buf), &len));
  CHECK_OR_FAIL(strcmp(buf, "Hello, NAPI!") == 0, "string mismatch");
  CHECK_OR_FAIL(len == 12, "length mismatch");

  // Create and read an int32
  napi_value int_val;
  NAPI_CALL(env, napi_create_int32(env, 42, &int_val));
  int32_t int_result;
  NAPI_CALL(env, napi_get_value_int32(env, int_val, &int_result));
  CHECK_OR_FAIL(int_result == 42, "int32 mismatch");

  // Create and read a double
  napi_value dbl_val;
  NAPI_CALL(env, napi_create_double(env, 3.14, &dbl_val));
  double dbl_result;
  NAPI_CALL(env, napi_get_value_double(env, dbl_val, &dbl_result));
  CHECK_OR_FAIL(dbl_result > 3.13 && dbl_result < 3.15, "double mismatch");

  return PrintSuccess("HELLO_NAPI_TEST");
}
