#ifndef NAPI_SHARED_UPSTREAM_JS_TEST_H_
#define NAPI_SHARED_UPSTREAM_JS_TEST_H_

#include <fstream>
#include <sstream>
#include <string>

#include "test_env.h"

inline std::string ReadTextFile(const std::string& path) {
  std::ifstream in(path);
  std::ostringstream ss;
  ss << in.rdbuf();
  return ss.str();
}

inline std::string NapiValueToUtf8(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return {};
  size_t length = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
    return {};
  }
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) {
    return {};
  }
  out.resize(copied);
  return out;
}

inline std::string NapiExceptionMessage(napi_env env, napi_value exception) {
  if (env == nullptr || exception == nullptr) return "<empty>";

  napi_value stack = nullptr;
  if (napi_get_named_property(env, exception, "stack", &stack) == napi_ok && stack != nullptr) {
    const std::string stack_message = NapiValueToUtf8(env, stack);
    if (!stack_message.empty()) return stack_message;
  }

  napi_value message = nullptr;
  if (napi_coerce_to_string(env, exception, &message) == napi_ok && message != nullptr) {
    const std::string coerced = NapiValueToUtf8(env, message);
    if (!coerced.empty()) return coerced;
  }

  return "<empty>";
}

inline bool DrainMicrotasks(napi_env env) {
  for (int i = 0; i < 64; ++i) {
    napi_status status = unofficial_napi_process_microtasks(env);
    if (status != napi_ok) {
      ADD_FAILURE() << "Failed to process microtasks: " << status;
      return false;
    }
  }
  return true;
}

inline bool RunScript(EnvScope& s, const std::string& source_text, const char* label) {
  napi_value source = nullptr;
  if (napi_create_string_utf8(s.env, source_text.c_str(), source_text.size(), &source) !=
      napi_ok) {
    ADD_FAILURE() << "Failed to create script source (" << label << ")";
    return false;
  }

  napi_value result = nullptr;
  napi_status status = napi_run_script(s.env, source, &result);
  if (status != napi_ok) {
    bool pending = false;
    if (napi_is_exception_pending(s.env, &pending) == napi_ok && pending) {
      napi_value exception = nullptr;
      if (napi_get_and_clear_last_exception(s.env, &exception) == napi_ok) {
        ADD_FAILURE() << "JS exception (" << label << "): "
                      << NapiExceptionMessage(s.env, exception);
      } else {
        ADD_FAILURE() << "JS exception (" << label << "): <failed to read exception>";
      }
    } else {
      ADD_FAILURE() << "Failed to run script (" << label << "): " << status;
    }
    return false;
  }

  return DrainMicrotasks(s.env);
}

inline napi_value ForceGcCallback(napi_env env, napi_callback_info info) {
  (void)info;
  (void)unofficial_napi_request_gc_for_testing(env);
  (void)unofficial_napi_process_microtasks(env);

  napi_value result = nullptr;
  (void)napi_get_undefined(env, &result);
  return result;
}

