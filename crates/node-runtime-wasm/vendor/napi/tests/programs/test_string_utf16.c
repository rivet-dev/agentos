#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: Create a UTF-16 string from known data, read back via UTF-8
  {
    // "Hello" in UTF-16LE: H=0x0048, e=0x0065, l=0x006C, l=0x006C, o=0x006F
    char16_t hello[] = { 'H', 'e', 'l', 'l', 'o', 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, hello, 5, &str));

    // Read back via UTF-8
    char buf[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 5, "utf16 create: expected utf8 length 5");
    CHECK_OR_FAIL(strcmp(buf, "Hello") == 0, "utf16 create: expected 'Hello'");
  }

  // Test 2: Create with NAPI_AUTO_LENGTH (null-terminated)
  {
    char16_t world[] = { 'W', 'o', 'r', 'l', 'd', 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, world, NAPI_AUTO_LENGTH, &str));

    char buf[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf8(env, str, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(len == 5, "utf16 auto_length: expected utf8 length 5");
    CHECK_OR_FAIL(strcmp(buf, "World") == 0, "utf16 auto_length: expected 'World'");
  }

  // Test 3: Read back via napi_get_value_string_utf16
  {
    char16_t input[] = { 'A', 'B', 'C', 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, input, 3, &str));

    char16_t out[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf16(env, str, out, 64, &len));
    CHECK_OR_FAIL(len == 3, "utf16 readback: expected length 3");
    CHECK_OR_FAIL(out[0] == 'A' && out[1] == 'B' && out[2] == 'C',
                  "utf16 readback: expected 'ABC'");
  }

  // Test 4: Query length with NULL buffer
  {
    char16_t data[] = { 'T', 'e', 's', 't', 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, data, 4, &str));

    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf16(env, str, NULL, 0, &len));
    CHECK_OR_FAIL(len == 4, "utf16 query_length: expected 4");
  }

  // Test 5: Empty string
  {
    char16_t empty[] = { 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, empty, 0, &str));

    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf16(env, str, NULL, 0, &len));
    CHECK_OR_FAIL(len == 0, "utf16 empty: expected length 0");
  }

  // Test 6: Non-ASCII characters (BMP range)
  // U+00E9 (é), U+00F1 (ñ), U+00FC (ü)
  {
    char16_t special[] = { 0x00E9, 0x00F1, 0x00FC, 0 };
    napi_value str;
    NAPI_CALL(env, napi_create_string_utf16(env, special, 3, &str));

    char16_t out[64];
    size_t len;
    NAPI_CALL(env, napi_get_value_string_utf16(env, str, out, 64, &len));
    CHECK_OR_FAIL(len == 3, "utf16 non-ascii: expected length 3");
    CHECK_OR_FAIL(out[0] == 0x00E9 && out[1] == 0x00F1 && out[2] == 0x00FC,
                  "utf16 non-ascii: expected matching characters");
  }

  return PrintSuccess("TEST_STRING_UTF16");
}
