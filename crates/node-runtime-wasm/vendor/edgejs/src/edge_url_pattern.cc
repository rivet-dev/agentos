#include "edge_url_pattern.h"

#include <cstring>
#include <optional>
#include <string>
#include <string_view>
#include <variant>
#include <vector>

#include "ada.h"

namespace {

using RegexProvider = ada::url_pattern_regex::std_regex_provider;

struct UrlPatternWrap {
  ada::url_pattern<RegexProvider> pattern;
};

constexpr const char* kUrlPatternComponents[] = {
    "protocol",
    "username",
    "password",
    "hostname",
    "port",
    "pathname",
    "search",
    "hash",
};

napi_value GetUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
  return out;
}

napi_value GetNull(napi_env env) {
  napi_value out = nullptr;
  napi_get_null(env, &out);
  return out;
}

std::string ValueToUtf8(napi_env env, napi_value value) {
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

bool ValueIsType(napi_env env, napi_value value, napi_valuetype expected) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == expected;
}

bool ValueIsString(napi_env env, napi_value value) { return ValueIsType(env, value, napi_string); }

bool ValueIsBoolean(napi_env env, napi_value value) { return ValueIsType(env, value, napi_boolean); }

bool ValueIsObject(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  if (type != napi_object && type != napi_function) return false;
  bool is_array = false;
  if (napi_is_array(env, value, &is_array) == napi_ok && is_array) return true;
  return true;
}

napi_value CreateString(napi_env env, std::string_view text) {
  napi_value out = nullptr;
  napi_create_string_utf8(env, text.data(), text.size(), &out);
  return out;
}

bool SetNamedValue(napi_env env, napi_value obj, const char* key, napi_value value) {
  return obj != nullptr && value != nullptr && napi_set_named_property(env, obj, key, value) == napi_ok;
}

bool SetNamedString(napi_env env, napi_value obj, const char* key, std::string_view value) {
  return SetNamedValue(env, obj, key, CreateString(env, value));
}

void ThrowTypeErrorWithCode(napi_env env, const char* code, std::string_view message) {
  napi_throw_type_error(env, code, std::string(message).c_str());
}

void ThrowConstructCallRequired(napi_env env) {
  ThrowTypeErrorWithCode(env, "ERR_CONSTRUCT_CALL_REQUIRED", "Cannot call constructor without `new`");
}

void ThrowInvalidArgType(napi_env env, std::string_view message) {
  ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", message);
}

void ThrowInvalidUrlPattern(napi_env env) {
  ThrowTypeErrorWithCode(env, "ERR_INVALID_URL_PATTERN", "Failed to construct URLPattern");
}

void ThrowOperationFailed(napi_env env, std::string_view message) {
  ThrowTypeErrorWithCode(env, "ERR_OPERATION_FAILED", std::string("Operation failed: ") + std::string(message));
}

void ThrowIllegalInvocation(napi_env env) {
  napi_throw_type_error(env, nullptr, "Illegal invocation");
}

