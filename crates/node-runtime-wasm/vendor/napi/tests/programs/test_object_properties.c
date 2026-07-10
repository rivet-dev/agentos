#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- Test napi_has_property ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "foo", NAPI_AUTO_LENGTH, &key));
    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 10, &val));
    NAPI_CALL(env, napi_set_property(env, obj, key, val));

    bool has;
    NAPI_CALL(env, napi_has_property(env, obj, key, &has));
    CHECK_OR_FAIL(has, "has_property: expected true for 'foo'");

    napi_value missing_key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "bar", NAPI_AUTO_LENGTH,
                                      &missing_key));
    NAPI_CALL(env, napi_has_property(env, obj, missing_key, &has));
    CHECK_OR_FAIL(!has, "has_property: expected false for 'bar'");
  }

  // ---- Test napi_has_own_property ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "own", NAPI_AUTO_LENGTH, &key));
    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 1, &val));
    NAPI_CALL(env, napi_set_property(env, obj, key, val));

    bool has;
    NAPI_CALL(env, napi_has_own_property(env, obj, key, &has));
    CHECK_OR_FAIL(has, "has_own_property: expected true for 'own'");

    // "toString" is inherited, not own
    napi_value inherited_key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "toString", NAPI_AUTO_LENGTH,
                                      &inherited_key));
    NAPI_CALL(env, napi_has_own_property(env, obj, inherited_key, &has));
    CHECK_OR_FAIL(!has,
                  "has_own_property: expected false for inherited 'toString'");
  }

  // ---- Test napi_delete_property ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "deleteme", NAPI_AUTO_LENGTH,
                                      &key));
    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 42, &val));
    NAPI_CALL(env, napi_set_property(env, obj, key, val));

    // Verify it exists
    bool has;
    NAPI_CALL(env, napi_has_property(env, obj, key, &has));
    CHECK_OR_FAIL(has, "delete_property: property should exist before delete");

    // Delete it
    bool deleted;
    NAPI_CALL(env, napi_delete_property(env, obj, key, &deleted));
    CHECK_OR_FAIL(deleted, "delete_property: expected true (deleted)");

    // Verify it no longer exists
    NAPI_CALL(env, napi_has_property(env, obj, key, &has));
    CHECK_OR_FAIL(!has, "delete_property: property should not exist after delete");
  }

  // ---- Test napi_has_named_property ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 5, &val));
    NAPI_CALL(env, napi_set_named_property(env, obj, "named", val));

    bool has;
    NAPI_CALL(env, napi_has_named_property(env, obj, "named", &has));
    CHECK_OR_FAIL(has, "has_named_property: expected true for 'named'");

    NAPI_CALL(env, napi_has_named_property(env, obj, "missing", &has));
    CHECK_OR_FAIL(!has, "has_named_property: expected false for 'missing'");
  }

  // ---- Test napi_has_element and napi_delete_element ----
  {
    napi_value arr;
    NAPI_CALL(env, napi_create_array(env, &arr));

    napi_value v0, v1;
    NAPI_CALL(env, napi_create_int32(env, 100, &v0));
    NAPI_CALL(env, napi_create_int32(env, 200, &v1));
    NAPI_CALL(env, napi_set_element(env, arr, 0, v0));
    NAPI_CALL(env, napi_set_element(env, arr, 1, v1));

    bool has;
    NAPI_CALL(env, napi_has_element(env, arr, 0, &has));
    CHECK_OR_FAIL(has, "has_element: expected true for index 0");

    NAPI_CALL(env, napi_has_element(env, arr, 1, &has));
    CHECK_OR_FAIL(has, "has_element: expected true for index 1");

    NAPI_CALL(env, napi_has_element(env, arr, 5, &has));
    CHECK_OR_FAIL(!has, "has_element: expected false for index 5");

    // Delete element at index 0
    bool deleted;
    NAPI_CALL(env, napi_delete_element(env, arr, 0, &deleted));
    CHECK_OR_FAIL(deleted, "delete_element: expected true");

    // After delete, element 0 should not be present (becomes a hole)
    NAPI_CALL(env, napi_has_element(env, arr, 0, &has));
    CHECK_OR_FAIL(!has,
                  "delete_element: expected false for index 0 after delete");
  }

  // ---- Test napi_get_property_names ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value va, vb, vc;
    NAPI_CALL(env, napi_create_int32(env, 1, &va));
    NAPI_CALL(env, napi_create_int32(env, 2, &vb));
    NAPI_CALL(env, napi_create_int32(env, 3, &vc));
    NAPI_CALL(env, napi_set_named_property(env, obj, "a", va));
    NAPI_CALL(env, napi_set_named_property(env, obj, "b", vb));
    NAPI_CALL(env, napi_set_named_property(env, obj, "c", vc));

    napi_value names;
    NAPI_CALL(env, napi_get_property_names(env, obj, &names));

    // names should be an array
    bool is_arr;
    NAPI_CALL(env, napi_is_array(env, names, &is_arr));
    CHECK_OR_FAIL(is_arr, "get_property_names: expected array");

    uint32_t length;
    NAPI_CALL(env, napi_get_array_length(env, names, &length));
    CHECK_OR_FAIL(length == 3,
                  "get_property_names: expected 3 property names");

    // Read each name and verify
    int found_a = 0, found_b = 0, found_c = 0;
    for (uint32_t i = 0; i < length; i++) {
      napi_value name;
      NAPI_CALL(env, napi_get_element(env, names, i, &name));
      char buf[256];
      size_t len;
      NAPI_CALL(env,
                napi_get_value_string_utf8(env, name, buf, sizeof(buf), &len));
      if (strcmp(buf, "a") == 0) found_a = 1;
      else if (strcmp(buf, "b") == 0) found_b = 1;
      else if (strcmp(buf, "c") == 0) found_c = 1;
    }
    CHECK_OR_FAIL(found_a && found_b && found_c,
                  "get_property_names: missing expected property names");
  }

  // ---- Test napi_object_freeze (NAPI_VERSION >= 8) ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 10, &val));
    NAPI_CALL(env, napi_set_named_property(env, obj, "frozen_prop", val));

    // Freeze the object
    NAPI_CALL(env, napi_object_freeze(env, obj));

    // The existing property should still be readable
    napi_value got;
    NAPI_CALL(env, napi_get_named_property(env, obj, "frozen_prop", &got));
    int32_t result;
    NAPI_CALL(env, napi_get_value_int32(env, got, &result));
    CHECK_OR_FAIL(result == 10,
                  "object_freeze: existing property should be readable");

    // After freeze, trying to set a new property in strict mode would throw,
    // but in non-strict it silently fails. Verify original value unchanged.
    napi_value new_val;
    NAPI_CALL(env, napi_create_int32(env, 99, &new_val));
    // Attempt to overwrite -- this may or may not produce an error depending
    // on the engine's strict mode behavior, so we just verify the value
    // hasn't changed afterward.
    napi_set_named_property(env, obj, "frozen_prop", new_val);

    // Clear any pending exception from the failed set
    bool is_pending;
    NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
    if (is_pending) {
      napi_value exc;
      NAPI_CALL(env, napi_get_and_clear_last_exception(env, &exc));
    }

    NAPI_CALL(env, napi_get_named_property(env, obj, "frozen_prop", &got));
    NAPI_CALL(env, napi_get_value_int32(env, got, &result));
    CHECK_OR_FAIL(result == 10,
                  "object_freeze: property should remain unchanged after freeze");
  }

  // ---- Test napi_object_seal (NAPI_VERSION >= 8) ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 20, &val));
    NAPI_CALL(env, napi_set_named_property(env, obj, "sealed_prop", val));

    // Seal the object
    NAPI_CALL(env, napi_object_seal(env, obj));

    // Existing property should still be readable
    napi_value got;
    NAPI_CALL(env, napi_get_named_property(env, obj, "sealed_prop", &got));
    int32_t result;
    NAPI_CALL(env, napi_get_value_int32(env, got, &result));
    CHECK_OR_FAIL(result == 20,
                  "object_seal: existing property should be readable");

    // Sealed objects allow modifying existing properties (unlike frozen)
    napi_value new_val;
    NAPI_CALL(env, napi_create_int32(env, 30, &new_val));
    NAPI_CALL(env, napi_set_named_property(env, obj, "sealed_prop", new_val));

    NAPI_CALL(env, napi_get_named_property(env, obj, "sealed_prop", &got));
    NAPI_CALL(env, napi_get_value_int32(env, got, &result));
    CHECK_OR_FAIL(result == 30,
                  "object_seal: existing property should be modifiable");

    // Adding new properties should fail silently or throw
    napi_value extra;
    NAPI_CALL(env, napi_create_int32(env, 50, &extra));
    napi_set_named_property(env, obj, "new_prop", extra);

    // Clear any pending exception
    bool is_pending;
    NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
    if (is_pending) {
      napi_value exc;
      NAPI_CALL(env, napi_get_and_clear_last_exception(env, &exc));
    }

    // new_prop should not exist on the sealed object
    bool has;
    NAPI_CALL(env, napi_has_named_property(env, obj, "new_prop", &has));
    CHECK_OR_FAIL(!has,
                  "object_seal: should not be able to add new properties");
  }

  // ---- Test napi_get_prototype ----
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value proto;
    NAPI_CALL(env, napi_get_prototype(env, obj, &proto));

    // The prototype of a plain object should be Object.prototype (an object)
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, proto, &vtype));
    CHECK_OR_FAIL(vtype == napi_object,
                  "get_prototype: expected prototype to be an object");

    // Object.prototype should have "toString" as an own property
    napi_value ts_key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "toString", NAPI_AUTO_LENGTH,
                                      &ts_key));
    bool has;
    NAPI_CALL(env, napi_has_own_property(env, proto, ts_key, &has));
    CHECK_OR_FAIL(has,
                  "get_prototype: Object.prototype should have 'toString'");
  }

  // ---- Test napi_delete_property returns false for non-configurable ----
  {
    // Properties on frozen objects are non-configurable
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 1, &val));
    NAPI_CALL(env, napi_set_named_property(env, obj, "keep", val));
    NAPI_CALL(env, napi_object_freeze(env, obj));

    napi_value key;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "keep", NAPI_AUTO_LENGTH, &key));
    bool deleted;
    napi_status status = napi_delete_property(env, obj, key, &deleted);

    // Clear any pending exception
    bool is_pending;
    NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
    if (is_pending) {
      napi_value exc;
      NAPI_CALL(env, napi_get_and_clear_last_exception(env, &exc));
    }

    // Either the call fails or deleted is false
    if (status == napi_ok) {
      CHECK_OR_FAIL(!deleted,
                    "delete_property: non-configurable property should not be deleted");
    }
    // If status != napi_ok, that is also acceptable (strict mode throw)
  }

  return PrintSuccess("TEST_OBJECT_PROPERTIES");
}
