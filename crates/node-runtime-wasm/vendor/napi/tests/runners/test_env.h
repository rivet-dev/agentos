#ifndef NAPI_SHARED_TEST_ENV_H_
#define NAPI_SHARED_TEST_ENV_H_

#include <memory>

#include <gtest/gtest.h>

#include "unofficial_napi.h"

#ifndef NAPI_TEST_MODULE_API_VERSION
#define NAPI_TEST_MODULE_API_VERSION 10
#endif

class NapiTestRuntime {
 public:
  NapiTestRuntime() {
    static constexpr char kDefaultFlags[] = "--expose-gc --js-float16array";
    EXPECT_EQ(unofficial_napi_set_flags_from_string(
                  kDefaultFlags, sizeof(kDefaultFlags) - 1),
              napi_ok);
  }
  ~NapiTestRuntime() = default;

  NapiTestRuntime(const NapiTestRuntime&) = delete;
  NapiTestRuntime& operator=(const NapiTestRuntime&) = delete;
};

class FixtureTestBase : public ::testing::Test {
 protected:
  static void SetUpTestSuite() { runtime_ = std::make_unique<NapiTestRuntime>(); }
  static void TearDownTestSuite() { runtime_.reset(); }
  static std::unique_ptr<NapiTestRuntime> runtime_;
};

inline std::unique_ptr<NapiTestRuntime> FixtureTestBase::runtime_;

struct EnvScope {
  explicit EnvScope(NapiTestRuntime* runtime) {
    (void)runtime;
    EXPECT_EQ(unofficial_napi_create_env(NAPI_TEST_MODULE_API_VERSION, &env, &scope),
              napi_ok);
    EXPECT_NE(env, nullptr);
    EXPECT_NE(scope, nullptr);
  }

  ~EnvScope() {
    if (scope != nullptr) {
      EXPECT_EQ(unofficial_napi_release_env(scope), napi_ok);
      scope = nullptr;
      env = nullptr;
    }
  }

  EnvScope(const EnvScope&) = delete;
  EnvScope& operator=(const EnvScope&) = delete;

  napi_env env = nullptr;
  void* scope = nullptr;
};

#endif  // NAPI_SHARED_TEST_ENV_H_