napi_value CreateNullProtoObject(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value object_ctor = nullptr;
  if (napi_get_named_property(env, global, "Object", &object_ctor) != napi_ok || object_ctor == nullptr) {
    return nullptr;
  }

  napi_value create_fn = nullptr;
  if (napi_get_named_property(env, object_ctor, "create", &create_fn) != napi_ok || create_fn == nullptr) {
    return nullptr;
  }

  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  napi_value argv[1] = {null_value};
  napi_value out = nullptr;
  if (napi_call_function(env, object_ctor, create_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

void UrlPatternFinalize(napi_env /*env*/, void* data, void* /*hint*/) {
  delete static_cast<UrlPatternWrap*>(data);
}

bool UnwrapUrlPattern(napi_env env, napi_value value, UrlPatternWrap** out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (value == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  UrlPatternWrap* wrap = nullptr;
  if (napi_unwrap(env, value, reinterpret_cast<void**>(&wrap)) != napi_ok || wrap == nullptr) {
    ThrowIllegalInvocation(env);
    return false;
  }
  *out = wrap;
  return true;
}

bool ParseUrlPatternInitObject(napi_env env, napi_value obj, ada::url_pattern_init* out) {
  if (out == nullptr) return false;
  for (const char* key : kUrlPatternComponents) {
    napi_value value = nullptr;
    if (napi_get_named_property(env, obj, key, &value) != napi_ok) return false;
    if (!ValueIsString(env, value)) continue;
    const std::string text = ValueToUtf8(env, value);
    if (std::strcmp(key, "protocol") == 0) out->protocol = text;
    else if (std::strcmp(key, "username") == 0) out->username = text;
    else if (std::strcmp(key, "password") == 0) out->password = text;
    else if (std::strcmp(key, "hostname") == 0) out->hostname = text;
    else if (std::strcmp(key, "port") == 0) out->port = text;
    else if (std::strcmp(key, "pathname") == 0) out->pathname = text;
    else if (std::strcmp(key, "search") == 0) out->search = text;
    else if (std::strcmp(key, "hash") == 0) out->hash = text;
  }

  napi_value base_url = nullptr;
  if (napi_get_named_property(env, obj, "baseURL", &base_url) != napi_ok) return false;
  if (ValueIsString(env, base_url)) {
    out->base_url = ValueToUtf8(env, base_url);
  }
  return true;
}

bool ParseUrlPatternOptionsObject(napi_env env, napi_value obj, ada::url_pattern_options* out) {
  if (out == nullptr) return false;
  napi_value ignore_case = nullptr;
  if (napi_get_named_property(env, obj, "ignoreCase", &ignore_case) != napi_ok) return false;
  if (!ValueIsBoolean(env, ignore_case)) {
    ThrowInvalidArgType(env, "options.ignoreCase must be a boolean");
    return false;
  }
  bool value = false;
  if (napi_get_value_bool(env, ignore_case, &value) != napi_ok) return false;
  out->ignore_case = value;
  return true;
}

std::string_view GetComponentValue(const UrlPatternWrap* wrap, const char* component) {
  if (wrap == nullptr || component == nullptr) return {};
  if (std::strcmp(component, "protocol") == 0) return wrap->pattern.get_protocol();
  if (std::strcmp(component, "username") == 0) return wrap->pattern.get_username();
  if (std::strcmp(component, "password") == 0) return wrap->pattern.get_password();
  if (std::strcmp(component, "hostname") == 0) return wrap->pattern.get_hostname();
  if (std::strcmp(component, "port") == 0) return wrap->pattern.get_port();
  if (std::strcmp(component, "pathname") == 0) return wrap->pattern.get_pathname();
  if (std::strcmp(component, "search") == 0) return wrap->pattern.get_search();
  if (std::strcmp(component, "hash") == 0) return wrap->pattern.get_hash();
  return {};
}

bool AddInputValue(napi_env env, napi_value array, uint32_t index, const ada::url_pattern_input& input) {
  napi_value value = nullptr;
  if (std::holds_alternative<std::string_view>(input)) {
    value = CreateString(env, std::get<std::string_view>(input));
  } else {
    value = CreateNullProtoObject(env);
    if (value == nullptr) return false;
    const auto& init = std::get<ada::url_pattern_init>(input);
    if (init.protocol.has_value() && !SetNamedString(env, value, "protocol", *init.protocol)) return false;
    if (init.username.has_value() && !SetNamedString(env, value, "username", *init.username)) return false;
    if (init.password.has_value() && !SetNamedString(env, value, "password", *init.password)) return false;
    if (init.hostname.has_value() && !SetNamedString(env, value, "hostname", *init.hostname)) return false;
    if (init.port.has_value() && !SetNamedString(env, value, "port", *init.port)) return false;
    if (init.pathname.has_value() && !SetNamedString(env, value, "pathname", *init.pathname)) return false;
    if (init.search.has_value() && !SetNamedString(env, value, "search", *init.search)) return false;
    if (init.hash.has_value() && !SetNamedString(env, value, "hash", *init.hash)) return false;
    if (init.base_url.has_value() && !SetNamedString(env, value, "baseURL", *init.base_url)) return false;
  }
  return napi_set_element(env, array, index, value) == napi_ok;
}

bool AddComponentResult(napi_env env,
                        napi_value obj,
                        const char* name,
                        const ada::url_pattern_component_result& result) {
  napi_value component = CreateNullProtoObject(env);
  if (component == nullptr) return false;
  if (!SetNamedString(env, component, "input", result.input)) return false;

  napi_value groups = CreateNullProtoObject(env);
  if (groups == nullptr) return false;
  for (const auto& [group_name, group_value] : result.groups) {
    napi_value value = group_value.has_value() ? CreateString(env, *group_value) : GetUndefined(env);
    if (value == nullptr || napi_set_named_property(env, groups, group_name.c_str(), value) != napi_ok) {
      return false;
    }
  }
  if (!SetNamedValue(env, component, "groups", groups)) return false;
  return SetNamedValue(env, obj, name, component);
}

napi_value UrlPatternGetter(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  void* data = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, &data) != napi_ok) return nullptr;

  UrlPatternWrap* wrap = nullptr;
  if (!UnwrapUrlPattern(env, this_arg, &wrap)) return nullptr;

  const char* component = static_cast<const char*>(data);
  return CreateString(env, GetComponentValue(wrap, component));
}

napi_value UrlPatternHasRegExpGroupsGetter(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr) != napi_ok) return nullptr;

  UrlPatternWrap* wrap = nullptr;
  if (!UnwrapUrlPattern(env, this_arg, &wrap)) return nullptr;

  napi_value out = nullptr;
  napi_get_boolean(env, wrap->pattern.has_regexp_groups(), &out);
  return out;
}

