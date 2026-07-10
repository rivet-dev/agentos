#include <stdio.h>
#include <stdint.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create an external value wrapping an integer cast to void*
  void* data = (void*)(uintptr_t)42;
  napi_value ext_val;
  NAPI_CALL(env,
            napi_create_external(env, data, NULL, NULL, &ext_val));

  // Verify typeof returns napi_external
  napi_valuetype vtype;
  NAPI_CALL(env, napi_typeof(env, ext_val, &vtype));
  CHECK_OR_FAIL(vtype == napi_external, "expected napi_external");

  // Retrieve the data pointer and verify it matches
  void* retrieved_data = NULL;
  NAPI_CALL(env, napi_get_value_external(env, ext_val, &retrieved_data));
  CHECK_OR_FAIL(retrieved_data == data,
                "napi_get_value_external returned wrong data pointer");
  CHECK_OR_FAIL((uintptr_t)retrieved_data == 42,
                "external data value mismatch");

  return PrintSuccess("TEST_EXTERNAL");
}
