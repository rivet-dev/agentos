#include "test_env.h"
#include "upstream_js_test.h"

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test38Finalizer : public FixtureTestBase {};

TEST_F(Test38Finalizer, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  napi_value addon = Init(s.env, exports);
  ASSERT_NE(addon, nullptr);
  ASSERT_TRUE(InstallUpstreamJsShim(s, addon));

  std::string path = std::string(NAPI_TESTS_ROOT_PATH) + "/js-native-api/test_finalizer/test.js";
  std::string source = ReadTextFile(path);
  const std::string strict = "'use strict';";
  if (source.rfind(strict, 0) == 0) {
    source.erase(0, strict.size());
  }

  const std::string first_gc_loop = "for (let i = 0; i < 10; ++i) {";
  size_t first_gc_loop_pos = source.find(first_gc_loop);
  ASSERT_NE(first_gc_loop_pos, std::string::npos);
  source.insert(first_gc_loop_pos, "await Promise.resolve();\n\n");

  const std::string async_call = "runAsyncTests();";
  size_t async_call_pos = source.rfind(async_call);
  ASSERT_NE(async_call_pos, std::string::npos);
  source.replace(async_call_pos, async_call.size(), "await runAsyncTests();");

  std::string wrapped_source = "'use strict';\n(async () => {\n" + source + "\n})();\n";
  ASSERT_TRUE(RunScript(s, wrapped_source, path.c_str()));
}
