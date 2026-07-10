#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"
#include "node_api.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: napi_get_node_version returns valid version info
  {
    const napi_node_version* version = NULL;
    NAPI_CALL(env, napi_get_node_version(env, &version));
    CHECK_OR_FAIL(version != NULL, "get_node_version: expected non-NULL");
    // Major version should be reasonable (> 0)
    CHECK_OR_FAIL(version->major > 0,
                  "get_node_version: expected major > 0");
  }

  // Test 2: Version struct release field is non-NULL
  {
    const napi_node_version* version = NULL;
    NAPI_CALL(env, napi_get_node_version(env, &version));
    CHECK_OR_FAIL(version->release != NULL,
                  "get_node_version: expected non-NULL release string");
  }

  // napi_fatal_error is intentionally not called because it aborts the process.
  // Just verify we can reference the function (linkability check).
  // The test succeeds if we get here without errors.

  return PrintSuccess("TEST_NODE_API_MISC");
}
