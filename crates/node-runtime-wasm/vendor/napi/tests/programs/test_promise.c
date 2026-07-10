#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: napi_create_promise creates a promise and deferred
  napi_deferred deferred;
  napi_value promise;
  NAPI_CALL(env, napi_create_promise(env, &deferred, &promise));
  CHECK_OR_FAIL(deferred != NULL, "deferred should not be NULL");
  CHECK_OR_FAIL(promise != NULL, "promise should not be NULL");

  // Test 2: napi_is_promise returns true for the promise
  bool is_promise;
  NAPI_CALL(env, napi_is_promise(env, promise, &is_promise));
  CHECK_OR_FAIL(is_promise, "napi_is_promise should return true for a promise");

  // Test 3: napi_is_promise returns false for a plain object
  napi_value obj;
  NAPI_CALL(env, napi_create_object(env, &obj));
  bool obj_is_promise;
  NAPI_CALL(env, napi_is_promise(env, obj, &obj_is_promise));
  CHECK_OR_FAIL(!obj_is_promise,
                "napi_is_promise should return false for a plain object");

  // Test 4: napi_resolve_deferred succeeds
  napi_value resolution;
  NAPI_CALL(env, napi_create_int32(env, 42, &resolution));
  NAPI_CALL(env, napi_resolve_deferred(env, deferred, resolution));

  return PrintSuccess("TEST_PROMISE");
}
