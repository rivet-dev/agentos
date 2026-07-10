#include <string>

#include "test_env.h"
#include "edge_runtime.h"

class Test4InternalBindingDispatchPhase02 : public FixtureTestBase {};

namespace {

constexpr const char* kDispatchCoverageScript = R"JS(
const bindingNames = [
  'async_wrap',
  'buffer',
  'builtins',
  'cares_wrap',
  'config',
  'constants',
  'contextify',
  'credentials',
  'crypto',
  'encoding_binding',
  'errors',
  'fs',
  'http_parser',
  'js_udp_wrap',
  'module_wrap',
  'modules',
  'options',
  'os',
  'pipe_wrap',
  'process_methods',
  'process_wrap',
  'report',
  'signal_wrap',
  'spawn_sync',
  'stream_wrap',
  'string_decoder',
  'symbols',
  'task_queue',
  'tcp_wrap',
  'timers',
  'trace_events',
  'tty_wrap',
  'types',
  'udp_wrap',
  'url',
  'url_pattern',
  'util',
  'uv',
];

const minimumShape = {
  async_wrap: ['setupHooks'],
  builtins: ['builtinIds'],
  config: ['hasOpenSSL'],
  constants: ['fs', 'os'],
  contextify: ['containsModuleSyntax', 'ContextifyScript', 'compileFunction'],
  credentials: ['implementsPosixCredentials'],
  module_wrap: ['createRequiredModuleFacade', 'kSourcePhase', 'kEvaluationPhase'],
  modules: ['readPackageJSON'],
  options: ['getCLIOptionsValues', 'envSettings', 'types'],
  symbols: ['async_id_symbol'],
  uv: ['getErrorMap', 'getErrorMessage', 'errname'],
};

for (const name of bindingNames) {
  const first = internalBinding(name);
  if (first === undefined || first === null) {
    throw new Error(`binding missing: ${name}`);
  }

  const second = internalBinding(name);
  if (second !== first) {
    throw new Error(`binding cache mismatch: ${name}`);
  }

  const type = typeof first;
  if (type !== 'object' && type !== 'function') {
    throw new Error(`binding invalid type: ${name} (${type})`);
  }

  const required = minimumShape[name] || [];
  for (const key of required) {
    if (!(key in first)) {
      throw new Error(`binding missing key: ${name}.${key}`);
    }
  }
}

globalThis.__edge_internal_binding_dispatch_count = bindingNames.length;
)JS";

}  // namespace

TEST_F(Test4InternalBindingDispatchPhase02, AllResolversAreReachableAndCached) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, kDispatchCoverageScript, &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);

  napi_value count_value = nullptr;
  ASSERT_EQ(napi_get_named_property(
                s.env, global, "__edge_internal_binding_dispatch_count", &count_value),
            napi_ok);

  uint32_t count = 0;
  ASSERT_EQ(napi_get_value_uint32(s.env, count_value, &count), napi_ok);
  EXPECT_EQ(count, 38u);
}
