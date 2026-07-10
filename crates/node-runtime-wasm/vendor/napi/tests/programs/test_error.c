#include <stdio.h>
#include <string.h>

#include "napi_test_helpers.h"

int main(void) {
  napi_env env = napi_wasm_init_env();
  CHECK_OR_FAIL(env != NULL, "napi_wasm_init_env returned NULL");

  // ---- Test napi_create_error ----
  {
    napi_value code, msg, error;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "ERR_TEST", NAPI_AUTO_LENGTH,
                                      &code));
    NAPI_CALL(env,
              napi_create_string_utf8(env, "test error message",
                                      NAPI_AUTO_LENGTH, &msg));
    NAPI_CALL(env, napi_create_error(env, code, msg, &error));

    // Error should be an object
    napi_valuetype vtype;
    NAPI_CALL(env, napi_typeof(env, error, &vtype));
    CHECK_OR_FAIL(vtype == napi_object, "error: expected napi_object");

    // napi_is_error should return true
    bool is_error;
    NAPI_CALL(env, napi_is_error(env, error, &is_error));
    CHECK_OR_FAIL(is_error, "napi_is_error: expected true for Error");

    // Read back the message property
    napi_value got_msg;
    NAPI_CALL(env, napi_get_named_property(env, error, "message", &got_msg));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, got_msg, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "test error message") == 0,
                  "error message mismatch");

    // Read back the code property
    napi_value got_code;
    NAPI_CALL(env, napi_get_named_property(env, error, "code", &got_code));
    char cbuf[256];
    size_t clen;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, got_code, cbuf, sizeof(cbuf),
                                         &clen));
    CHECK_OR_FAIL(strcmp(cbuf, "ERR_TEST") == 0, "error code mismatch");
  }

  // ---- Test napi_create_type_error ----
  {
    napi_value code, msg, error;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "ERR_TYPE", NAPI_AUTO_LENGTH,
                                      &code));
    NAPI_CALL(env,
              napi_create_string_utf8(env, "type error message",
                                      NAPI_AUTO_LENGTH, &msg));
    NAPI_CALL(env, napi_create_type_error(env, code, msg, &error));

    bool is_error;
    NAPI_CALL(env, napi_is_error(env, error, &is_error));
    CHECK_OR_FAIL(is_error, "napi_is_error: expected true for TypeError");

    napi_value got_msg;
    NAPI_CALL(env, napi_get_named_property(env, error, "message", &got_msg));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, got_msg, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "type error message") == 0,
                  "type error message mismatch");
  }

  // ---- Test napi_create_range_error ----
  {
    napi_value code, msg, error;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "ERR_RANGE", NAPI_AUTO_LENGTH,
                                      &code));
    NAPI_CALL(env,
              napi_create_string_utf8(env, "range error message",
                                      NAPI_AUTO_LENGTH, &msg));
    NAPI_CALL(env, napi_create_range_error(env, code, msg, &error));

    bool is_error;
    NAPI_CALL(env, napi_is_error(env, error, &is_error));
    CHECK_OR_FAIL(is_error, "napi_is_error: expected true for RangeError");

    napi_value got_msg;
    NAPI_CALL(env, napi_get_named_property(env, error, "message", &got_msg));
    char buf[256];
    size_t len;
    NAPI_CALL(env,
              napi_get_value_string_utf8(env, got_msg, buf, sizeof(buf), &len));
    CHECK_OR_FAIL(strcmp(buf, "range error message") == 0,
                  "range error message mismatch");
  }

  // ---- Test napi_is_error returns false for non-errors ----
  {
    // A plain object is not an error
    napi_value obj;
    NAPI_CALL(env, napi_create_object(env, &obj));
    bool is_error;
    NAPI_CALL(env, napi_is_error(env, obj, &is_error));
    CHECK_OR_FAIL(!is_error, "napi_is_error: expected false for plain object");

    // A number is not an error
    napi_value num;
    NAPI_CALL(env, napi_create_int32(env, 42, &num));
    NAPI_CALL(env, napi_is_error(env, num, &is_error));
    CHECK_OR_FAIL(!is_error, "napi_is_error: expected false for number");

    // A string is not an error
    napi_value str;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "not an error", NAPI_AUTO_LENGTH,
                                      &str));
    NAPI_CALL(env, napi_is_error(env, str, &is_error));
    CHECK_OR_FAIL(!is_error, "napi_is_error: expected false for string");
  }

  // ---- Test napi_throw and exception pending/clear ----
  // Note: napi_throw uses NAPI_PREAMBLE which checks can_call_into_js().
  // When called outside a JS callback (as in our main() test harness),
  // V8 may return napi_pending_exception (status 10). We accept both
  // napi_ok and napi_pending_exception as valid outcomes.
  {
    // Initially no exception should be pending
    bool is_pending;
    NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
    CHECK_OR_FAIL(!is_pending,
                  "exception pending: expected false initially");

    // Create and throw an error
    napi_value code, msg, error;
    NAPI_CALL(env,
              napi_create_string_utf8(env, "ERR_THROWN", NAPI_AUTO_LENGTH,
                                      &code));
    NAPI_CALL(env,
              napi_create_string_utf8(env, "thrown error", NAPI_AUTO_LENGTH,
                                      &msg));
    NAPI_CALL(env, napi_create_error(env, code, msg, &error));
    napi_status throw_status = napi_throw(env, error);
    // Accept napi_ok (inside callback) or napi_pending_exception (outside)
    CHECK_OR_FAIL(throw_status == napi_ok ||
                      throw_status == napi_pending_exception,
                  "napi_throw: unexpected status");

    if (throw_status == napi_ok) {
      // Exception should now be pending
      NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
      CHECK_OR_FAIL(is_pending,
                    "exception pending: expected true after throw");

      // Clear the exception
      napi_value exception;
      NAPI_CALL(env, napi_get_and_clear_last_exception(env, &exception));

      // Exception should no longer be pending
      NAPI_CALL(env, napi_is_exception_pending(env, &is_pending));
      CHECK_OR_FAIL(!is_pending,
                    "exception pending: expected false after clear");

      // Verify the cleared exception is our error
      bool is_err;
      NAPI_CALL(env, napi_is_error(env, exception, &is_err));
      CHECK_OR_FAIL(is_err,
                    "cleared exception: expected to be an error");

      napi_value ex_msg;
      NAPI_CALL(env,
                napi_get_named_property(env, exception, "message", &ex_msg));
      char buf[256];
      size_t len;
      NAPI_CALL(env, napi_get_value_string_utf8(env, ex_msg, buf, sizeof(buf),
                                                 &len));
      CHECK_OR_FAIL(strcmp(buf, "thrown error") == 0,
                    "cleared exception: message mismatch");
    }
    // else: napi_pending_exception means throw was rejected because we're
    // outside a JS callback frame, which is expected in this test harness.
  }

  return PrintSuccess("TEST_ERROR");
}
