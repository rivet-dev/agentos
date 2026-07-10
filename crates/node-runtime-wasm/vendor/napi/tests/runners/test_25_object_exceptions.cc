#include <string>

#include "test_env.h"
#include "upstream_js_test.h"

extern "C" napi_value Init(napi_env env, napi_value exports);

class Test25ObjectExceptions : public FixtureTestBase {};

TEST_F(Test25ObjectExceptions, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  ASSERT_NE(Init(s.env, exports), nullptr);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  ASSERT_EQ(napi_set_named_property(s.env, global, "__toex", exports), napi_ok);

  ASSERT_TRUE(RunScript(s, R"JS(
(() => {
function throws() { throw new Error('foobar'); }
const p = new Proxy({}, {
  get: throws,
  getOwnPropertyDescriptor: throws,
  defineProperty: throws,
  deleteProperty: throws,
  has: throws,
  set: throws,
  ownKeys: throws,
});
__toex.testExceptions(p);
})();
)JS", "test_25_object_exceptions"));
}
