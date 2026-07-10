#include "test_env.h"

#include "unofficial_napi.h"

class Test65UnofficialContextify : public FixtureTestBase {};

namespace {

napi_value Str(napi_env env, const char* value) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &out) != napi_ok) return nullptr;
  return out;
}

napi_value Sym(napi_env env, const char* value) {
  napi_value desc = Str(env, value);
  if (desc == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_create_symbol(env, desc, &out) != napi_ok) return nullptr;
  return out;
}

}  // namespace

TEST_F(Test65UnofficialContextify, MakeRunDisposeRoundTrip) {
  EnvScope s(runtime_.get());

  napi_value sandbox = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &sandbox), napi_ok);

  napi_value result = nullptr;
  ASSERT_EQ(unofficial_napi_contextify_make_context(s.env,
                                                    sandbox,
                                                    Str(s.env, "ctx"),
                                                    Str(s.env, "test://origin"),
                                                    true,
                                                    true,
                                                    true,
                                                    Sym(s.env, "hdo"),
                                                    &result),
            napi_ok);
  ASSERT_NE(result, nullptr);

  napi_value eval_result = nullptr;
  const unofficial_napi_js_source run_source{Str(s.env, "globalThis.answer = 42; answer"), nullptr};
  ASSERT_EQ(unofficial_napi_contextify_run_script(s.env,
                                                  sandbox,
                                                  &run_source,
                                                  Str(s.env, "ctx.js"),
                                                  0,
                                                  0,
                                                  -1,
                                                  true,
                                                  false,
                                                  false,
                                                  Sym(s.env, "hdo"),
                                                  &eval_result),
            napi_ok);
  ASSERT_NE(eval_result, nullptr);

  int32_t answer = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, eval_result, &answer), napi_ok);
  EXPECT_EQ(answer, 42);

  napi_value answer_value = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, sandbox, "answer", &answer_value), napi_ok);
  ASSERT_EQ(napi_get_value_int32(s.env, answer_value, &answer), napi_ok);
  EXPECT_EQ(answer, 42);

  ASSERT_EQ(unofficial_napi_contextify_dispose_context(s.env, sandbox), napi_ok);
  const unofficial_napi_js_source disposed_source{Str(s.env, "1"), nullptr};
  EXPECT_EQ(unofficial_napi_contextify_run_script(s.env,
                                                  sandbox,
                                                  &disposed_source,
                                                  Str(s.env, "after_dispose.js"),
                                                  0,
                                                  0,
                                                  -1,
                                                  true,
                                                  false,
                                                  false,
                                                  Sym(s.env, "hdo"),
                                                  &eval_result),
            napi_invalid_arg);
}

TEST_F(Test65UnofficialContextify, SandboxGlobalThisAndMarkerAreNotEnumerableForDeepFreeze) {
  EnvScope s(runtime_.get());

  napi_value sandbox = nullptr;
  ASSERT_EQ(napi_create_object(s.env, &sandbox), napi_ok);

  napi_value result = nullptr;
  ASSERT_EQ(unofficial_napi_contextify_make_context(s.env,
                                                    sandbox,
                                                    Str(s.env, "ctx"),
                                                    Str(s.env, "test://origin"),
                                                    true,
                                                    true,
                                                    true,
                                                    Sym(s.env, "hdo"),
                                                    &result),
            napi_ok);
  ASSERT_NE(result, nullptr);

  napi_value eval_result = nullptr;
  const unofficial_napi_js_source freeze_source{
      Str(s.env,
          R"JS(
const globalThisDescriptor = Object.getOwnPropertyDescriptor(globalThis, "globalThis");
const markerDescriptor = Object.getOwnPropertyDescriptor(globalThis, "__quickjs_contextified");
if (!globalThisDescriptor || globalThisDescriptor.enumerable ||
    !globalThisDescriptor.writable || !globalThisDescriptor.configurable) {
  throw new Error("globalThis should be writable/configurable but non-enumerable");
}
if (!markerDescriptor || markerDescriptor.enumerable ||
    !markerDescriptor.writable || !markerDescriptor.configurable) {
  throw new Error("contextify marker should be writable/configurable but non-enumerable");
}
const keys = Object.keys(globalThis);
if (keys.includes("globalThis") || keys.includes("__quickjs_contextified")) {
  throw new Error("contextify internals should not be enumerable");
}
globalThis.__RSC_MANIFEST = {};
globalThis.__RSC_MANIFEST["/x"] = { ok: true };
function deepFreeze(obj) {
  if (obj === null || typeof obj !== "object" || Object.isFrozen(obj)) {
    return obj;
  }
  for (const value of Object.values(obj)) {
    deepFreeze(value);
  }
  return Object.freeze(obj);
}
deepFreeze(globalThis);
globalThis.__RSC_MANIFEST["/x"].ok;
)JS"),
      nullptr};
  ASSERT_EQ(unofficial_napi_contextify_run_script(s.env,
                                                  sandbox,
                                                  &freeze_source,
                                                  Str(s.env, "deep_freeze.js"),
                                                  0,
                                                  0,
                                                  -1,
                                                  true,
                                                  false,
                                                  false,
                                                  Sym(s.env, "hdo"),
                                                  &eval_result),
            napi_ok);
  ASSERT_NE(eval_result, nullptr);

  bool ok = false;
  ASSERT_EQ(napi_get_value_bool(s.env, eval_result, &ok), napi_ok);
  EXPECT_TRUE(ok);
}

