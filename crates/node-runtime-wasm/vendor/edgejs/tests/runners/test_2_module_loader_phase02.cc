#include <filesystem>
#include <fstream>
#include <string>

#include "test_env.h"
#include "edge_runtime.h"

class Test2ModuleLoaderPhase02 : public FixtureTestBase {};

namespace {

namespace fs = std::filesystem;

std::string ValueToUtf8(napi_env env, napi_value value) {
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) {
    return "";
  }
  size_t length = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &length) != napi_ok) {
    return "";
  }
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) {
    return "";
  }
  out.resize(copied);
  return out;
}

fs::path CreateTempProjectRoot(const std::string& name) {
  const fs::path root = fs::temp_directory_path() / ("edge_phase02_" + name);
  std::error_code ec;
  fs::remove_all(root, ec);
  fs::create_directories(root);
  return root;
}

void WriteFile(const fs::path& path, const std::string& contents) {
  fs::create_directories(path.parent_path());
  std::ofstream out(path);
  out << contents;
  out.close();
}

void RemoveDir(const fs::path& path) {
  std::error_code ec;
  fs::remove_all(path, ec);
}

}  // namespace

TEST_F(Test2ModuleLoaderPhase02, RelativeJsRequireWorks) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("relative");
  WriteFile(root / "lib.js", "module.exports = { value: 'ok-relative' };");
  WriteFile(root / "main.js", "const lib = require('./lib'); console.log(lib.value);");

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());
  EXPECT_NE(stdout_output.find("ok-relative"), std::string::npos);
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, JsonRequireWorks) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("json");
  WriteFile(root / "data.json", "{ \"name\": \"json-ok\", \"value\": 7 }");
  WriteFile(root / "main.js", "const d = require('./data.json'); console.log(d.name, d.value);");

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());
  EXPECT_NE(stdout_output.find("json-ok 7"), std::string::npos);
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, DirectoryIndexAndPackageMainWork) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("dir_main");
  WriteFile(root / "pkg" / "package.json", "{ \"main\": \"entry\" }");
  WriteFile(root / "pkg" / "entry.js", "module.exports = { value: 'package-main' };");
  WriteFile(root / "dir" / "index.js", "module.exports = { value: 'dir-index' };");
  WriteFile(root / "main.js",
            "const a = require('./pkg'); const b = require('./dir'); console.log(a.value, b.value);");

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());
  EXPECT_NE(stdout_output.find("package-main dir-index"), std::string::npos);
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, RequireCacheIdentityAndMutationWorks) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("cache");
  WriteFile(root / "counter.js", "globalThis.__phase2_loads = (globalThis.__phase2_loads || 0) + 1; module.exports = { id: 1 };");
  WriteFile(root / "main.js",
            "const path = __dirname + '/counter.js'; const a = require('./counter');"
            "const b = require('./counter');"
            "const same = (a === b) && globalThis.__phase2_loads === 1;"
            "require.cache[path] = { exports: { id: 99 } };"
            "const c = require('./counter');"
            "globalThis.__phase2_cache_result = same && c.id === 99 ? 'ok' : 'bad';");

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value cache_result = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "__phase2_cache_result", &cache_result), napi_ok);
  EXPECT_EQ(ValueToUtf8(s.env, cache_result), "ok");
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, MissingModuleReturnsModuleNotFound) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("missing");
  WriteFile(root / "main.js", "require('./does-not-exist');");

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_NE(error.find("Cannot find module"), std::string::npos);
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, EntryScriptPathWithDotSegmentsUsesNormalizedModuleBase) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("entry_dot_segments");
  fs::create_directories(root / "invoke");
  WriteFile(root / "lib.js", "module.exports = { value: 'normalized-base' };");
  WriteFile(root / "main.js",
            "const path = require('path');"
            "const lib = require('./lib');"
            "console.log(lib.value, path.normalize(__filename) === __filename ? 'normalized' : 'bad');");

  testing::internal::CaptureStdout();
  std::string error;
  const fs::path invoked_path = root / "invoke" / ".." / "main.js";
  const int exit_code = EdgeRunScriptFile(s.env, invoked_path.c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());
  EXPECT_NE(stdout_output.find("normalized-base normalized"), std::string::npos);
  RemoveDir(root);
}

TEST_F(Test2ModuleLoaderPhase02, PackageMainDotSegmentsResolveLikeNode) {
  EnvScope s(runtime_.get());
  const fs::path root = CreateTempProjectRoot("package_main_dot_segments");
  WriteFile(root / "pkg" / "package.json", "{ \"main\": \"./sub/../entry\" }");
  WriteFile(root / "pkg" / "entry.js", "module.exports = { value: 'package-main-dot-segments' };");
  WriteFile(root / "main.js", "const pkg = require('./pkg'); console.log(pkg.value);");

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, (root / "main.js").c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty());
  EXPECT_NE(stdout_output.find("package-main-dot-segments"), std::string::npos);
  RemoveDir(root);
}
