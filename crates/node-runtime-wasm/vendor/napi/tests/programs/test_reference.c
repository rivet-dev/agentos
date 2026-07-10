#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create an object to hold a reference to
  napi_value obj;
  NAPI_CALL(env, napi_create_object(env, &obj));

  // Set a property so we can verify the object identity later
  napi_value marker;
  NAPI_CALL(env, napi_create_int32(env, 42, &marker));
  NAPI_CALL(env, napi_set_named_property(env, obj, "marker", marker));

  // Test 1: napi_create_reference with initial refcount 1
  napi_ref ref;
  NAPI_CALL(env, napi_create_reference(env, obj, 1, &ref));
  CHECK_OR_FAIL(ref != NULL, "napi_create_reference should return non-NULL ref");

  // Test 2: napi_get_reference_value retrieves the original value
  napi_value retrieved;
  NAPI_CALL(env, napi_get_reference_value(env, ref, &retrieved));
  CHECK_OR_FAIL(retrieved != NULL,
                "napi_get_reference_value should return non-NULL");

  // Verify it is the same object by reading the marker property
  napi_value got_marker;
  NAPI_CALL(env, napi_get_named_property(env, retrieved, "marker", &got_marker));
  int32_t marker_val;
  NAPI_CALL(env, napi_get_value_int32(env, got_marker, &marker_val));
  CHECK_OR_FAIL(marker_val == 42, "reference should point to the same object");

  // Test 3: napi_reference_ref increments the reference count
  uint32_t ref_count;
  NAPI_CALL(env, napi_reference_ref(env, ref, &ref_count));
  CHECK_OR_FAIL(ref_count == 2,
                "ref count should be 2 after incrementing from 1");

  // Test 4: napi_reference_unref decrements the reference count
  uint32_t unref_count;
  NAPI_CALL(env, napi_reference_unref(env, ref, &unref_count));
  CHECK_OR_FAIL(unref_count == 1,
                "ref count should be 1 after decrementing from 2");

  // Test 5: napi_delete_reference succeeds
  NAPI_CALL(env, napi_delete_reference(env, ref));

  return PrintSuccess("TEST_REFERENCE");
}
