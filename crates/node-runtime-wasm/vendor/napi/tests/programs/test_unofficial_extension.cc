#include <cstdint>

#include "napi_test_helpers.h"
#include "unofficial_napi.h"

int main() {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != nullptr, "napi_wasm_init_env returned NULL");

  uint64_t hash_seed = 0;
  NAPI_CALL(env, unofficial_napi_get_hash_seed(env, &hash_seed));
  (void)hash_seed;

  return PrintSuccess("TEST_UNOFFICIAL_EXTENSION");
}
