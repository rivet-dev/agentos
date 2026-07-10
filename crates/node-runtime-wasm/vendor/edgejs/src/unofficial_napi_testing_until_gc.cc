#include <limits>
#include <string>

#include "unofficial_napi.h"

namespace {

napi_value MakeError(napi_env env, const char* msg) {
  napi_value str = nullptr;
  napi_value err = nullptr;
  if (napi_create_string_utf8(env, msg, NAPI_AUTO_LENGTH, &str) != napi_ok) return nullptr;
  if (napi_create_error(env, nullptr, str, &err) != napi_ok) return nullptr;
  return err;
}

napi_value MakeError(napi_env env, const std::string& msg) {
  return MakeError(env, msg.c_str());
}

napi_value UnofficialNapiTestingUntilGc(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    napi_throw_type_error(env, nullptr, "Unable to read callback arguments");
    return nullptr;
  }

  if (argc < 2) {
    napi_throw_type_error(env, nullptr, "Expected (name, conditionFn, [maxCount], [options])");
    return nullptr;
  }

  size_t name_len = 0;
  if (napi_get_value_string_utf8(env, argv[0], nullptr, 0, &name_len) != napi_ok) {
    napi_throw_type_error(env, nullptr, "name must be a string");
    return nullptr;
  }
  std::string name(name_len, '\0');
  napi_get_value_string_utf8(env, argv[0], name.data(), name_len + 1, &name_len);

  napi_valuetype fn_type = napi_undefined;
  if (napi_typeof(env, argv[1], &fn_type) != napi_ok || fn_type != napi_function) {
    napi_throw_type_error(env, nullptr, "conditionFn must be a function");
    return nullptr;
  }

  uint32_t remaining = 10;
  if (argc >= 3) {
    napi_valuetype max_type = napi_undefined;
    if (napi_typeof(env, argv[2], &max_type) != napi_ok) {
      napi_throw_type_error(env, nullptr, "Unable to read maxCount type");
      return nullptr;
    }
    if (max_type == napi_number) {
      double count = 0;
      if (napi_get_value_double(env, argv[2], &count) != napi_ok) {
        napi_throw_type_error(env, nullptr, "maxCount must be a number (or Infinity)");
        return nullptr;
      }
      if (count == std::numeric_limits<double>::infinity()) {
        remaining = std::numeric_limits<uint32_t>::max();
      } else if (count < 0) {
        napi_throw_range_error(env, nullptr, "maxCount must be >= 0");
        return nullptr;
      } else if (count > static_cast<double>(std::numeric_limits<uint32_t>::max() - 1)) {
        remaining = std::numeric_limits<uint32_t>::max();
      } else {
        remaining = static_cast<uint32_t>(count);
      }
    } else if (max_type != napi_undefined) {
      napi_throw_type_error(env, nullptr, "maxCount must be a number (or Infinity)");
      return nullptr;
    }
  }

  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok) {
    napi_throw_error(env, nullptr, "Unable to create promise");
    return nullptr;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    napi_value err = MakeError(env, "Unable to acquire global object");
    napi_reject_deferred(env, deferred, err);
    return promise;
  }

  while (true) {
    napi_value result = nullptr;
    napi_status call_status = napi_call_function(env, global, argv[1], 0, nullptr, &result);
    if (call_status != napi_ok) {
      napi_value err = nullptr;
      napi_value pending = nullptr;
      if (napi_get_and_clear_last_exception(env, &pending) == napi_ok && pending != nullptr) {
        err = pending;
      } else {
        err = MakeError(env, "conditionFn threw");
      }
      napi_reject_deferred(env, deferred, err);
      return promise;
    }

    bool ok = false;
    if (napi_get_value_bool(env, result, &ok) != napi_ok) {
      napi_value err = MakeError(env, "conditionFn did not return a boolean");
      napi_reject_deferred(env, deferred, err);
      return promise;
    }

    if (ok) {
      napi_value undef = nullptr;
      napi_get_undefined(env, &undef);
      napi_resolve_deferred(env, deferred, undef);
      return promise;
    }

    if (remaining == 0) {
      napi_value err = MakeError(env, std::string("Test ") + name + " failed");
      napi_reject_deferred(env, deferred, err);
      return promise;
    }

    if (remaining != std::numeric_limits<uint32_t>::max()) {
      remaining--;
    }

    if (unofficial_napi_request_gc_for_testing(env) != napi_ok) {
      napi_value err = MakeError(env, "unofficial_napi_request_gc_for_testing failed");
      napi_reject_deferred(env, deferred, err);
      return promise;
    }
  }
}

}  // namespace

napi_status EdgeInstallUnofficialNapiTestingUntilGc(napi_env env, napi_value target) {
  if (env == nullptr || target == nullptr) return napi_invalid_arg;

  napi_value fn = nullptr;
  napi_status status = napi_create_function(env,
                                            "unofficial_napi_testing_until_gc",
                                            NAPI_AUTO_LENGTH,
                                            UnofficialNapiTestingUntilGc,
                                            nullptr,
                                            &fn);
  if (status != napi_ok || fn == nullptr) {
    return (status == napi_ok) ? napi_generic_failure : status;
  }
  return napi_set_named_property(env, target, "unofficial_napi_testing_until_gc", fn);
}
