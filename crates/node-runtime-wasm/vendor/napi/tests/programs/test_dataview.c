#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Create a backing ArrayBuffer (128 bytes)
  size_t ab_byte_length = 128;
  napi_value arraybuffer;
  NAPI_CALL(env,
            napi_create_arraybuffer(env, ab_byte_length, NULL, &arraybuffer));

  // Test 1: napi_create_dataview
  size_t dv_byte_length = 64;
  size_t dv_byte_offset = 32;
  napi_value dataview;
  NAPI_CALL(env, napi_create_dataview(env, dv_byte_length, arraybuffer,
                                       dv_byte_offset, &dataview));

  // Test 2: napi_is_dataview returns true for dataview
  bool is_dv;
  NAPI_CALL(env, napi_is_dataview(env, dataview, &is_dv));
  CHECK_OR_FAIL(is_dv, "napi_is_dataview should return true for a dataview");

  // Test 3: napi_is_dataview returns false for arraybuffer
  bool ab_is_dv;
  NAPI_CALL(env, napi_is_dataview(env, arraybuffer, &ab_is_dv));
  CHECK_OR_FAIL(!ab_is_dv,
                "napi_is_dataview should return false for an arraybuffer");

  // Test 4: napi_get_dataview_info returns correct byte_length and offset
  size_t info_byte_length;
  napi_value info_arraybuffer;
  size_t info_byte_offset;
  NAPI_CALL(env, napi_get_dataview_info(env, dataview, &info_byte_length,
                                         NULL, &info_arraybuffer,
                                         &info_byte_offset));
  CHECK_OR_FAIL(info_byte_length == dv_byte_length,
                "dataview info byte_length mismatch");
  CHECK_OR_FAIL(info_byte_offset == dv_byte_offset,
                "dataview info byte_offset mismatch");

  // Verify the arraybuffer from dataview info is actually an arraybuffer
  bool info_is_ab;
  NAPI_CALL(env, napi_is_arraybuffer(env, info_arraybuffer, &info_is_ab));
  CHECK_OR_FAIL(info_is_ab,
                "arraybuffer from dataview info should be an arraybuffer");

  return PrintSuccess("TEST_DATAVIEW");
}
