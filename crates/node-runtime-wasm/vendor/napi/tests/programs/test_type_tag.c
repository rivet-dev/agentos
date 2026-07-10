#include <stdio.h>
#include <stdbool.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Define two distinct type tags
  const napi_type_tag tag_a = { 0x1234567890abcdefULL, 0xfedcba0987654321ULL };
  const napi_type_tag tag_b = { 0xaabbccdd11223344ULL, 0x5566778899aabbccULL };

  // Create an object and tag it with tag_a
  napi_value obj1;
  NAPI_CALL(env, napi_create_object(env, &obj1));
  NAPI_CALL(env, napi_type_tag_object(env, obj1, &tag_a));

  // Check that tag_a matches
  bool matches = false;
  NAPI_CALL(env, napi_check_object_type_tag(env, obj1, &tag_a, &matches));
  CHECK_OR_FAIL(matches == true, "tag_a should match obj1");

  // Check that tag_b does NOT match
  matches = true;
  NAPI_CALL(env, napi_check_object_type_tag(env, obj1, &tag_b, &matches));
  CHECK_OR_FAIL(matches == false, "tag_b should not match obj1");

  // Create a second object and tag it with tag_b
  napi_value obj2;
  NAPI_CALL(env, napi_create_object(env, &obj2));
  NAPI_CALL(env, napi_type_tag_object(env, obj2, &tag_b));

  // Verify tag_b matches obj2
  matches = false;
  NAPI_CALL(env, napi_check_object_type_tag(env, obj2, &tag_b, &matches));
  CHECK_OR_FAIL(matches == true, "tag_b should match obj2");

  // Verify tag_a does NOT match obj2
  matches = true;
  NAPI_CALL(env, napi_check_object_type_tag(env, obj2, &tag_a, &matches));
  CHECK_OR_FAIL(matches == false, "tag_a should not match obj2");

  return PrintSuccess("TEST_TYPE_TAG");
}
