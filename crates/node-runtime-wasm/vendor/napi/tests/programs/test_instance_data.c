#include <stdio.h>
#include <stdint.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: get_instance_data before setting — should return NULL
  {
    void* data = (void*)0xDEAD;
    NAPI_CALL(env, napi_get_instance_data(env, &data));
    CHECK_OR_FAIL(data == NULL, "instance_data: expected NULL before set");
  }

  // Test 2: set_instance_data, then get_instance_data
  {
    void* my_data = (void*)(uintptr_t)0x12345678;
    NAPI_CALL(env, napi_set_instance_data(env, my_data, NULL, NULL));

    void* retrieved = NULL;
    NAPI_CALL(env, napi_get_instance_data(env, &retrieved));
    CHECK_OR_FAIL(retrieved == my_data,
                  "instance_data: expected same pointer back");
  }

  // Test 3: overwrite with new data
  {
    void* new_data = (void*)(uintptr_t)0xABCDEF00;
    NAPI_CALL(env, napi_set_instance_data(env, new_data, NULL, NULL));

    void* retrieved = NULL;
    NAPI_CALL(env, napi_get_instance_data(env, &retrieved));
    CHECK_OR_FAIL(retrieved == new_data,
                  "instance_data overwrite: expected new pointer");
  }

  // Test 4: adjust_external_memory with positive value
  {
    int64_t adjusted = 0;
    NAPI_CALL(env, napi_adjust_external_memory(env, 1024, &adjusted));
    // adjusted_value should be >= 0 (it's a hint to the GC)
    CHECK_OR_FAIL(adjusted >= 0,
                  "adjust_external_memory positive: expected >= 0");
  }

  // Test 5: adjust_external_memory with negative value
  {
    int64_t adjusted = 0;
    NAPI_CALL(env, napi_adjust_external_memory(env, -512, &adjusted));
    CHECK_OR_FAIL(adjusted >= 0,
                  "adjust_external_memory negative: expected >= 0");
  }

  // Test 6: adjust_external_memory with zero
  {
    int64_t adjusted = -1;
    NAPI_CALL(env, napi_adjust_external_memory(env, 0, &adjusted));
    CHECK_OR_FAIL(adjusted >= 0,
                  "adjust_external_memory zero: expected >= 0");
  }

  return PrintSuccess("TEST_INSTANCE_DATA");
}