TEST_F(Test65UnofficialContextify, CompileFunctionAndCachedData) {
  EnvScope s(runtime_.get());

  napi_value params = nullptr;
  ASSERT_EQ(napi_create_array_with_length(s.env, 2, &params), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 0, Str(s.env, "a")), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 1, Str(s.env, "b")), napi_ok);

  napi_value context_extensions = nullptr;
  ASSERT_EQ(napi_create_array_with_length(s.env, 0, &context_extensions), napi_ok);

  napi_value undef = nullptr;
  ASSERT_EQ(napi_get_undefined(s.env, &undef), napi_ok);

  napi_value out = nullptr;
  const unofficial_napi_js_source fn_source{Str(s.env, "return a + b;"), nullptr};
  ASSERT_EQ(unofficial_napi_contextify_compile_function(s.env,
                                                        &fn_source,
                                                        Str(s.env, "fn.js"),
                                                        0,
                                                        0,
                                                        undef,
                                                        context_extensions,
                                                        params,
                                                        Sym(s.env, "hdo"),
                                                        &out),
            napi_ok);
  ASSERT_NE(out, nullptr);

  napi_value fn = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, out, "function", &fn), napi_ok);
  ASSERT_NE(fn, nullptr);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value argv[2] = {nullptr, nullptr};
  ASSERT_EQ(napi_create_int32(s.env, 2, &argv[0]), napi_ok);
  ASSERT_EQ(napi_create_int32(s.env, 3, &argv[1]), napi_ok);

  napi_value fn_result = nullptr;
  ASSERT_EQ(napi_call_function(s.env, global, fn, 2, argv, &fn_result), napi_ok);
  int32_t sum = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, fn_result, &sum), napi_ok);
  EXPECT_EQ(sum, 5);

  // Cached data now flows through the bytecode handle APIs: compile eagerly,
  // serialize the engine bytes, and restore a live artifact from them.
  void* bytecode = nullptr;
  ASSERT_EQ(unofficial_napi_bytecode_compile(s.env,
                                             Str(s.env, "1 + 1"),
                                             Str(s.env, "script.js"),
                                             unofficial_napi_bytecode_shape_script,
                                             undef,
                                             Sym(s.env, "hdo"),
                                             0,
                                             0,
                                             &bytecode,
                                             nullptr),
            napi_ok);
  ASSERT_NE(bytecode, nullptr);

  napi_value cached_data = nullptr;
  ASSERT_EQ(unofficial_napi_bytecode_serialize(s.env, bytecode, &cached_data), napi_ok);
  ASSERT_NE(cached_data, nullptr);
  bool is_typedarray = false;
  ASSERT_EQ(napi_is_typedarray(s.env, cached_data, &is_typedarray), napi_ok);
  EXPECT_TRUE(is_typedarray);
  ASSERT_EQ(unofficial_napi_bytecode_release(s.env, bytecode), napi_ok);

  const uint8_t* bytes = nullptr;
  size_t byte_length = 0;
  napi_typedarray_type array_type = napi_uint8_array;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  void* data = nullptr;
  ASSERT_EQ(napi_get_typedarray_info(s.env, cached_data, &array_type, &byte_length, &data,
                                     &arraybuffer, &byte_offset),
            napi_ok);
  ASSERT_GT(byte_length, 0u);
  bytes = static_cast<const uint8_t*>(data);

  void* restored = nullptr;
  bool rejected = false;
  ASSERT_EQ(unofficial_napi_bytecode_deserialize(s.env,
                                                 bytes,
                                                 byte_length,
                                                 Str(s.env, "1 + 1"),
                                                 Str(s.env, "script.js"),
                                                 unofficial_napi_bytecode_shape_script,
                                                 undef,
                                                 Sym(s.env, "hdo"),
                                                 &restored,
                                                 &rejected),
            napi_ok);
  EXPECT_FALSE(rejected);
  ASSERT_NE(restored, nullptr);
  ASSERT_EQ(unofficial_napi_bytecode_release(s.env, restored), napi_ok);
}

