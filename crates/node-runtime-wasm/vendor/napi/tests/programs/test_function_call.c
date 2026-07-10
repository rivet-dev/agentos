#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Get the global object to use as receiver
  napi_value global;
  NAPI_CALL(env, napi_get_global(env, &global));

  // --- Test 1: Call a function that adds two numbers ---

  // Create the function via napi_run_script
  // The script evaluates to a function expression (IIFE-wrapped not needed;
  // a bare function expression in parentheses works).
  napi_value add_script;
  NAPI_CALL(env,
            napi_create_string_utf8(
                env, "(function(a, b) { return a + b; })",
                NAPI_AUTO_LENGTH, &add_script));
  napi_value add_func;
  NAPI_CALL(env, napi_run_script(env, add_script, &add_func));

  // Verify it is a function
  napi_valuetype vtype;
  NAPI_CALL(env, napi_typeof(env, add_func, &vtype));
  CHECK_OR_FAIL(vtype == napi_function, "expected napi_function for add_func");

  // Prepare arguments: 3.5 + 4.5 = 8.0
  napi_value arg_a, arg_b;
  NAPI_CALL(env, napi_create_double(env, 3.5, &arg_a));
  NAPI_CALL(env, napi_create_double(env, 4.5, &arg_b));
  napi_value args[2] = { arg_a, arg_b };

  // Call the function
  napi_value add_result;
  NAPI_CALL(env,
            napi_call_function(env, global, add_func, 2, args, &add_result));

  // Read back the result as a double
  double sum;
  NAPI_CALL(env, napi_get_value_double(env, add_result, &sum));
  CHECK_OR_FAIL(sum > 7.9 && sum < 8.1, "expected sum of 8.0");

  // --- Test 2: Call a function that returns a string, no arguments ---

  napi_value hello_script;
  NAPI_CALL(env,
            napi_create_string_utf8(
                env, "(function() { return 'hello'; })",
                NAPI_AUTO_LENGTH, &hello_script));
  napi_value hello_func;
  NAPI_CALL(env, napi_run_script(env, hello_script, &hello_func));

  // Verify it is a function
  NAPI_CALL(env, napi_typeof(env, hello_func, &vtype));
  CHECK_OR_FAIL(vtype == napi_function,
                "expected napi_function for hello_func");

  // Call with no arguments
  napi_value hello_result;
  NAPI_CALL(env,
            napi_call_function(env, global, hello_func, 0, NULL, &hello_result));

  // Verify the result is the string "hello"
  char buf[256];
  size_t len;
  NAPI_CALL(env,
            napi_get_value_string_utf8(env, hello_result, buf, sizeof(buf), &len));
  CHECK_OR_FAIL(strcmp(buf, "hello") == 0, "expected string 'hello'");

  return PrintSuccess("TEST_FUNCTION_CALL");
}
