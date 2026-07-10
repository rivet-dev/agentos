#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

// --- Constructor callback ---
// When called with `new`, new.target is non-null.
// Sets this.value = first arg (or 0 if no args).

static napi_value my_constructor(napi_env env, napi_callback_info info) {
  // Check new.target
  napi_value new_target;
  napi_get_new_target(env, info, &new_target);

  // Get this
  size_t argc = 1;
  napi_value argv[1];
  napi_value this_arg;
  napi_get_cb_info(env, info, &argc, argv, &this_arg, NULL);

  // Set this.value = arg[0] or 42 default
  napi_value key;
  napi_create_string_utf8(env, "value", NAPI_AUTO_LENGTH, &key);

  napi_value val;
  if (argc >= 1) {
    val = argv[0];
  } else {
    napi_create_int32(env, 42, &val);
  }
  napi_set_property(env, this_arg, key, val);

  // Also set this.wasNew = (new_target != undefined)
  napi_value was_new_key;
  napi_create_string_utf8(env, "wasNew", NAPI_AUTO_LENGTH, &was_new_key);

  napi_valuetype nt_type;
  napi_typeof(env, new_target, &nt_type);

  napi_value was_new;
  napi_get_boolean(env, nt_type != napi_undefined, &was_new);
  napi_set_property(env, this_arg, was_new_key, was_new);

  return this_arg;
}

// Simple method for the prototype
static napi_value get_value_method(napi_env env, napi_callback_info info) {
  napi_value this_arg;
  napi_get_cb_info(env, info, NULL, NULL, &this_arg, NULL);

  napi_value key, val;
  napi_create_string_utf8(env, "value", NAPI_AUTO_LENGTH, &key);
  napi_get_property(env, this_arg, key, &val);
  return val;
}

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  napi_value global;
  NAPI_CALL(env, napi_get_global(env, &global));

  // Test 1: napi_new_instance with a constructor function
  {
    // Create constructor function
    napi_value ctor_fn;
    NAPI_CALL(env, napi_create_function(env, "MyObj", NAPI_AUTO_LENGTH,
                                        my_constructor, NULL, &ctor_fn));

    // Call with new
    napi_value arg;
    NAPI_CALL(env, napi_create_int32(env, 99, &arg));
    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor_fn, 1, &arg, &instance));

    // Verify instance.value == 99
    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "value", NAPI_AUTO_LENGTH, &key));
    napi_value val;
    NAPI_CALL(env, napi_get_property(env, instance, key, &val));
    int32_t v;
    NAPI_CALL(env, napi_get_value_int32(env, val, &v));
    CHECK_OR_FAIL(v == 99, "constructor: expected value=99");

    // Verify instance.wasNew == true (new.target was set)
    napi_value was_new_key;
    NAPI_CALL(env, napi_create_string_utf8(env, "wasNew", NAPI_AUTO_LENGTH, &was_new_key));
    napi_value was_new;
    NAPI_CALL(env, napi_get_property(env, instance, was_new_key, &was_new));
    bool was_new_val;
    NAPI_CALL(env, napi_get_value_bool(env, was_new, &was_new_val));
    CHECK_OR_FAIL(was_new_val == true, "constructor: expected wasNew=true");
  }

  // Test 2: napi_new_instance with no arguments
  {
    napi_value ctor_fn;
    NAPI_CALL(env, napi_create_function(env, "MyObj2", NAPI_AUTO_LENGTH,
                                        my_constructor, NULL, &ctor_fn));

    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor_fn, 0, NULL, &instance));

    // Verify instance.value == 42 (default)
    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "value", NAPI_AUTO_LENGTH, &key));
    napi_value val;
    NAPI_CALL(env, napi_get_property(env, instance, key, &val));
    int32_t v;
    NAPI_CALL(env, napi_get_value_int32(env, val, &v));
    CHECK_OR_FAIL(v == 42, "no-args constructor: expected value=42");
  }

  // Test 3: Multiple instances from same constructor
  {
    napi_value ctor_fn;
    NAPI_CALL(env, napi_create_function(env, "Multi", NAPI_AUTO_LENGTH,
                                        my_constructor, NULL, &ctor_fn));

    napi_value instances[3];
    for (int i = 0; i < 3; i++) {
      napi_value arg;
      NAPI_CALL(env, napi_create_int32(env, (i + 1) * 10, &arg));
      NAPI_CALL(env, napi_new_instance(env, ctor_fn, 1, &arg, &instances[i]));
    }

    // Verify each has correct value
    napi_value key;
    NAPI_CALL(env, napi_create_string_utf8(env, "value", NAPI_AUTO_LENGTH, &key));
    for (int i = 0; i < 3; i++) {
      napi_value val;
      NAPI_CALL(env, napi_get_property(env, instances[i], key, &val));
      int32_t v;
      NAPI_CALL(env, napi_get_value_int32(env, val, &v));
      CHECK_OR_FAIL(v == (i + 1) * 10, "multi-instance: value mismatch");
    }
  }

  // Test 4: instanceof check
  {
    napi_value ctor_fn;
    NAPI_CALL(env, napi_create_function(env, "InstCheck", NAPI_AUTO_LENGTH,
                                        my_constructor, NULL, &ctor_fn));

    napi_value instance;
    NAPI_CALL(env, napi_new_instance(env, ctor_fn, 0, NULL, &instance));

    bool is_inst;
    NAPI_CALL(env, napi_instanceof(env, instance, ctor_fn, &is_inst));
    CHECK_OR_FAIL(is_inst, "instanceof: expected true");

    // A plain object should not be an instance
    napi_value plain_obj;
    NAPI_CALL(env, napi_create_object(env, &plain_obj));
    NAPI_CALL(env, napi_instanceof(env, plain_obj, ctor_fn, &is_inst));
    CHECK_OR_FAIL(!is_inst, "instanceof: plain object should be false");
  }

  return PrintSuccess("TEST_CONSTRUCTOR");
}