napi_value UrlPatternExec(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  UrlPatternWrap* wrap = nullptr;
  if (!UnwrapUrlPattern(env, this_arg, &wrap)) return nullptr;

  ada::url_pattern_input input;
  std::string input_storage;
  std::optional<std::string> base_url_storage;

  if (argc == 0) {
    input = ada::url_pattern_init{};
  } else if (ValueIsString(env, argv[0])) {
    input_storage = ValueToUtf8(env, argv[0]);
    input = std::string_view(input_storage);
  } else if (ValueIsObject(env, argv[0])) {
    ada::url_pattern_init init{};
    if (!ParseUrlPatternInitObject(env, argv[0], &init)) return nullptr;
    input = std::move(init);
  } else {
    ThrowInvalidArgType(env, "URLPattern input needs to be a string or an object");
    return nullptr;
  }

  if (argc > 1) {
    if (!ValueIsString(env, argv[1])) {
      ThrowInvalidArgType(env, "baseURL must be a string");
      return nullptr;
    }
    base_url_storage = ValueToUtf8(env, argv[1]);
  }

  const std::string_view* base_url_ptr = nullptr;
  std::string_view base_url_view;
  if (base_url_storage.has_value()) {
    base_url_view = *base_url_storage;
    base_url_ptr = &base_url_view;
  }

  auto result = wrap->pattern.exec(input, base_url_ptr);
  if (!result.has_value()) {
    ThrowOperationFailed(env, "Failed to exec URLPattern");
    return nullptr;
  }
  if (!result->has_value()) {
    return GetNull(env);
  }

  napi_value out = CreateNullProtoObject(env);
  if (out == nullptr) return nullptr;

  napi_value inputs = nullptr;
  if (napi_create_array_with_length(env, result->value().inputs.size(), &inputs) != napi_ok || inputs == nullptr) {
    return nullptr;
  }
  for (uint32_t i = 0; i < result->value().inputs.size(); ++i) {
    if (!AddInputValue(env, inputs, i, result->value().inputs[i])) return nullptr;
  }
  if (!SetNamedValue(env, out, "inputs", inputs)) return nullptr;

  if (!AddComponentResult(env, out, "protocol", result->value().protocol)) return nullptr;
  if (!AddComponentResult(env, out, "username", result->value().username)) return nullptr;
  if (!AddComponentResult(env, out, "password", result->value().password)) return nullptr;
  if (!AddComponentResult(env, out, "hostname", result->value().hostname)) return nullptr;
  if (!AddComponentResult(env, out, "port", result->value().port)) return nullptr;
  if (!AddComponentResult(env, out, "pathname", result->value().pathname)) return nullptr;
  if (!AddComponentResult(env, out, "search", result->value().search)) return nullptr;
  if (!AddComponentResult(env, out, "hash", result->value().hash)) return nullptr;

  return out;
}

napi_value UrlPatternTest(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  UrlPatternWrap* wrap = nullptr;
  if (!UnwrapUrlPattern(env, this_arg, &wrap)) return nullptr;

  ada::url_pattern_input input;
  std::string input_storage;
  std::optional<std::string> base_url_storage;

  if (argc == 0) {
    input = ada::url_pattern_init{};
  } else if (ValueIsString(env, argv[0])) {
    input_storage = ValueToUtf8(env, argv[0]);
    input = std::string_view(input_storage);
  } else if (ValueIsObject(env, argv[0])) {
    ada::url_pattern_init init{};
    if (!ParseUrlPatternInitObject(env, argv[0], &init)) return nullptr;
    input = std::move(init);
  } else {
    ThrowInvalidArgType(env, "URLPattern input needs to be a string or an object");
    return nullptr;
  }

  if (argc > 1) {
    if (!ValueIsString(env, argv[1])) {
      ThrowInvalidArgType(env, "baseURL must be a string");
      return nullptr;
    }
    base_url_storage = ValueToUtf8(env, argv[1]);
  }

  const std::string_view* base_url_ptr = nullptr;
  std::string_view base_url_view;
  if (base_url_storage.has_value()) {
    base_url_view = *base_url_storage;
    base_url_ptr = &base_url_view;
  }

  auto result = wrap->pattern.test(input, base_url_ptr);
  if (!result.has_value()) {
    ThrowOperationFailed(env, "Failed to test URLPattern");
    return nullptr;
  }

  napi_value out = nullptr;
  napi_get_boolean(env, *result, &out);
  return out;
}

