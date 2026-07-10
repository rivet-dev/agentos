#include <algorithm>
#include <string>

#include "test_env.h"
#include "upstream_js_test.h"
#include "unofficial_napi.h"

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test14Exception : public FixtureTestBase {};

namespace {

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return {};
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return {};
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) return {};
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

std::string GetArrowMessage(napi_env env, napi_value exception) {
  if (env == nullptr || exception == nullptr) return {};

  napi_value source_line = nullptr;
  napi_value thrown_at = nullptr;
  if (unofficial_napi_take_preserved_error_formatting(
          env, exception, &source_line, &thrown_at) == napi_ok) {
    const std::string preserved = ValueToUtf8(env, source_line);
    if (!preserved.empty()) return preserved;
  }

  source_line = nullptr;
  if (unofficial_napi_get_error_source_line_for_stderr(env, exception, &source_line) ==
      napi_ok) {
    const std::string formatted = ValueToUtf8(env, source_line);
    if (!formatted.empty()) return formatted;
  }

  napi_value arrow = nullptr;
  if (napi_get_named_property(env, exception, "node:arrowMessage", &arrow) != napi_ok ||
      arrow == nullptr) {
    return {};
  }

  return ValueToUtf8(env, arrow);
}

std::string NormalizeCaretLine(std::string value) {
  size_t line_start = 0;
  while (line_start < value.size()) {
    size_t line_end = value.find('\n', line_start);
    if (line_end == std::string::npos) line_end = value.size();

    size_t first_non_space = value.find_first_not_of(' ', line_start);
    if (first_non_space != std::string::npos && first_non_space < line_end &&
        value[first_non_space] == '^' &&
        value.find_first_not_of(' ', first_non_space + 1) >= line_end) {
      value.erase(line_start, first_non_space - line_start);
      line_end -= first_non_space - line_start;
    }

    line_start = line_end + 1;
  }
  return value;
}

}  // namespace

TEST_F(Test14Exception, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  (void)Init(s.env, exports);

  bool pending = false;
  ASSERT_EQ(napi_is_exception_pending(s.env, &pending), napi_ok);
  ASSERT_TRUE(pending);
  napi_value init_error = nullptr;
  ASSERT_EQ(napi_get_and_clear_last_exception(s.env, &init_error), napi_ok);

  napi_value binding = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, init_error, "binding", &binding), napi_ok);
  ASSERT_TRUE(InstallUpstreamJsShim(s, binding));
  ASSERT_TRUE(SetUpstreamRequireException(s, init_error));
  ASSERT_TRUE(RunUpstreamJsFile(
      s, std::string(NAPI_TESTS_ROOT_PATH) + "/js-native-api/test_exception/test.js"));
}

TEST_F(Test14Exception, SetLastExceptionStoresArrowMessageOnThrownError) {
  EnvScope s(runtime_.get());

  napi_value script = nullptr;
  ASSERT_EQ(
      napi_create_string_utf8(
          s.env,
          "throw new Error('boom')\n//# sourceURL=original.js",
          NAPI_AUTO_LENGTH,
          &script),
      napi_ok);
  napi_value result = nullptr;
  ASSERT_EQ(napi_run_script(s.env, script, &result), napi_pending_exception);

  napi_value exception = nullptr;
  ASSERT_EQ(napi_get_and_clear_last_exception(s.env, &exception), napi_ok);
  ASSERT_NE(exception, nullptr);
  EXPECT_NE(GetArrowMessage(s.env, exception).find("throw new Error('boom')"), std::string::npos);
}

TEST_F(Test14Exception, SetLastExceptionPreservesArrowMessageAcrossSameErrorRethrow) {
  EnvScope s(runtime_.get());

  napi_value script = nullptr;
  ASSERT_EQ(
      napi_create_string_utf8(
          s.env, "throw new Error('boom')", NAPI_AUTO_LENGTH, &script),
      napi_ok);
  napi_value result = nullptr;
  ASSERT_EQ(napi_run_script(s.env, script, &result), napi_pending_exception);

  napi_value first = nullptr;
  ASSERT_EQ(napi_get_and_clear_last_exception(s.env, &first), napi_ok);
  ASSERT_NE(first, nullptr);
  const std::string first_line = GetArrowMessage(s.env, first);
  ASSERT_FALSE(first_line.empty());

  ASSERT_EQ(napi_throw(s.env, first), napi_pending_exception);

  napi_value second = nullptr;
  ASSERT_EQ(napi_get_and_clear_last_exception(s.env, &second), napi_ok);
  ASSERT_NE(second, nullptr);
  EXPECT_EQ(NormalizeCaretLine(GetArrowMessage(s.env, second)),
            NormalizeCaretLine(first_line));
}

TEST_F(Test14Exception, PreserveErrorSourceMessageStoresMappedArrowMessageWhenSourceMapsEnabled) {
  EnvScope s(runtime_.get());

  ASSERT_EQ(unofficial_napi_set_source_maps_enabled(s.env, true), napi_ok);

  napi_value callback_script = nullptr;
  ASSERT_EQ(
      napi_create_string_utf8(
          s.env,
          "(() => 'mapped.js:10\\nconst boom = 1;\\n      ^\\n\\n')",
          NAPI_AUTO_LENGTH,
          &callback_script),
      napi_ok);
  napi_value callback = nullptr;
  ASSERT_EQ(napi_run_script(s.env, callback_script, &callback), napi_ok);
  ASSERT_NE(callback, nullptr);
  ASSERT_EQ(
      unofficial_napi_set_get_source_map_error_source_callback(s.env, callback),
      napi_ok);

  napi_value script = nullptr;
  ASSERT_EQ(
      napi_create_string_utf8(
          s.env,
          "(() => { try { throw new Error('boom'); } catch (e) { return e; } })()\n"
          "//# sourceURL=original.js",
          NAPI_AUTO_LENGTH,
          &script),
      napi_ok);
  napi_value error = nullptr;
  ASSERT_EQ(napi_run_script(s.env, script, &error), napi_ok);
  ASSERT_NE(error, nullptr);
  ASSERT_EQ(unofficial_napi_preserve_error_source_message(s.env, error), napi_ok);

#if defined(NAPI_TEST_ENGINE_QUICKJS)
  // These source-map APIs are stable no-ops in QuickJS, so caught Errors do not
  // receive node:arrowMessage or preserved source-map output.
  EXPECT_EQ(GetArrowMessage(s.env, error), "");
#else
  EXPECT_EQ(GetArrowMessage(s.env, error),
            "mapped.js:10\nconst boom = 1;\n      ^\n\n");
#endif
}