inline bool InstallUpstreamJsShim(EnvScope& s, napi_value addon_exports) {
  napi_value global = nullptr;
  if (napi_get_global(s.env, &global) != napi_ok) return false;
  if (napi_set_named_property(s.env, global, "__napi_test_addon", addon_exports) != napi_ok) {
    return false;
  }

  napi_value gc = nullptr;
  if (napi_create_function(
          s.env, "__napi_force_gc", NAPI_AUTO_LENGTH, ForceGcCallback, nullptr, &gc) !=
      napi_ok) {
    return false;
  }
  if (napi_set_named_property(s.env, global, "__napi_force_gc", gc) != napi_ok) {
    return false;
  }

  const char* shim = R"JS(
(() => {
  'use strict';
  const __mustCallRecords = [];
  function __deepEqual(a, b) {
    if (Object.is(a, b)) return true;
    if (typeof a !== typeof b) return false;
    if (a === null || b === null) return a === b;
    if (typeof a !== 'object') return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const aKeys = Reflect.ownKeys(a);
    const bKeys = Reflect.ownKeys(b);
    if (aKeys.length !== bKeys.length) return false;
    for (const k of aKeys) {
      if (!bKeys.includes(k)) return false;
      if (!__deepEqual(a[k], b[k])) return false;
    }
    return true;
  }

  globalThis.common = {
    buildType: 'Debug',
    mustCall(fn, expected = 1) {
      if (typeof fn !== 'function') fn = () => {};
      const rec = { called: 0, expected };
      __mustCallRecords.push(rec);
      return function(...args) {
        rec.called++;
        return fn.apply(this, args);
      };
    },
    mustNotCall(message) {
      return function() {
        throw new Error(message || 'mustNotCall');
      };
    }
  };

  function __assert(value, message) {
    if (!value) throw new Error(message || 'assert failed');
  }
  __assert.strictEqual = function(actual, expected, message) {
    if (!Object.is(actual, expected)) throw new Error(message || `strictEqual: ${actual} !== ${expected}`);
  };
  __assert.deepStrictEqual = function(actual, expected, message) {
    if (!__deepEqual(actual, expected)) {
      throw new Error(message || `deepStrictEqual failed: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
    }
  };
  __assert.notStrictEqual = function(actual, expected, message) {
    if (Object.is(actual, expected)) throw new Error(message || `notStrictEqual: ${actual} === ${expected}`);
  };
  __assert.throws = function(fn, expected) {
    let threw = false;
    let err;
    try {
      fn();
    } catch (e) {
      threw = true;
      err = e;
    }
    if (!threw) throw new Error('assert.throws failed: no throw');
    if (expected === undefined) return;
    if (expected instanceof RegExp) {
      const message = String(err);
      const source = String(expected);
      const quickjsReadOnly =
        source.includes('Cannot assign to read only property') &&
        /^TypeError: '.+' is read-only$/.test(message);
      const quickjsGetterOnly =
        source.includes('which has only a getter') &&
        /^TypeError: no setter for property$/.test(message);
      if (!expected.test(message) && !quickjsReadOnly && !quickjsGetterOnly) {
        throw new Error(`assert.throws regex mismatch: ${message}`);
      }
      return;
    }
    if (typeof expected === 'function') {
      if (expected.prototype && err instanceof expected) return;
      const ok = expected(err);
      if (ok !== true) throw new Error('assert.throws predicate mismatch');
      return;
    }
    if (typeof expected === 'object' && expected !== null) {
      for (const key of Object.keys(expected)) {
        if (!Object.is(err?.[key], expected[key])) {
          throw new Error(`assert.throws object mismatch on ${key}`);
        }
      }
      return;
    }
  };
  __assert.ok = function(value, message) {
    if (!value) throw new Error(message || 'assert.ok failed');
  };
  globalThis.assert = __assert;

  globalThis.require = function(spec) {
    if (spec === '../../common') return globalThis.common;
    if (spec === '../../common/gc') {
      return {
        gcUntil: async function(name, predicate) {
          for (let i = 0; i < 256; i++) {
            if (predicate()) return;
            __napi_force_gc();
            await Promise.resolve();
          }
          throw new Error(`gcUntil timeout: ${name}`);
        }
      };
    }
    if (spec === 'assert') return globalThis.assert;
    if (spec.startsWith('./build/')) {
      if (globalThis.__napi_test_require_exception) {
        const err = globalThis.__napi_test_require_exception;
        globalThis.__napi_test_require_exception = null;
        throw err;
      }
      return globalThis.__napi_test_addon;
    }
    throw new Error(`Unsupported require: ${spec}`);
  };
  globalThis.require.main = {};
  globalThis.module = {};
  globalThis.global = globalThis;
  globalThis.gc = globalThis.__napi_force_gc;
  globalThis.console = {
    log() {},
    error() {},
    warn() {},
  };

  globalThis.__napi_verify_must_call = function() {
    for (const rec of __mustCallRecords) {
      if (rec.called !== rec.expected) {
        throw new Error(`mustCall mismatch: called=${rec.called}, expected=${rec.expected}`);
      }
    }
  };
})();
)JS";

  return RunScript(s, shim, "shim");
}

inline bool SetUpstreamRequireException(EnvScope& s, napi_value exception_value) {
  napi_value global = nullptr;
  if (napi_get_global(s.env, &global) != napi_ok) return false;
  return napi_set_named_property(s.env, global, "__napi_test_require_exception", exception_value) ==
         napi_ok;
}

inline bool RunUpstreamJsFile(EnvScope& s, const std::string& path) {
  const std::string source = ReadTextFile(path);
  if (source.empty()) {
    ADD_FAILURE() << "Unable to read upstream JS file: " << path;
    return false;
  }
  if (!RunScript(s, source, path.c_str())) return false;
  if (!RunScript(s, "for (let i = 0; i < 32; i++) __napi_force_gc();", "pre-verify-gc")) {
    return false;
  }
  return RunScript(s, "__napi_verify_must_call();", "must-call-verification");
}

inline bool RunUpstreamJsFileNoMustCallVerification(EnvScope& s, const std::string& path) {
  const std::string source = ReadTextFile(path);
  if (source.empty()) {
    ADD_FAILURE() << "Unable to read upstream JS file: " << path;
    return false;
  }
  if (!RunScript(s, source, path.c_str())) return false;
  return RunScript(s, "for (let i = 0; i < 32; i++) __napi_force_gc();", "post-run-gc");
}

#endif  // NAPI_SHARED_UPSTREAM_JS_TEST_H_
