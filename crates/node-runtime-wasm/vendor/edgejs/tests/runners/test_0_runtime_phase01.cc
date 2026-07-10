#include <filesystem>
#include <fstream>
#include <functional>
#include <string>
#include <vector>

#include "test_env.h"
#include "edge_path.h"
#include "edge_runtime.h"
#include "edge_runtime_platform.h"
#include "edge_task_queue.h"
#include "edge_timers_host.h"

class Test0RuntimePhase01 : public FixtureTestBase {};

namespace {

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

std::string WriteTempScript(const std::string& stem, const std::string& contents) {
  const auto temp_dir = std::filesystem::temp_directory_path();
  const auto unique_name =
      stem + "_" + std::to_string(static_cast<unsigned long long>(std::hash<std::string>{}(contents))) + ".js";
  const auto script_path = temp_dir / unique_name;
  std::ofstream out(script_path);
  out << contents;
  out.close();
  return script_path.string();
}

void RemoveTempScript(const std::string& path) {
  std::error_code ec;
  std::filesystem::remove(path, ec);
}

int32_t* GetInt32TypedArrayData(napi_env env, napi_value value, size_t expected_length) {
  napi_typedarray_type type = napi_int8_array;
  size_t length = 0;
  void* data = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env, value, &type, &length, &data, &arraybuffer, &byte_offset) != napi_ok) {
    return nullptr;
  }
  if (type != napi_int32_array || length < expected_length || data == nullptr) {
    return nullptr;
  }
  return static_cast<int32_t*>(data);
}

std::string GetGlobalUtf8(napi_env env, const char* name) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return "";

  napi_value value = nullptr;
  if (napi_get_named_property(env, global, name, &value) != napi_ok || value == nullptr) return "";
  return ValueToUtf8(env, value);
}

constexpr const char* kZlibRoundTripScript = R"JS(
const assert = require('assert');
const zlib = require('zlib');

const input = Buffer.from('edge-zlib-roundtrip');
const syncCompressed = zlib.gzipSync(input);
assert.strictEqual(zlib.gunzipSync(syncCompressed).toString(), 'edge-zlib-roundtrip');

zlib.gzip(input, (err, compressed) => {
  if (err) throw err;
  const inflated = zlib.gunzipSync(compressed);
  globalThis.__edge_zlib_roundtrip = inflated.toString();
});
)JS";

struct PlatformImmediateTask {
  std::vector<int>* order = nullptr;
  int value = 0;
  int nested_value = 0;
  int nested_flags = kEdgeRuntimePlatformTaskNone;
};

void RunPlatformImmediateTask(napi_env env, void* data) {
  auto* task = static_cast<PlatformImmediateTask*>(data);
  if (task == nullptr || task->order == nullptr) return;
  task->order->push_back(task->value);
  if (task->nested_value != 0) {
    auto* nested = new PlatformImmediateTask();
    nested->order = task->order;
    nested->value = task->nested_value;
    if (EdgeRuntimePlatformEnqueueTask(env,
                                      RunPlatformImmediateTask,
                                      nested,
                                      [](napi_env, void* p) { delete static_cast<PlatformImmediateTask*>(p); },
                                      task->nested_flags) != napi_ok) {
      delete nested;
    }
  }
}

}  // namespace

TEST_F(Test0RuntimePhase01, ValidFixtureScriptReturnsZero) {
  EnvScope s(runtime_.get());
  const std::string script_path = std::string(NAPI_V8_ROOT_PATH) + "/tests/fixtures/phase0_hello.js";
  ASSERT_TRUE(std::filesystem::exists(script_path)) << "Missing fixture: " << script_path;
  testing::internal::CaptureStdout();

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, script_path.c_str(), &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();
  EXPECT_EQ(exit_code, 0) << "error=" << error << ", fixture=" << script_path;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("hello from edge"), std::string::npos);
}

