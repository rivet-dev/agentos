#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create a backing ArrayBuffer (256 bytes)
  size_t ab_byte_length = 256;
  napi_value arraybuffer;
  NAPI_CALL(env,
            napi_create_arraybuffer(env, ab_byte_length, NULL, &arraybuffer));

  // Test 1: napi_create_typedarray (Uint8Array, type = napi_uint8_array = 1)
  size_t ta_length = 64;
  size_t ta_byte_offset = 16;
  napi_value typedarray;
  NAPI_CALL(env, napi_create_typedarray(env, napi_uint8_array, ta_length,
                                         arraybuffer, ta_byte_offset,
                                         &typedarray));

  // Test 2: napi_is_typedarray returns true
  bool is_ta;
  NAPI_CALL(env, napi_is_typedarray(env, typedarray, &is_ta));
  CHECK_OR_FAIL(is_ta, "napi_is_typedarray should return true");

  // Test 3: napi_get_typedarray_info returns correct type, length, offset
  // NOTE: Pass NULL for data pointer since it points to host memory
  // and cannot be dereferenced from WASM.
  napi_typedarray_type ta_type;
  size_t info_length;
  napi_value info_arraybuffer;
  size_t info_byte_offset;
  NAPI_CALL(env, napi_get_typedarray_info(env, typedarray, &ta_type,
                                           &info_length, NULL,
                                           &info_arraybuffer,
                                           &info_byte_offset));
  CHECK_OR_FAIL(ta_type == napi_uint8_array,
                "typedarray type should be napi_uint8_array");
  CHECK_OR_FAIL(info_length == ta_length,
                "typedarray info length mismatch");
  CHECK_OR_FAIL(info_byte_offset == ta_byte_offset,
                "typedarray info byte_offset mismatch");

  // Verify the arraybuffer returned from info is also an arraybuffer
  bool info_is_ab;
  NAPI_CALL(env, napi_is_arraybuffer(env, info_arraybuffer, &info_is_ab));
  CHECK_OR_FAIL(info_is_ab,
                "arraybuffer from typedarray info should be an arraybuffer");

  return PrintSuccess("TEST_TYPEDARRAY");
}
