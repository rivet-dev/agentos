#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create an object
  napi_value obj;
  NAPI_CALL(env, napi_create_object(env, &obj));

  // Set a named string property
  napi_value str_val;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "world", NAPI_AUTO_LENGTH, &str_val));
  NAPI_CALL(env, napi_set_named_property(env, obj, "hello", str_val));

  // Get it back
  napi_value got_val;
  NAPI_CALL(env, napi_get_named_property(env, obj, "hello", &got_val));
  char buf[256];
  size_t len;
  NAPI_CALL(env, napi_get_value_string_utf8(env, got_val, buf, sizeof(buf), &len));
  CHECK_OR_FAIL(strcmp(buf, "world") == 0, "named property string mismatch");

  // Set a numeric property
  napi_value num_val;
  NAPI_CALL(env, napi_create_int32(env, 99, &num_val));
  NAPI_CALL(env, napi_set_named_property(env, obj, "count", num_val));

  // Get it back
  napi_value got_num;
  NAPI_CALL(env, napi_get_named_property(env, obj, "count", &got_num));
  int32_t count;
  NAPI_CALL(env, napi_get_value_int32(env, got_num, &count));
  CHECK_OR_FAIL(count == 99, "named property int mismatch");

  // Test property-based set/get (with napi_value key)
  napi_value key;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "key1", NAPI_AUTO_LENGTH, &key));
  napi_value val;
  NAPI_CALL(env,
            napi_create_string_utf8(env, "val1", NAPI_AUTO_LENGTH, &val));
  NAPI_CALL(env, napi_set_property(env, obj, key, val));

  napi_value got_prop;
  NAPI_CALL(env, napi_get_property(env, obj, key, &got_prop));
  char buf2[256];
  size_t len2;
  NAPI_CALL(env,
            napi_get_value_string_utf8(env, got_prop, buf2, sizeof(buf2), &len2));
  CHECK_OR_FAIL(strcmp(buf2, "val1") == 0, "property roundtrip mismatch");

  return PrintSuccess("OBJECT_ROUNDTRIP_TEST");
}