TEST_F(Test65UnofficialContextify, CompileFunctionDoesNotUseGlobalFunctionConstructor) {
  EnvScope s(runtime_.get());

  napi_value patch_result = nullptr;
  ASSERT_EQ(napi_run_script(
                s.env,
                Str(s.env,
                    R"JS(
globalThis.Function = function Function() {
  throw new Error("global Function constructor should not be called");
};
1;
)JS"),
                &patch_result),
            napi_ok);

  napi_value undef = nullptr;
  ASSERT_EQ(napi_get_undefined(s.env, &undef), napi_ok);

  napi_value params = nullptr;
  ASSERT_EQ(napi_create_array_with_length(s.env, 1, &params), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 0, Str(s.env, "value")), napi_ok);

  napi_value out = nullptr;
  const unofficial_napi_js_source fn_source{Str(s.env, "return value + 1;"), nullptr};
  ASSERT_EQ(unofficial_napi_contextify_compile_function(s.env,
                                                        &fn_source,
                                                        Str(s.env, "no-global-function.js"),
                                                        0,
                                                        0,
                                                        undef,
                                                        undef,
                                                        params,
                                                        undef,
                                                        &out),
            napi_ok);
  ASSERT_NE(out, nullptr);

  napi_value fn = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, out, "function", &fn), napi_ok);
  ASSERT_NE(fn, nullptr);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value argv[1] = {nullptr};
  ASSERT_EQ(napi_create_int32(s.env, 41, &argv[0]), napi_ok);

  napi_value fn_result = nullptr;
  ASSERT_EQ(napi_call_function(s.env, global, fn, 1, argv, &fn_result), napi_ok);
  int32_t answer = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, fn_result, &answer), napi_ok);
  EXPECT_EQ(answer, 42);
}

TEST_F(Test65UnofficialContextify, CompileFunctionAcceptsHashbangBody) {
  EnvScope s(runtime_.get());

  napi_value undef = nullptr;
  ASSERT_EQ(napi_get_undefined(s.env, &undef), napi_ok);

  napi_value out = nullptr;
  const unofficial_napi_js_source hashbang_source{Str(s.env, "#!/usr/bin/env node\nreturn 42;"), nullptr};
  ASSERT_EQ(unofficial_napi_contextify_compile_function(s.env,
                                                        &hashbang_source,
                                                        Str(s.env, ""),
                                                        0,
                                                        0,
                                                        undef,
                                                        undef,
                                                        undef,
                                                        undef,
                                                        &out),
            napi_ok);
  ASSERT_NE(out, nullptr);

  napi_value fn = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, out, "function", &fn), napi_ok);
  ASSERT_NE(fn, nullptr);

  napi_value global = nullptr;
  ASSERT_EQ(napi_get_global(s.env, &global), napi_ok);
  napi_value fn_result = nullptr;
  ASSERT_EQ(napi_call_function(s.env, global, fn, 0, nullptr, &fn_result), napi_ok);

  int32_t answer = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, fn_result, &answer), napi_ok);
  EXPECT_EQ(answer, 42);
}

TEST_F(Test65UnofficialContextify, CompileFunctionRejectsBomBeforeHashbang) {
  EnvScope s(runtime_.get());

  napi_value undef = nullptr;
  ASSERT_EQ(napi_get_undefined(s.env, &undef), napi_ok);

  napi_value out = nullptr;
  const unofficial_napi_js_source bom_source{
      Str(s.env, "\xEF\xBB\xBF#!/usr/bin/env node\nreturn 42;"), nullptr};
  EXPECT_EQ(unofficial_napi_contextify_compile_function(s.env,
                                                        &bom_source,
                                                        Str(s.env, "bom_hashbang.js"),
                                                        0,
                                                        0,
                                                        undef,
                                                        undef,
                                                        undef,
                                                        undef,
                                                        &out),
            napi_pending_exception);

  bool pending = false;
  ASSERT_EQ(napi_is_exception_pending(s.env, &pending), napi_ok);
  EXPECT_TRUE(pending);
  if (pending) {
    napi_value error = nullptr;
    ASSERT_EQ(napi_get_and_clear_last_exception(s.env, &error), napi_ok);
    ASSERT_NE(error, nullptr);
  }
}

