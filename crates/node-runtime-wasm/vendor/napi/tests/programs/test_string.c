#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- Test napi_create_string_utf8 with explicit length ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "Hello, World!", 5, &str));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 5, "explicit length: expected len 5");
    CHECK_OR_FAIL(strcmp(buf, "Hello") == 0,
                  "explicit length: expected 'Hello'");
  }

  // ---- Test napi_create_string_utf8 with NAPI_AUTO_LENGTH ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "auto length test",
                                      NAPI_AUTO_LENGTH, &str));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 16, "auto length: expected len 16");
    CHECK_OR_FAIL(strcmp(buf, "auto length test") == 0,
                  "auto length: string mismatch");
  }

  // ---- Test napi_create_string_utf8 with zero length (empty string) ----
  {
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &str));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 0, "empty string: expected len 0");
    CHECK_OR_FAIL(strcmp(buf, "") == 0, "empty string: expected ''");
  }

  // ---- Test napi_get_value_string_utf8 with NULL buffer (length query) ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "measure me", NAPI_AUTO_LENGTH,
                                      &str));
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, str, NULL, 0, &len));
    CHECK_OR_FAIL(len == 10, "length query: expected len 10");
  }

  // ---- Test buffer-too-small truncation behavior ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "truncate me", NAPI_AUTO_LENGTH,
                                      &str));
    // Buffer of 6 bytes: room for 5 chars + null terminator
    char buf[6];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    // Should write 5 chars + null terminator, len reports chars copied
    CHECK_OR_FAIL(len == 5, "truncation: expected copied len 5");
    CHECK_OR_FAIL(strcmp(buf, "trunc") == 0,
                  "truncation: expected 'trunc'");
  }

  // ---- Test napi_create_string_latin1 and napi_get_value_string_latin1 ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_latin1(env, "Latin1 test", NAPI_AUTO_LENGTH,
                                        &str));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_latin1(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 11, "latin1: expected len 11");
    CHECK_OR_FAIL(strcmp(buf, "Latin1 test") == 0,
                  "latin1: string mismatch");
  }

  // ---- Test latin1 with explicit length ----
  {
    napi_value str;
    NAPI_CALL(env, napi_create_string_latin1(env, "abcdef", 3, &str));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_latin1(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 3, "latin1 explicit: expected len 3");
    CHECK_OR_FAIL(strcmp(buf, "abc") == 0,
                  "latin1 explicit: expected 'abc'");
  }

  // ---- Test typeof string value ----
  {
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "typeof test", NAPI_AUTO_LENGTH,
                                      &str));
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, str, &vtype));
    CHECK_OR_FAIL(vtype == napi_string, "typeof: expected napi_string");
  }

  return PrintSuccess("TEST_STRING");
}