TEST_F(Test0RuntimePhase01, ThrownErrorReturnsNonZero) {
  EnvScope s(runtime_.get());
  const std::string script_path = WriteTempScript("edge_phase01_throw", "throw new Error('boom from edge');");

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, script_path.c_str(), &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_NE(error.find("boom from edge"), std::string::npos);

  RemoveTempScript(script_path);
}

TEST_F(Test0RuntimePhase01, SyntaxErrorReturnsNonZero) {
  EnvScope s(runtime_.get());
  const std::string script_path = WriteTempScript("edge_phase01_syntax", "function (");

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, script_path.c_str(), &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_FALSE(error.empty());

  RemoveTempScript(script_path);
}

TEST_F(Test0RuntimePhase01, EsmImportOfThrowingCjsReturnsJsErrorInsteadOfCrashing) {
  EnvScope s(runtime_.get());

  const auto temp_dir = std::filesystem::temp_directory_path() / "edge_phase01_esm_import_cjs_throw";
  std::error_code mkdir_ec;
  std::filesystem::create_directories(temp_dir, mkdir_ec);
  ASSERT_FALSE(mkdir_ec) << mkdir_ec.message();

  const auto cjs_path = temp_dir / "bad.cjs";
  const auto esm_path = temp_dir / "main.mjs";

  {
    std::ofstream out(cjs_path);
    out << "throw new Error('boom from cjs');\n";
  }
  {
    std::ofstream out(esm_path);
    out << "import './bad.cjs';\n";
    out << "globalThis.__should_not_run = true;\n";
  }

  std::string error;
  const int exit_code = EdgeRunScriptFile(s.env, esm_path.c_str(), &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_NE(error.find("boom from cjs"), std::string::npos) << error;

  std::error_code remove_ec;
  std::filesystem::remove_all(temp_dir, remove_ec);
}

TEST_F(Test0RuntimePhase01, SourcePathCanBeTestedIndependently) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, "globalThis.__phase01_source = 'ok';", &error);
  EXPECT_EQ(exit_code, 0);
  EXPECT_TRUE(error.empty());

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value value = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, global, "__phase01_source", &value), napi_ok);
  EXPECT_EQ(ValueToUtf8(s.env, value), "ok");
}

