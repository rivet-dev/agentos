#include <algorithm>
#include <string>
#include <string_view>
#include <vector>

#include "binding_registry/binding_registry.h"
#include "edge_runtime.h"
#include "test_env.h"

class Test6BindingRegistryPhase04 : public FixtureTestBase {};

namespace {

bool HasName(const std::vector<std::string_view>& names, std::string_view name) {
  return std::binary_search(names.begin(), names.end(), name);
}

constexpr const char* kRegistryCacheScript = R"JS(
const names = [
  'async_wrap',
  'buffer',
  'builtins',
  'config',
  'constants',
  'contextify',
  'errors',
  'fs',
  'module_wrap',
  'modules',
  'options',
  'symbols',
  'task_queue',
  'tcp_wrap',
  'types',
  'util',
  'uv',
];

for (const name of names) {
  const first = internalBinding(name);
  if (first === undefined || first === null) {
    throw new Error(`binding missing: ${name}`);
  }
  const second = internalBinding(name);
  if (second !== first) {
    throw new Error(`registry cache mismatch: ${name}`);
  }
}

if (internalBinding('__edge_missing_binding__') !== undefined) {
  throw new Error('unknown binding should resolve to undefined');
}

globalThis.__edge_binding_registry_cache_ok = 1;
)JS";

}  // namespace

TEST_F(Test6BindingRegistryPhase04, ManifestIsSortedAndCoversCurrentInternalBindings) {
  const auto names = edge::binding_registry::Names();

  ASSERT_EQ(names.size(), 63u);
  EXPECT_TRUE(std::is_sorted(names.begin(), names.end()));
  EXPECT_TRUE(std::adjacent_find(names.begin(), names.end()) == names.end());

  EXPECT_TRUE(edge::binding_registry::Has("buffer"));
  EXPECT_TRUE(edge::binding_registry::Has("contextify"));
  EXPECT_TRUE(edge::binding_registry::Has("module_wrap"));
  EXPECT_TRUE(edge::binding_registry::Has("zlib"));

  EXPECT_TRUE(HasName(names, "buffer"));
  EXPECT_TRUE(HasName(names, "fs"));
  EXPECT_FALSE(edge::binding_registry::Has("__edge_missing_binding__"));
  EXPECT_FALSE(edge::binding_registry::Has("os_constants"));
}

TEST_F(Test6BindingRegistryPhase04, DirectRegistryGetCachesAndFinalizes) {
  EnvScope s(runtime_.get());

  napi_value first = edge::binding_registry::Get(s.env, "types");
  ASSERT_NE(first, nullptr);

  napi_value second = edge::binding_registry::Get(s.env, "types");
  EXPECT_EQ(second, first);

  napi_value missing = edge::binding_registry::Get(s.env, "__edge_missing_binding__");
  ASSERT_NE(missing, nullptr);
  napi_valuetype missing_type = napi_null;
  ASSERT_EQ(napi_typeof(s.env, missing, &missing_type), napi_ok);
  EXPECT_EQ(missing_type, napi_undefined);

  edge::binding_registry::FinalizeEnv(s.env);
  EXPECT_EQ(edge::binding_registry::Get(s.env, "types"), nullptr);
}

TEST_F(Test6BindingRegistryPhase04, InternalBindingCallbackUsesRegistryCache) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, kRegistryCacheScript, &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

  napi_value ok_value = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "__edge_binding_registry_cache_ok", &ok_value), napi_ok);

  int32_t ok = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, ok_value, &ok), napi_ok);
  EXPECT_EQ(ok, 1);
}