napi_value UrlPatternConstructor(napi_env env, napi_callback_info info) {
  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) != napi_ok) return nullptr;
  if (new_target == nullptr) {
    ThrowConstructCallRequired(env);
    return nullptr;
  }

  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok) return nullptr;

  std::optional<ada::url_pattern_init> init;
  std::optional<std::string> input_storage;
  std::optional<std::string> base_url_storage;
  std::optional<ada::url_pattern_options> options;

  if (argc == 0) {
    init = ada::url_pattern_init{};
  } else if (ValueIsString(env, argv[0])) {
    input_storage = ValueToUtf8(env, argv[0]);
  } else if (ValueIsObject(env, argv[0])) {
    ada::url_pattern_init input_init{};
    if (!ParseUrlPatternInitObject(env, argv[0], &input_init)) return nullptr;
    init = std::move(input_init);
  } else {
    ThrowInvalidArgType(env, "Input must be an object or a string");
    return nullptr;
  }

  if (argc > 1) {
    if (ValueIsString(env, argv[1])) {
      base_url_storage = ValueToUtf8(env, argv[1]);
    } else if (ValueIsObject(env, argv[1])) {
      ada::url_pattern_options parsed_options{};
      if (!ParseUrlPatternOptionsObject(env, argv[1], &parsed_options)) return nullptr;
      options = parsed_options;
    } else {
      ThrowInvalidArgType(env, "second argument must be a string or object");
      return nullptr;
    }

    if (argc > 2) {
      if (!ValueIsObject(env, argv[2])) {
        ThrowInvalidArgType(env, "options must be an object");
        return nullptr;
      }
      ada::url_pattern_options parsed_options{};
      if (!ParseUrlPatternOptionsObject(env, argv[2], &parsed_options)) return nullptr;
      options = parsed_options;
    }
  }

  std::string_view base_url_view;
  const std::string_view* base_url_ptr = nullptr;
  if (base_url_storage.has_value()) {
    base_url_view = *base_url_storage;
    base_url_ptr = &base_url_view;
  }

  ada::url_pattern_input input;
  if (init.has_value()) {
    input = std::move(*init);
  } else {
    input = std::string_view(*input_storage);
  }

  auto parsed = ada::parse_url_pattern<RegexProvider>(
      std::move(input), base_url_ptr, options.has_value() ? &*options : nullptr);
  if (!parsed.has_value()) {
    ThrowInvalidUrlPattern(env);
    return nullptr;
  }

  auto* wrap = new UrlPatternWrap{std::move(*parsed)};
  if (napi_wrap(env, this_arg, wrap, UrlPatternFinalize, nullptr, nullptr) != napi_ok) {
    delete wrap;
    return nullptr;
  }
  return this_arg;
}

}  // namespace

napi_value EdgeInstallUrlPatternBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  napi_property_descriptor properties[] = {
      {"protocol", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("protocol")},
      {"username", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("username")},
      {"password", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("password")},
      {"hostname", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("hostname")},
      {"port", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("port")},
      {"pathname", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("pathname")},
      {"search", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("search")},
      {"hash", nullptr, nullptr, UrlPatternGetter, nullptr, nullptr, napi_default, const_cast<char*>("hash")},
      {"hasRegExpGroups", nullptr, nullptr, UrlPatternHasRegExpGroupsGetter, nullptr, nullptr, napi_default, nullptr},
      {"exec", nullptr, UrlPatternExec, nullptr, nullptr, nullptr, napi_default_method, nullptr},
      {"test", nullptr, UrlPatternTest, nullptr, nullptr, nullptr, napi_default_method, nullptr},
  };

  napi_value ctor = nullptr;
  if (napi_define_class(env,
                        "URLPattern",
                        NAPI_AUTO_LENGTH,
                        UrlPatternConstructor,
                        nullptr,
                        sizeof(properties) / sizeof(properties[0]),
                        properties,
                        &ctor) != napi_ok ||
      ctor == nullptr) {
    return nullptr;
  }

  napi_set_named_property(env, binding, "URLPattern", ctor);
  return binding;
}
