#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"
#include "node_api.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: Create buffer, verify is_buffer
  {
    void* data = NULL;
    napi_value buf;
    NAPI_CALL(env, napi_create_buffer(env, 10, &data, &buf));

    bool is_buf;
    NAPI_CALL(env, napi_is_buffer(env, buf, &is_buf));
    CHECK_OR_FAIL(is_buf, "create_buffer: expected is_buffer true");
  }

  // Test 2: Get buffer info — length and data
  {
    void* data = NULL;
    napi_value buf;
    NAPI_CALL(env, napi_create_buffer(env, 16, &data, &buf));

    void* info_data = NULL;
    size_t info_len = 0;
    NAPI_CALL(env, napi_get_buffer_info(env, buf, &info_data, &info_len));
    CHECK_OR_FAIL(info_len == 16, "buffer_info: expected length 16");
  }

  // Test 3: Write into buffer, read back via buffer_info
  {
    void* data = NULL;
    napi_value buf;
    NAPI_CALL(env, napi_create_buffer(env, 5, &data, &buf));

    if (data) {
      memcpy(data, "Hello", 5);
    }

    void* info_data = NULL;
    size_t info_len = 0;
    NAPI_CALL(env, napi_get_buffer_info(env, buf, &info_data, &info_len));
    CHECK_OR_FAIL(info_len == 5, "buffer write/read: expected length 5");
    if (info_data) {
      CHECK_OR_FAIL(memcmp(info_data, "Hello", 5) == 0,
                    "buffer write/read: data mismatch");
    }
  }

  // Test 4: Create buffer copy from known data
  {
    const char* src = "World";
    void* result_data = NULL;
    napi_value buf;
    NAPI_CALL(env, napi_create_buffer_copy(env, 5, src, &result_data, &buf));

    bool is_buf;
    NAPI_CALL(env, napi_is_buffer(env, buf, &is_buf));
    CHECK_OR_FAIL(is_buf, "buffer_copy: expected is_buffer true");

    void* info_data = NULL;
    size_t info_len = 0;
    NAPI_CALL(env, napi_get_buffer_info(env, buf, &info_data, &info_len));
    CHECK_OR_FAIL(info_len == 5, "buffer_copy: expected length 5");
    if (info_data) {
      CHECK_OR_FAIL(memcmp(info_data, "World", 5) == 0,
                    "buffer_copy: data mismatch");
    }
  }

  // Test 5: is_buffer returns false for plain object
  {
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    bool is_buf;
    NAPI_CALL(env, napi_is_buffer(env, obj, &is_buf));
    CHECK_OR_FAIL(!is_buf, "is_buffer object: expected false");
  }

  // Test 6: is_buffer returns false for ArrayBuffer
  {
    void* data = NULL;
    napi_value ab;
    NAPI_CALL(env, napi_create_arraybuffer(env, 10, &data, &ab));

    bool is_buf;
    NAPI_CALL(env, napi_is_buffer(env, ab, &is_buf));
    CHECK_OR_FAIL(!is_buf, "is_buffer arraybuffer: expected false");
  }

  // Test 7: Zero-length buffer
  {
    void* data = NULL;
    napi_value buf;
    NAPI_CALL(env, napi_create_buffer(env, 0, &data, &buf));

    bool is_buf;
    NAPI_CALL(env, napi_is_buffer(env, buf, &is_buf));
    CHECK_OR_FAIL(is_buf, "zero buffer: expected is_buffer true");

    void* info_data = NULL;
    size_t info_len = 99;
    NAPI_CALL(env, napi_get_buffer_info(env, buf, &info_data, &info_len));
    CHECK_OR_FAIL(info_len == 0, "zero buffer: expected length 0");
  }

  return PrintSuccess("TEST_BUFFER");
}
