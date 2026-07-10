#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: napi_create_date with a known timestamp
  double timestamp = 1234567890.0;
  napi_value date_val;
  NAPI_CALL(env, napi_create_date(env, timestamp, &date_val));

  // Test 2: napi_is_date returns true for dates
  bool is_date;
  NAPI_CALL(env, napi_is_date(env, date_val, &is_date));
  CHECK_OR_FAIL(is_date, "napi_is_date should return true for a date");

  // Test 3: napi_is_date returns false for a plain number
  napi_value num_val;
  NAPI_CALL(env, napi_create_double(env, 1234567890.0, &num_val));
  bool num_is_date;
  NAPI_CALL(env, napi_is_date(env, num_val, &num_is_date));
  CHECK_OR_FAIL(!num_is_date, "napi_is_date should return false for a number");

  // Test 4: napi_get_date_value retrieves the same timestamp
  double retrieved;
  NAPI_CALL(env, napi_get_date_value(env, date_val, &retrieved));
  CHECK_OR_FAIL(retrieved == timestamp,
                "napi_get_date_value should return original timestamp");

  return PrintSuccess("TEST_DATE");
}
