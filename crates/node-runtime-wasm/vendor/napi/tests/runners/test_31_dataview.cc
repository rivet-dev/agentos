#include "test_env.h"
#include "upstream_js_test.h"

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test31DataView : public FixtureTestBase {};

TEST_F(Test31DataView, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  napi_value addon = Init(s.env, exports);
  ASSERT_NE(addon, nullptr);
  ASSERT_TRUE(InstallUpstreamJsShim(s, addon));
  ASSERT_TRUE(RunUpstreamJsFile(
      s, std::string(NAPI_TESTS_ROOT_PATH) + "/js-native-api/test_dataview/test.js"));
}

TEST_F(Test31DataView, CreateArrayBufferWithNullDataOutParam) {
  EnvScope s(runtime_.get());

  napi_value arraybuffer = nullptr;
  ASSERT_EQ(napi_create_arraybuffer(s.env, 12, nullptr, &arraybuffer), napi_ok);
  ASSERT_NE(arraybuffer, nullptr);

  bool is_arraybuffer = false;
  ASSERT_EQ(napi_is_arraybuffer(s.env, arraybuffer, &is_arraybuffer), napi_ok);
  ASSERT_TRUE(is_arraybuffer);

  void* data = nullptr;
  size_t byte_length = 0;
  ASSERT_EQ(napi_get_arraybuffer_info(s.env, arraybuffer, &data, &byte_length),
            napi_ok);
  ASSERT_NE(data, nullptr);
  ASSERT_EQ(byte_length, 12u);
}
