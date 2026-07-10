#include <stdio.h>
#include <stdint.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: napi_create_bigint_int64 / napi_get_value_bigint_int64 roundtrip
  napi_value bigint_i64;
  NAPI_CALL(env, napi_create_bigint_int64(env, -12345678901LL, &bigint_i64));

  int64_t i64_result;
  bool lossless_i64;
  NAPI_CALL(env, napi_get_value_bigint_int64(env, bigint_i64, &i64_result,
                                              &lossless_i64));
  CHECK_OR_FAIL(i64_result == -12345678901LL,
                "bigint int64 roundtrip value mismatch");
  CHECK_OR_FAIL(lossless_i64, "bigint int64 lossless should be true");

  // Test 2: napi_create_bigint_uint64 / napi_get_value_bigint_uint64 roundtrip
  napi_value bigint_u64;
  NAPI_CALL(env, napi_create_bigint_uint64(env, 9876543210ULL, &bigint_u64));

  uint64_t u64_result;
  bool lossless_u64;
  NAPI_CALL(env, napi_get_value_bigint_uint64(env, bigint_u64, &u64_result,
                                               &lossless_u64));
  CHECK_OR_FAIL(u64_result == 9876543210ULL,
                "bigint uint64 roundtrip value mismatch");
  CHECK_OR_FAIL(lossless_u64, "bigint uint64 lossless should be true");

  // Test 3: napi_typeof returns napi_bigint (9) for bigint values
  napi_valuetype vtype;
  NAPI_CALL(env, napi_typeof(env, bigint_i64, &vtype));
  CHECK_OR_FAIL(vtype == napi_bigint, "expected napi_bigint for int64 bigint");

  NAPI_CALL(env, napi_typeof(env, bigint_u64, &vtype));
  CHECK_OR_FAIL(vtype == napi_bigint, "expected napi_bigint for uint64 bigint");

  return PrintSuccess("TEST_BIGINT");
}
