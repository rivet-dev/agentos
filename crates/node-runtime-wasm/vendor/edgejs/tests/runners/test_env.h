#ifndef NAPI_V8_TEST_ENV_H_
#define NAPI_V8_TEST_ENV_H_

#include <memory>

#include <gtest/gtest.h>

#include "edge_environment_runtime.h"
#include "edge_runtime_platform.h"
#include "edge_timers_host.h"
#include "unofficial_napi.h"

class V8Runtime {
 public:
  V8Runtime() = default;
  ~V8Runtime() = default;
};

class FixtureTestBase : public ::testing::Test {
 protected:
  static void SetUpTestSuite() { runtime_ = std::make_unique<V8Runtime>(); }
  static void TearDownTestSuite() { runtime_.reset(); }
  static std::unique_ptr<V8Runtime> runtime_;
};

inline std::unique_ptr<V8Runtime> FixtureTestBase::runtime_;

struct EnvScope {
  struct IsolateShim {
    explicit IsolateShim(napi_env env_in) : env(env_in) {}
    napi_env env = nullptr;
  };

  explicit EnvScope(V8Runtime* runtime) {
    (void)runtime;
    EXPECT_EQ(unofficial_napi_create_env(8, &env, &scope), napi_ok);
    EXPECT_NE(env, nullptr);
    if (env != nullptr) {
      EXPECT_TRUE(EdgeAttachEnvironmentForRuntime(env));
      EXPECT_EQ(EdgeRuntimePlatformInstallHooks(env), napi_ok);
      EXPECT_EQ(EdgeInitializeTimersHost(env), napi_ok);
    }
    isolate = std::make_unique<IsolateShim>(env);
  }

  ~EnvScope() {
    isolate.reset();
    if (env != nullptr) {
      EXPECT_EQ(unofficial_napi_release_env(scope), napi_ok);
      env = nullptr;
      scope = nullptr;
    }
  }

  std::unique_ptr<IsolateShim> isolate;
  void* scope = nullptr;
  napi_env env = nullptr;
};

#endif  // NAPI_V8_TEST_ENV_H_
