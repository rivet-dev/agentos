#include <stdio.h>
#include <string.h>
#include <stdint.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // Test 1: Create positive BigInt from words [1], read back
  {
    uint64_t words_in[] = { 1 };
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_words(env, 0, 1, words_in, &bigint));

    int sign;
    size_t word_count = 1;
    uint64_t words_out[1];
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, words_out));
    CHECK_OR_FAIL(sign == 0, "bigint_words: expected positive sign");
    CHECK_OR_FAIL(word_count == 1, "bigint_words: expected word_count 1");
    CHECK_OR_FAIL(words_out[0] == 1, "bigint_words: expected word[0] == 1");
  }

  // Test 2: Create negative BigInt from words [42]
  {
    uint64_t words_in[] = { 42 };
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_words(env, 1, 1, words_in, &bigint));

    int sign;
    size_t word_count = 1;
    uint64_t words_out[1];
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, words_out));
    CHECK_OR_FAIL(sign == 1, "bigint_words neg: expected negative sign");
    CHECK_OR_FAIL(words_out[0] == 42, "bigint_words neg: expected word[0] == 42");
  }

  // Test 3: Large BigInt from two words
  {
    uint64_t words_in[] = { 0xFFFFFFFFFFFFFFFFULL, 0x1 };
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_words(env, 0, 2, words_in, &bigint));

    int sign;
    size_t word_count = 2;
    uint64_t words_out[2];
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, words_out));
    CHECK_OR_FAIL(sign == 0, "bigint_words large: expected positive");
    CHECK_OR_FAIL(word_count == 2, "bigint_words large: expected 2 words");
    CHECK_OR_FAIL(words_out[0] == 0xFFFFFFFFFFFFFFFFULL,
                  "bigint_words large: word[0] mismatch");
    CHECK_OR_FAIL(words_out[1] == 0x1, "bigint_words large: word[1] mismatch");
  }

  // Test 4: Cross-API — create via int64, read via words
  {
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_int64(env, 12345, &bigint));

    int sign;
    size_t word_count = 0;
    // Query word count first
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, NULL));
    CHECK_OR_FAIL(word_count >= 1, "bigint_words cross-api: expected >= 1 word");

    uint64_t words_out[4];
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, words_out));
    CHECK_OR_FAIL(sign == 0, "bigint_words cross-api: expected positive");
    CHECK_OR_FAIL(words_out[0] == 12345, "bigint_words cross-api: expected 12345");
  }

  // Test 5: Zero BigInt
  {
    uint64_t words_in[] = { 0 };
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_words(env, 0, 1, words_in, &bigint));

    int sign;
    size_t word_count = 1;
    uint64_t words_out[1];
    NAPI_CALL(env, napi_get_value_bigint_words(env, bigint, &sign, &word_count, words_out));
    // Zero BigInt: sign should be 0
    CHECK_OR_FAIL(sign == 0, "bigint_words zero: expected positive sign");
  }

  // Test 6: Verify typeof is bigint
  {
    uint64_t words_in[] = { 999 };
    napi_value bigint;
    NAPI_CALL(env, napi_create_bigint_words(env, 0, 1, words_in, &bigint));

    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, bigint, &vtype));
    CHECK_OR_FAIL(vtype == napi_bigint, "bigint_words typeof: expected napi_bigint");
  }

  return PrintSuccess("TEST_BIGINT_WORDS");
}
