#include <stdio.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- Test napi_create_symbol with description ----
  {
    napi_value description;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "my_symbol", NAPI_AUTO_LENGTH,
                                      &description));
    napi_value sym;
    NAPI_CALL(env, napi_create_symbol(env, description, &sym));

    // typeof should be napi_symbol
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, sym, &vtype));
    CHECK_OR_FAIL(vtype == napi_symbol,
                  "symbol with description: expected napi_symbol");
  }

  // ---- Test napi_create_symbol without description (NULL) ----
  {
    napi_value sym;
    NAPI_CALL(env, napi_create_symbol(env, NULL, &sym));

    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, sym, &vtype));
    CHECK_OR_FAIL(vtype == napi_symbol,
                  "symbol without description: expected napi_symbol");
  }

  // ---- Test two symbols are not strict_equals ----
  {
    napi_value desc;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "same_desc", NAPI_AUTO_LENGTH,
                                      &desc));

    napi_value sym1, sym2;
    NAPI_CALL(env, napi_create_symbol(env, desc, &sym1));
    NAPI_CALL(env, napi_create_symbol(env, desc, &sym2));

    bool is_equal;
    NAPI_CALL(env, napi_strict_equals(env, sym1, sym2, &is_equal));
    CHECK_OR_FAIL(!is_equal,
                  "two symbols with same description should not be equal");
  }

  // ---- Test a symbol is strict_equals to itself ----
  {
    napi_value desc;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "self", NAPI_AUTO_LENGTH, &desc));

    napi_value sym;
    NAPI_CALL(env, napi_create_symbol(env, desc, &sym));

    bool is_equal;
    NAPI_CALL(env, napi_strict_equals(env, sym, sym, &is_equal));
    CHECK_OR_FAIL(is_equal,
                  "a symbol should be strict_equals to itself");
  }

  // ---- Test symbol is not equal to string with same content ----
  {
    napi_value desc;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "test", NAPI_AUTO_LENGTH, &desc));

    napi_value sym;
    NAPI_CALL(env, napi_create_symbol(env, desc, &sym));

    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "test", NAPI_AUTO_LENGTH, &str));

    bool is_equal;
    NAPI_CALL(env, napi_strict_equals(env, sym, str, &is_equal));
    CHECK_OR_FAIL(!is_equal,
                  "symbol should not be strict_equals to string");
  }

  // ---- Test symbol can be used as object property key ----
  {
    napi_value desc;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "prop_key", NAPI_AUTO_LENGTH,
                                      &desc));

    napi_value sym;
    NAPI_CALL(env, napi_create_symbol(env, desc, &sym));

    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));

    napi_value val;
    NAPI_CALL(env, napi_create_int32(env, 99, &val));
    NAPI_CALL(env, napi_set_property(env, obj, sym, val));

    napi_value got;
    NAPI_CALL(env, napi_get_property(env, obj, sym, &got));
    int32_t result;
    NAPI_CALL(env, napi_get_value_int32(env, got, &result));
    CHECK_OR_FAIL(result == 99,
                  "symbol property: expected value 99");
  }

  return PrintSuccess("TEST_SYMBOL");
}
