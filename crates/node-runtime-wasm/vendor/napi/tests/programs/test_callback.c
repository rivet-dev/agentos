#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

// --- Callback functions ---

// Simple callback that returns its first argument
static napi_value echo_cb(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  if (argc >= 1) return argv[0];
  napi_value undef;
  napi_get_undefined(env, &undef);
  return undef;
}

// Callback that adds two int32 arguments
static napi_value add_cb(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);

  int32_t a = 0, b = 0;
  napi_get_value_int32(env, argv[0], &a);
  napi_get_value_int32(env, argv[1], &b);

  napi_value result;
  napi_create_int32(env, a + b, &result);
  return result;
}

// Callback that returns the 'this' argument
static napi_value get_this_cb(napi_env env, napi_callback_info info) {
  napi_value this_arg;
  napi_get_cb_info(env, info, NULL, NULL, &this_arg, NULL);
  return this_arg;
}

// Callback that returns the data pointer as an int
static napi_value get_data_cb(napi_env env, napi_callback_info info) {
  void* data = NULL;
  napi_get_cb_info(env, info, NULL, NULL, NULL, &data);

  napi_value result;
  napi_create_int32(env, (int32_t)(intptr_t)data, &result);
  return result;
}

// Callback with zero args — returns a constant string
static napi_value hello_cb(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value result;
  napi_create_string_utf8(env, "hello", NAPI_AUTO_LENGTH, &result);
  return result;
}

// Callback that reports argc
static napi_value argc_cb(napi_env env, napi_callback_info info) {
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, NULL, NULL, NULL);
  napi_value result;
  napi_create_int32(env, (int32_t)argc, &result);
  return result;
}

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_value global;
  NAPI_CALL(env, napi_get_global(env, &global));

  // Test 1: Simple echo callback — pass a value and get it back
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "echo", NAPI_AUTO_LENGTH,
                                        echo_cb, NULL, &fn));

    // Verify it's a function
    napi_valuetype vt;
    NAPI_CALL(env, napi_typeof(env, fn, &vt));
    CHECK_OR_FAIL(vt == napi_function, "echo: expected napi_function type");

    // Call with an int argument
    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 42, &arg));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, global, fn, 1, &arg, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 42, "echo: expected 42");
  }

  // Test 2: Add callback — pass two ints, get sum
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "add", NAPI_AUTO_LENGTH,
                                        add_cb, NULL, &fn));
    napi_value args[2];
    NAPI_CALL(env, napi_create_int32(env, 10, &args[0]));
    NAPI_CALL(env, napi_create_int32(env, 32, &args[1]));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, global, fn, 2, args, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 42, "add: expected 42");
  }

  // Test 3: Callback with zero arguments — returns a constant
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "hello", NAPI_AUTO_LENGTH,
                                        hello_cb, NULL, &fn));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, global, fn, 0, NULL, &result));

    char buf[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "hello") == 0, "hello: expected 'hello'");
  }

  // Test 4: argc reporting callback
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "argc", NAPI_AUTO_LENGTH,
                                        argc_cb, NULL, &fn));

    // Call with 0 args
    napi_value r0;
    NAPI_CALL(env, napi_call_function(env, global, fn, 0, NULL, &r0));
    int32_t v0;
    NAPI_CALL(env, napi_get_value_int32(env, r0, &v0));
    CHECK_OR_FAIL(v0 == 0, "argc: expected 0 for no args");

    // Call with 3 args
    napi_value args[3];
    NAPI_CALL(env, napi_create_int32(env, 1, &args[0]));
    NAPI_CALL(env, napi_create_int32(env, 2, &args[1]));
    NAPI_CALL(env, napi_create_int32(env, 3, &args[2]));
    napi_value r3;
    NAPI_CALL(env, napi_call_function(env, global, fn, 3, args, &r3));
    int32_t v3;
    NAPI_CALL(env, napi_get_value_int32(env, r3, &v3));
    CHECK_OR_FAIL(v3 == 3, "argc: expected 3 for three args");
  }

  // Test 5: Data pointer — pass a value through the data param
  {
    void* my_data = (void*)(intptr_t)12345;
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "getdata", NAPI_AUTO_LENGTH,
                                        get_data_cb, my_data, &fn));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, global, fn, 0, NULL, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 12345, "data: expected 12345");
  }

  // Test 6: Calling a callback multiple times
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "echo2", NAPI_AUTO_LENGTH,
                                        echo_cb, NULL, &fn));
    for (int i = 0; i < 5; i++) {
      napi_value arg, result;
      NAPI_CALL(env, napi_create_int32(env, i * 10, &arg));
      NAPI_CALL(env, napi_call_function(env, global, fn, 1, &arg, &result));
      int32_t val;
      NAPI_CALL(env, napi_get_value_int32(env, result, &val));
      CHECK_OR_FAIL(val == i * 10, "multi-call: value mismatch");
    }
  }

  return PrintSuccess("TEST_CALLBACK");
}
