#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

// --- Class: Counter ---
// Constructor: Counter(initialValue)
// Method: increment() -> returns this.count after incrementing
// Method: getValue() -> returns this.count

static napi_value counter_constructor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_value this_arg;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, NULL);

  // Set this.count = arg[0] or 0
  napi_value key;
  napi_create_string_utf8(env, "count", NAPI_AUTO_LENGTH, &key);

  napi_value val;
  if (argc >= 1) {
    val = argv[0];
  } else {
    napi_create_int32(env, 0, &val);
  }
  napi_set_property(env, this_arg, key, val);

  return this_arg;
}

static napi_value counter_increment(napi_env env, napi_callback_info info) {
  napi_value this_arg;
  napi_get_cb_info(env, info, NULL, NULL, &this_arg, NULL);

  napi_value key;
  napi_create_string_utf8(env, "count", NAPI_AUTO_LENGTH, &key);

  napi_value current;
  napi_get_property(env, this_arg, key, &current);

  int32_t v;
  napi_get_value_int32(env, current, &v);
  v++;

  napi_value new_val;
  napi_create_int32(env, v, &new_val);
  napi_set_property(env, this_arg, key, new_val);

  return new_val;
}

static napi_value counter_get_value(napi_env env, napi_callback_info info) {
  napi_value this_arg;
  napi_get_cb_info(env, info, NULL, NULL, &this_arg, NULL);

  napi_value key;
  napi_create_string_utf8(env, "count", NAPI_AUTO_LENGTH, &key);

  napi_value val;
  napi_get_property(env, this_arg, key, &val);
  return val;
}

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_value global;
  NAPI_CALL(env, napi_get_global(env, &global));

  // Test 1: Define a class with constructor and methods
  {
    napi_property_descriptor props[] = {
      {"increment", NULL, counter_increment, NULL, NULL, NULL, napi_default, NULL},
      {"getValue", NULL, counter_get_value, NULL, NULL, NULL, napi_default, NULL},
    };

    napi_value ctor;
    NAPI_CALL(env, napi_define_class(env, "Counter", NAPI_AUTO_LENGTH,
                                     counter_constructor, NULL,
                                     2, props, &ctor));

    // Verify it's a function
    napi_valuetype vt;
    NAPI_CALL(env, napi_typeof(env, ctor, &vt));
    CHECK_OR_FAIL(vt == napi_function, "define_class: expected function type");

    // Create an instance
    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 10, &arg));
    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor, 1, &arg, &instance));

    // Call getValue - should return 10
    napi_value get_method;
    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "getValue", NAPI_AUTO_LENGTH, &key));
    NAPI_CALL(env, napi_get_property(env, instance, key, &get_method));
    napi_value result;
    NAPI_CALL(env, napi_call_function(env, instance, get_method, 0, NULL, &result));
    int32_t v;
    NAPI_CALL(env, napi_get_value_int32(env, result, &v));
    CHECK_OR_FAIL(v == 10, "getValue: expected 10");

    // Call increment 3 times
    napi_value inc_key;
    NAPI_CALL(env, napi_create_string_utf8(env, "increment", NAPI_AUTO_LENGTH, &inc_key));
    napi_value inc_method;
    NAPI_CALL(env, napi_get_property(env, instance, inc_key, &inc_method));
    for (int i = 0; i < 3; i++) {
      NAPI_CALL(env, napi_call_function(env, instance, inc_method, 0, NULL, &result));
    }

    // getValue should now return 13
    NAPI_CALL(env, napi_call_function(env, instance, get_method, 0, NULL, &result));
    NAPI_CALL(env, napi_get_value_int32(env, result, &v));
    CHECK_OR_FAIL(v == 13, "after 3 increments: expected 13");
  }

  // Test 2: Multiple instances of the same class
  {
    napi_property_descriptor props[] = {
      {"getValue", NULL, counter_get_value, NULL, NULL, NULL, napi_default, NULL},
    };

    napi_value ctor;
    NAPI_CALL(env, napi_define_class(env, "Counter2", NAPI_AUTO_LENGTH,
                                     counter_constructor, NULL,
                                     1, props, &ctor));

    napi_value a1, a2;
    NAPI_CALL(env, napi_create_int32(env, 100, &a1));
    NAPI_CALL(env, napi_create_int32(env, 200, &a2));

    napi_value i1, i2;
    NAPI_CALL(env, napi_new_instance(env, ctor, 1, &a1, &i1));
    NAPI_CALL(env, napi_new_instance(env, ctor, 1, &a2, &i2));

    // Each instance should have its own count
    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "getValue", NAPI_AUTO_LENGTH, &key));
    napi_value m1, m2, r1, r2;
    NAPI_CALL(env, napi_get_property(env, i1, key, &m1));
    NAPI_CALL(env, napi_get_property(env, i2, key, &m2));
    NAPI_CALL(env, napi_call_function(env, i1, m1, 0, NULL, &r1));
    NAPI_CALL(env, napi_call_function(env, i2, m2, 0, NULL, &r2));

    int32_t v1, v2;
    NAPI_CALL(env, napi_get_value_int32(env, r1, &v1));
    NAPI_CALL(env, napi_get_value_int32(env, r2, &v2));
    CHECK_OR_FAIL(v1 == 100, "multi-instance: i1 expected 100");
    CHECK_OR_FAIL(v2 == 200, "multi-instance: i2 expected 200");
  }

  // Test 3: instanceof check with defined class
  {
    napi_property_descriptor props[] = {
      {"getValue", NULL, counter_get_value, NULL, NULL, NULL, napi_default, NULL},
    };

    napi_value ctor;
    NAPI_CALL(env, napi_define_class(env, "Counter3", NAPI_AUTO_LENGTH,
                                     counter_constructor, NULL,
                                     1, props, &ctor));

    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor, 0, NULL, &instance));

    bool is_inst;
    NAPI_CALL(env, napi_instanceof(env, instance, ctor, &is_inst));
    CHECK_OR_FAIL(is_inst, "instanceof: expected true for class instance");
  }

  // Test 4: Class with no methods (just constructor)
  {
    napi_value ctor;
    NAPI_CALL(env, napi_define_class(env, "Empty", NAPI_AUTO_LENGTH,
                                     counter_constructor, NULL,
                                     0, NULL, &ctor));

    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 42, &arg));
    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor, 1, &arg, &instance));

    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "count", NAPI_AUTO_LENGTH, &key));
    napi_value val;
    NAPI_CALL(env, napi_get_property(env, instance, key, &val));
    int32_t v;
    NAPI_CALL(env, napi_get_value_int32(env, val, &v));
    CHECK_OR_FAIL(v == 42, "empty class: expected count=42");
  }

  return PrintSuccess("TEST_DEFINE_CLASS");
}
