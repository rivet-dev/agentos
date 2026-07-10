#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

// --- Method callbacks ---

// Method that returns a constant
static napi_value method_hello(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value result;
  napi_create_string_utf8(env, "hello_from_method", NAPI_AUTO_LENGTH, &result);
  return result;
}

// Method that doubles its first argument
static napi_value method_double(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);

  int32_t val = 0;
  napi_get_value_int32(env, argv[0], &val);

  napi_value result;
  napi_create_int32(env, val * 2, &result);
  return result;
}

// Method that accesses 'this' to read a property
static napi_value method_get_x(napi_env env, napi_callback_info info) {
  napi_value this_arg;
  napi_get_cb_info(env, info, NULL, NULL, &this_arg, NULL);

  napi_value key, val;
  napi_create_string_utf8(env, "x", NAPI_AUTO_LENGTH, &key);
  napi_get_property(env, this_arg, key, &val);
  return val;
}

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_value global;
  NAPI_CALL(env, napi_get_global(env, &global));

  // Test 1: Define a method property using napi_create_function + set_named_property
  // (This tests the fundamental callback dispatch for object methods)
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "hello", NAPI_AUTO_LENGTH,
                                        method_hello, NULL, &fn));
    NAPI_CALL(env, napi_set_named_property(env, obj, "hello", fn));

    // Call the method
    napi_value method;
    NAPI_CALL(env, napi_get_named_property(env, obj, "hello", &method));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, obj, method, 0, NULL, &result));

    char buf[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf8(env, result, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "hello_from_method") == 0,
                  "method: expected 'hello_from_method'");
  }

  // Test 2: Method with arguments (double)
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "double", NAPI_AUTO_LENGTH,
                                        method_double, NULL, &fn));
    NAPI_CALL(env, napi_set_named_property(env, obj, "double", fn));

    napi_value method;
    NAPI_CALL(env, napi_get_named_property(env, obj, "double", &method));

    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 21, &arg));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, obj, method, 1, &arg, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 42, "double: expected 42");
  }

  // Test 3: Method that reads 'this.x'
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    // Set x property
    napi_value x_key, x_val;
    NAPI_CALL(env, napi_create_string_utf8(env, "x", NAPI_AUTO_LENGTH, &x_key));
    NAPI_CALL(env, napi_create_int32(env, 99, &x_val));
    NAPI_CALL(env, napi_set_property(env, obj, x_key, x_val));

    // Set method
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "getX", NAPI_AUTO_LENGTH,
                                        method_get_x, NULL, &fn));
    NAPI_CALL(env, napi_set_named_property(env, obj, "getX", fn));

    // Call method
    napi_value method;
    NAPI_CALL(env, napi_get_named_property(env, obj, "getX", &method));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, obj, method, 0, NULL, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 99, "getX: expected 99");
  }

  // Test 4: Multiple methods on same object
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value fn1, fn2;
    NAPI_CALL(env, napi_create_function(env, "hello", NAPI_AUTO_LENGTH,
                                        method_hello, NULL, &fn1));
    NAPI_CALL(env, napi_create_function(env, "double", NAPI_AUTO_LENGTH,
                                        method_double, NULL, &fn2));

    NAPI_CALL(env, napi_set_named_property(env, obj, "greet", fn1));
    NAPI_CALL(env, napi_set_named_property(env, obj, "dbl", fn2));

    // Call first method
    napi_value m1;
    NAPI_CALL(env, napi_get_named_property(env, obj, "greet", &m1));
    napi_value r1;
    NAPI_CALL(env, napi_call_function(env, obj, m1, 0, NULL, &r1));
    char buf[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf8(env, r1, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "hello_from_method") == 0, "multi: greet failed");

    // Call second method
    napi_value m2;
    NAPI_CALL(env, napi_get_named_property(env, obj, "dbl", &m2));
    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 7, &arg));
    napi_value r2;
    NAPI_CALL(env, napi_call_function(env, obj, m2, 1, &arg, &r2));
    int32_t v2;
    NAPI_CALL(env, napi_get_value_int32(env, r2, &v2));
    CHECK_OR_FAIL(v2 == 14, "multi: dbl expected 14");
  }

  // Test 5: Function as value property (pass function around)
  {
    napi_value fn;
    NAPI_CALL(env, napi_create_function(env, "echo", NAPI_AUTO_LENGTH,
                                        method_double, NULL, &fn));

    // Store in object and retrieve
    napi_value container;
    NAPI_CALL(env, napi_create_object(env, &container));
    NAPI_CALL(env, napi_set_named_property(env, container, "func", fn));

    napi_value retrieved;
    NAPI_CALL(env, napi_get_named_property(env, container, "func", &retrieved));

    // Call retrieved function
    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 5, &arg));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, global, retrieved, 1, &arg, &result));

    int32_t val;
    NAPI_CALL(env, napi_get_value_int32(env, result, &val));
    CHECK_OR_FAIL(val == 10, "pass-around: expected 10");
  }

  return PrintSuccess("TEST_DEFINE_PROPERTIES");
}
