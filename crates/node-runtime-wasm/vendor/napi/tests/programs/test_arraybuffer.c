#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: napi_create_arraybuffer with a byte length
  size_t byte_length = 1024;
  napi_value arraybuffer;
  NAPI_CALL(env,
            napi_create_arraybuffer(env, byte_length, NULL, &arraybuffer));

  // Test 2: napi_is_arraybuffer returns true
  bool is_ab;
  NAPI_CALL(env, napi_is_arraybuffer(env, arraybuffer, &is_ab));
  CHECK_OR_FAIL(is_ab, "napi_is_arraybuffer should return true");

  // Test 3: napi_get_arraybuffer_info returns correct byte length
  // NOTE: Pass NULL for data pointer since it points to host memory
  // and cannot be dereferenced from WASM.
  size_t info_byte_length;
  NAPI_CALL(env,
            napi_get_arraybuffer_info(env, arraybuffer, NULL,
                                      &info_byte_length));
  CHECK_OR_FAIL(info_byte_length == byte_length,
                "arraybuffer info byte_length mismatch");

  // Test 4: napi_detach_arraybuffer
  NAPI_CALL(env, napi_detach_arraybuffer(env, arraybuffer));

  // Test 5: napi_is_detached_arraybuffer returns true after detach
  bool is_detached;
  NAPI_CALL(env,
            napi_is_detached_arraybuffer(env, arraybuffer, &is_detached));
  CHECK_OR_FAIL(is_detached,
                "napi_is_detached_arraybuffer should return true after detach");

  return PrintSuccess("TEST_ARRAYBUFFER");
}