TEST_F(Test0RuntimePhase01, EmptySourceReturnsNonZero) {
  EnvScope s(runtime_.get());

  std::string error;
  const int exit_code = EdgeRunScriptSource(s.env, "", &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_EQ(error, "Empty script source");
}

TEST_F(Test0RuntimePhase01, NativePathResolveNormalizesDotSegments) {
#ifdef _WIN32
  EXPECT_EQ(edge_path::PathResolve("C:\\base\\dir", {"..\\pkg", ".\\entry.js"}),
            "C:\\base\\pkg\\entry.js");
#else
  EXPECT_EQ(edge_path::PathResolve("/tmp/base/dir", {"../pkg", "./entry.js"}),
            "/tmp/base/pkg/entry.js");
#endif
}

TEST_F(Test0RuntimePhase01, TimersHostStateIsIsolatedPerEnv) {
  EnvScope first(runtime_.get());
  EnvScope second(runtime_.get());

  napi_value first_binding = EdgeInstallTimersHostBinding(first.env);
  napi_value second_binding = EdgeInstallTimersHostBinding(second.env);
  ASSERT_NE(first_binding, nullptr);
  ASSERT_NE(second_binding, nullptr);

  napi_value first_timeout_info = nullptr;
  napi_value second_timeout_info = nullptr;
  napi_value first_immediate_info = nullptr;
  napi_value second_immediate_info = nullptr;
  ASSERT_EQ(napi_get_named_property(first.env, first_binding, "timeoutInfo", &first_timeout_info), napi_ok);
  ASSERT_EQ(napi_get_named_property(second.env, second_binding, "timeoutInfo", &second_timeout_info), napi_ok);
  ASSERT_EQ(napi_get_named_property(first.env, first_binding, "immediateInfo", &first_immediate_info), napi_ok);
  ASSERT_EQ(napi_get_named_property(second.env, second_binding, "immediateInfo", &second_immediate_info), napi_ok);

  int32_t* first_timeout_data = GetInt32TypedArrayData(first.env, first_timeout_info, 1);
  int32_t* second_timeout_data = GetInt32TypedArrayData(second.env, second_timeout_info, 1);
  int32_t* first_immediate_data = GetInt32TypedArrayData(first.env, first_immediate_info, 3);
  int32_t* second_immediate_data = GetInt32TypedArrayData(second.env, second_immediate_info, 3);
  ASSERT_NE(first_timeout_data, nullptr);
  ASSERT_NE(second_timeout_data, nullptr);
  ASSERT_NE(first_immediate_data, nullptr);
  ASSERT_NE(second_immediate_data, nullptr);

  first_timeout_data[0] = 2;
  second_timeout_data[0] = 5;
  first_immediate_data[1] = 1;
  second_immediate_data[1] = 3;

  EXPECT_EQ(EdgeGetActiveTimeoutCount(first.env), 2);
  EXPECT_EQ(EdgeGetActiveTimeoutCount(second.env), 5);
  EXPECT_EQ(EdgeGetActiveImmediateRefCount(first.env), 1u);
  EXPECT_EQ(EdgeGetActiveImmediateRefCount(second.env), 3u);
}

TEST_F(Test0RuntimePhase01, TimersHostStateCanBeDestroyedAndRecreatedAcrossEnvs) {
  {
    EnvScope first(runtime_.get());

    napi_value binding = EdgeInstallTimersHostBinding(first.env);
    ASSERT_NE(binding, nullptr);

    napi_value timeout_info = nullptr;
    napi_value immediate_info = nullptr;
    ASSERT_EQ(napi_get_named_property(first.env, binding, "timeoutInfo", &timeout_info), napi_ok);
    ASSERT_EQ(napi_get_named_property(first.env, binding, "immediateInfo", &immediate_info), napi_ok);

    int32_t* timeout_data = GetInt32TypedArrayData(first.env, timeout_info, 1);
    int32_t* immediate_data = GetInt32TypedArrayData(first.env, immediate_info, 3);
    ASSERT_NE(timeout_data, nullptr);
    ASSERT_NE(immediate_data, nullptr);

    timeout_data[0] = 4;
    immediate_data[1] = 2;

    EXPECT_EQ(EdgeGetActiveTimeoutCount(first.env), 4);
    EXPECT_EQ(EdgeGetActiveImmediateRefCount(first.env), 2u);
  }

  {
    EnvScope second(runtime_.get());

    napi_value binding = EdgeInstallTimersHostBinding(second.env);
    ASSERT_NE(binding, nullptr);

    napi_value timeout_info = nullptr;
    napi_value immediate_info = nullptr;
    ASSERT_EQ(napi_get_named_property(second.env, binding, "timeoutInfo", &timeout_info), napi_ok);
    ASSERT_EQ(napi_get_named_property(second.env, binding, "immediateInfo", &immediate_info), napi_ok);

    int32_t* timeout_data = GetInt32TypedArrayData(second.env, timeout_info, 1);
    int32_t* immediate_data = GetInt32TypedArrayData(second.env, immediate_info, 3);
    ASSERT_NE(timeout_data, nullptr);
    ASSERT_NE(immediate_data, nullptr);

    timeout_data[0] = 1;
    immediate_data[1] = 5;

    EXPECT_EQ(EdgeGetActiveTimeoutCount(second.env), 1);
    EXPECT_EQ(EdgeGetActiveImmediateRefCount(second.env), 5u);
  }
}

TEST_F(Test0RuntimePhase01, TaskQueueStateIsIsolatedPerEnv) {
  EnvScope first(runtime_.get());
  EnvScope second(runtime_.get());

  napi_value first_binding = EdgeGetOrCreateTaskQueueBinding(first.env);
  napi_value second_binding = EdgeGetOrCreateTaskQueueBinding(second.env);
  ASSERT_NE(first_binding, nullptr);
  ASSERT_NE(second_binding, nullptr);

  napi_value first_tick_info = nullptr;
  napi_value second_tick_info = nullptr;
  ASSERT_EQ(napi_get_named_property(first.env, first_binding, "tickInfo", &first_tick_info), napi_ok);
  ASSERT_EQ(napi_get_named_property(second.env, second_binding, "tickInfo", &second_tick_info), napi_ok);

  int32_t* first_tick_fields = GetInt32TypedArrayData(first.env, first_tick_info, 2);
  int32_t* second_tick_fields = GetInt32TypedArrayData(second.env, second_tick_info, 2);
  ASSERT_NE(first_tick_fields, nullptr);
  ASSERT_NE(second_tick_fields, nullptr);

  first_tick_fields[0] = 1;
  first_tick_fields[1] = 0;
  second_tick_fields[0] = 0;
  second_tick_fields[1] = 1;

  bool first_has_tick_scheduled = false;
  bool first_has_rejection_to_warn = false;
  bool second_has_tick_scheduled = false;
  bool second_has_rejection_to_warn = false;
  EXPECT_TRUE(EdgeGetTaskQueueFlags(first.env, &first_has_tick_scheduled, &first_has_rejection_to_warn));
  EXPECT_TRUE(EdgeGetTaskQueueFlags(second.env, &second_has_tick_scheduled, &second_has_rejection_to_warn));
  EXPECT_TRUE(first_has_tick_scheduled);
  EXPECT_FALSE(first_has_rejection_to_warn);
  EXPECT_FALSE(second_has_tick_scheduled);
  EXPECT_TRUE(second_has_rejection_to_warn);
}

TEST_F(Test0RuntimePhase01, NativeImmediateQueueRunsBeforeJsImmediatesAndDrainsNestedTasks) {
  EnvScope s(runtime_.get());

  auto* first = new PlatformImmediateTask();
  std::vector<int> order;
  first->order = &order;
  first->value = 1;
  first->nested_value = 2;
  ASSERT_EQ(EdgeRuntimePlatformEnqueueTask(s.env,
                                          RunPlatformImmediateTask,
                                          first,
                                          [](napi_env, void* p) { delete static_cast<PlatformImmediateTask*>(p); },
                                          kEdgeRuntimePlatformTaskRefed),
            napi_ok);
  EXPECT_TRUE(EdgeRuntimePlatformHasImmediateTasks(s.env));
  EXPECT_TRUE(EdgeRuntimePlatformHasRefedImmediateTasks(s.env));
  EXPECT_EQ(EdgeRuntimePlatformDrainImmediateTasks(s.env), 2u);

  ASSERT_EQ(order.size(), 2u);
  EXPECT_EQ(order[0], 1);
  EXPECT_EQ(order[1], 2);
  EXPECT_FALSE(EdgeRuntimePlatformHasImmediateTasks(s.env));
  EXPECT_FALSE(EdgeRuntimePlatformHasRefedImmediateTasks(s.env));
}

TEST_F(Test0RuntimePhase01, ZlibWriteResultStateCanBeDestroyedAndRecreatedAcrossEnvs) {
  {
    EnvScope first(runtime_.get());

    std::string error;
    const int exit_code = EdgeRunScriptSourceWithLoop(first.env, kZlibRoundTripScript, &error, true);
    EXPECT_EQ(exit_code, 0) << "error=" << error;
    EXPECT_TRUE(error.empty());
    EXPECT_EQ(GetGlobalUtf8(first.env, "__edge_zlib_roundtrip"), "edge-zlib-roundtrip");
  }

  {
    EnvScope second(runtime_.get());

    std::string error;
    const int exit_code = EdgeRunScriptSourceWithLoop(second.env, kZlibRoundTripScript, &error, true);
    EXPECT_EQ(exit_code, 0) << "error=" << error;
    EXPECT_TRUE(error.empty());
    EXPECT_EQ(GetGlobalUtf8(second.env, "__edge_zlib_roundtrip"), "edge-zlib-roundtrip");
  }
}