TEST_F(Test65UnofficialContextify, FunctionPrototypeSourceLocationsDoNotBlockStaticAssignments) {
  EnvScope s(runtime_.get());

  napi_value result = nullptr;
  ASSERT_EQ(napi_run_script(
                s.env,
                Str(s.env,
                    R"JS(
"use strict";
for (const key of ["fileName", "lineNumber", "columnNumber"]) {
  if (Object.getOwnPropertyDescriptor(Function.prototype, key) !== undefined) {
    throw new Error(`${key} should not be inherited from Function.prototype`);
  }
}
class Manifest {}
Manifest.fileName = "package.json";
Manifest.lineNumber = 1;
Manifest.columnNumber = 2;
const fileName = Object.getOwnPropertyDescriptor(Manifest, "fileName");
const lineNumber = Object.getOwnPropertyDescriptor(Manifest, "lineNumber");
const columnNumber = Object.getOwnPropertyDescriptor(Manifest, "columnNumber");
fileName.value === "package.json" && fileName.writable &&
  lineNumber.value === 1 && lineNumber.writable &&
  columnNumber.value === 2 && columnNumber.writable ? 1 : 0;
)JS"),
                &result),
            napi_ok);
  ASSERT_NE(result, nullptr);

  int32_t ok = 0;
  ASSERT_EQ(napi_get_value_int32(s.env, result, &ok), napi_ok);
  EXPECT_EQ(ok, 1);
}

TEST_F(Test65UnofficialContextify, CjsCompileAndSyntaxDetection) {
  EnvScope s(runtime_.get());

  napi_value params = nullptr;
  ASSERT_EQ(napi_create_array_with_length(s.env, 5, &params), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 0, Str(s.env, "exports")), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 1, Str(s.env, "require")), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 2, Str(s.env, "module")), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 3, Str(s.env, "__filename")), napi_ok);
  ASSERT_EQ(napi_set_element(s.env, params, 4, Str(s.env, "__dirname")), napi_ok);

  napi_value undef = nullptr;
  ASSERT_EQ(napi_get_undefined(s.env, &undef), napi_ok);

  napi_value out = nullptr;
  const unofficial_napi_js_source cjs_source{Str(s.env, "module.exports = 1;"), nullptr};
  ASSERT_EQ(unofficial_napi_contextify_compile_function(s.env,
                                                        &cjs_source,
                                                        Str(s.env, "cjs.js"),
                                                        0,
                                                        0,
                                                        undef,
                                                        undef,
                                                        params,
                                                        undef,
                                                        &out),
            napi_ok);
  ASSERT_NE(out, nullptr);
  napi_value fn = nullptr;
  ASSERT_EQ(napi_get_named_property(s.env, out, "function", &fn), napi_ok);
  ASSERT_NE(fn, nullptr);

  bool contains = false;
  ASSERT_EQ(unofficial_napi_contextify_contains_module_syntax(s.env,
                                                              Str(s.env, "export const x = 1;"),
                                                              Str(s.env, "esmish.js"),
                                                              Str(s.env, "file:///esmish.js"),
                                                              true,
                                                              &contains),
            napi_ok);
  EXPECT_TRUE(contains);

  ASSERT_EQ(unofficial_napi_contextify_contains_module_syntax(s.env,
                                                              Str(s.env, "module.exports = 1;"),
                                                              Str(s.env, "cjs.js"),
                                                              Str(s.env, "file:///cjs.js"),
                                                              true,
                                                              &contains),
            napi_ok);
  EXPECT_FALSE(contains);

  ASSERT_EQ(unofficial_napi_contextify_contains_module_syntax(
                s.env,
                Str(s.env,
                    "var __export = (target, all) => target;\n"
                    "// Annotate the CommonJS export names for ESM import in node:\n"
                    "0 && (module.exports = { build });\n"),
                Str(s.env, "esbuild-ish.js"),
                Str(s.env, "file:///esbuild-ish.js"),
                true,
                &contains),
            napi_ok);
  EXPECT_FALSE(contains);

  ASSERT_EQ(unofficial_napi_contextify_contains_module_syntax(s.env,
                                                              Str(s.env, "import('node:fs');"),
                                                              Str(s.env, "dynamic-import.cjs"),
                                                              Str(s.env, "file:///dynamic-import.cjs"),
                                                              true,
                                                              &contains),
            napi_ok);
  EXPECT_FALSE(contains);

  ASSERT_EQ(unofficial_napi_contextify_contains_module_syntax(s.env,
                                                              Str(s.env, "await 1;"),
                                                              Str(s.env, "tla.js"),
                                                              Str(s.env, "file:///tla.js"),
                                                              true,
                                                              &contains),
            napi_ok);
  EXPECT_TRUE(contains);
}

TEST_F(Test65UnofficialContextify, PrivateSymbolAcceptsAutoLength) {
  EnvScope s(runtime_.get());

  napi_value symbol = nullptr;
  ASSERT_EQ(unofficial_napi_create_private_symbol(
                s.env, "node:arrowMessage", NAPI_AUTO_LENGTH, &symbol),
            napi_ok);
  ASSERT_NE(symbol, nullptr);

  napi_valuetype type = napi_undefined;
  ASSERT_EQ(napi_typeof(s.env, symbol, &type), napi_ok);
  EXPECT_TRUE(type == napi_symbol || type == napi_object);
}
