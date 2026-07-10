#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Run a simple script that returns a string
  napi_value script_str;
  NAPI_CALL(env, napi_create_string_utf8(env, "'Hello' + ', World!'",
                                          NAPI_AUTO_LENGTH, &script_str));
  napi_value result;
  NAPI_CALL(env, napi_run_script(env, script_str, &result));

  // Check the result
  char buf[256];
  size_t len;
  NAPI_CALL(env, napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
  CHECK_OR_FAIL(strcmp(buf, "Hello, World!") == 0, "script result mismatch");

  // Run a script that returns a number
  napi_value num_script;
  NAPI_CALL(env, napi_create_string_utf8(env, "2 + 3 * 4",
                                          NAPI_AUTO_LENGTH, &num_script));
  napi_value num_result;
  NAPI_CALL(env, napi_run_script(env, num_script, &num_result));

  double num;
  NAPI_CALL(env, napi_get_value_double(env, num_result, &num));
  CHECK_OR_FAIL(num > 13.9 && num < 14.1, "expected 14");

  return PrintSuccess("RUN_SCRIPT_TEST");
}
