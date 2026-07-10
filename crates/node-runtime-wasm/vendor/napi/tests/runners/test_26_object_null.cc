#include <string>

#include "test_env.h"
#include "upstream_js_test.h"

extern "C" void init_test_null(napi_env env, napi_value exports);

class Test26ObjectNull : public FixtureTestBase {};

TEST_F(Test26ObjectNull, PortedCoreFlow) {
  EnvScope s(runtime_.get());
  napi_value exports = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &exports), napi_ok);
  init_test_null(s.env, exports);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  ASSERT_EQ(napi_set_named_property(s.env, global, "__ton", exports), napi_ok);

  ASSERT_TRUE(RunScript(s, R"JS(
(() => {
const eProp = { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', keyIsNull:'Invalid argument', valueIsNull:'Invalid argument' };
const tn = __ton.testNull;
const eq = (a,b) => JSON.stringify(a) === JSON.stringify(b);
if (!eq(tn.setProperty(), eProp)) throw new Error('setProperty');
if (!eq(tn.getProperty(), eProp)) throw new Error('getProperty');
if (!eq(tn.hasProperty(), eProp)) throw new Error('hasProperty');
if (!eq(tn.hasOwnProperty(), eProp)) throw new Error('hasOwnProperty');
if (!eq(tn.deleteProperty(), { ...eProp, valueIsNull: 'napi_ok' })) throw new Error('deleteProperty');
if (!eq(tn.setNamedProperty(), eProp)) throw new Error('setNamedProperty');
if (!eq(tn.getNamedProperty(), eProp)) throw new Error('getNamedProperty');
if (!eq(tn.hasNamedProperty(), eProp)) throw new Error('hasNamedProperty');
const eElem = { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', valueIsNull:'Invalid argument' };
if (!eq(tn.setElement(), eElem)) throw new Error('setElement');
if (!eq(tn.getElement(), eElem)) throw new Error('getElement');
if (!eq(tn.hasElement(), eElem)) throw new Error('hasElement');
if (!eq(tn.deleteElement(), { ...eElem, valueIsNull: 'napi_ok' })) throw new Error('deleteElement');
if (!eq(tn.defineProperties(), { envIsNull:'Invalid argument', objectIsNull:'Invalid argument', descriptorListIsNull:'Invalid argument', utf8nameIsNull:'Invalid argument', methodIsNull:'Invalid argument' })) throw new Error('defineProperties');
if (!eq(tn.getPropertyNames(), eElem)) throw new Error('getPropertyNames');
if (!eq(tn.getAllPropertyNames(), eElem)) throw new Error('getAllPropertyNames');
if (!eq(tn.getPrototype(), eElem)) throw new Error('getPrototype');
})();
)JS", "test_26_object_null"));
}
