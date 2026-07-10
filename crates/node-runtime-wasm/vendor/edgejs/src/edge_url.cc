#include "edge_url.h"

#include <optional>
#include <string>
#include <string_view>

#include "ada.h"

namespace {

constexpr char kHexDigits[] = "0123456789ABCDEF";
constexpr uint32_t kUrlComponentsLength = 9;

struct UrlBindingState {
  napi_ref url_components_ref = nullptr;
};

enum UrlUpdateAction : uint32_t {
  kProtocol = 0,
  kHost = 1,
  kHostname = 2,
  kPort = 3,
  kUsername = 4,
  kPassword = 5,
  kPathname = 6,
  kSearch = 7,
  kHash = 8,
  kHref = 9,
};

napi_value GetUndefined(napi_env env) {
  napi_value out = nullptr;
  napi_get_undefined(env, &out);
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

bool ValueIsString(napi_env env, napi_value value) {
  napi_valuetype t = napi_undefined;
  return value != nullptr && napi_typeof(env, value, &t) == napi_ok && t == napi_string;
}

bool ValueToBool(napi_env env, napi_value value, bool default_value) {
  if (value == nullptr) return default_value;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_boolean) return default_value;
  bool out = default_value;
  if (napi_get_value_bool(env, value, &out) != napi_ok) return default_value;
  return out;
}

void SetNamedString(napi_env env, napi_value obj, const char* key, std::string_view value) {
  napi_value v = nullptr;
  if (napi_create_string_utf8(env, value.data(), value.size(), &v) == napi_ok && v != nullptr) {
    napi_set_named_property(env, obj, key, v);
  }
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb, void* data) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, data, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

bool IsUnreserved(unsigned char c) {
  switch (c) {
    case '\0':
    case '\t':
    case '\n':
    case '\r':
    case ' ':
    case '"':
    case '#':
    case '%':
    case '?':
    case '[':
    case '\\':
    case ']':
    case '^':
    case '|':
    case '~':
      return false;
    default:
      return true;
  }
}

std::string EncodePathChars(std::string_view input, bool windows) {
  std::string encoded = "file://";
  encoded.reserve(input.size() + 7);
  for (unsigned char ch : input) {
    if (windows && ch == '\\') {
      encoded.push_back('/');
      continue;
    }
    if (IsUnreserved(ch)) {
      encoded.push_back(static_cast<char>(ch));
      continue;
    }
    encoded.push_back('%');
    encoded.push_back(kHexDigits[(ch >> 4) & 0xF]);
    encoded.push_back(kHexDigits[ch & 0xF]);
  }
  return encoded;
}

void DeleteBindingState(napi_env env, void* data, void* /*hint*/) {
  auto* state = static_cast<UrlBindingState*>(data);
  if (state == nullptr) return;
  if (state->url_components_ref != nullptr) {
    napi_delete_reference(env, state->url_components_ref);
    state->url_components_ref = nullptr;
  }
  delete state;
}

UrlBindingState* GetBindingState(napi_env env, napi_callback_info info, size_t* argc, napi_value* argv) {
  void* data = nullptr;
  if (napi_get_cb_info(env, info, argc, argv, nullptr, &data) != napi_ok) return nullptr;
  return static_cast<UrlBindingState*>(data);
}

napi_value GetUrlComponentsArray(napi_env env, UrlBindingState* state) {
  if (state == nullptr || state->url_components_ref == nullptr) return nullptr;
  napi_value arr = nullptr;
  if (napi_get_reference_value(env, state->url_components_ref, &arr) != napi_ok || arr == nullptr) return nullptr;
  return arr;
}

void SetArrayU32(napi_env env, napi_value arr, uint32_t index, uint32_t value) {
  napi_value v = nullptr;
  if (napi_create_uint32(env, value, &v) == napi_ok && v != nullptr) {
    napi_set_element(env, arr, index, v);
  }
}

void UpdateComponents(napi_env env, UrlBindingState* state, const ada::url_components& c, ada::scheme::type type) {
  napi_value arr = GetUrlComponentsArray(env, state);
  if (arr == nullptr) return;
  SetArrayU32(env, arr, 0, static_cast<uint32_t>(c.protocol_end));
  SetArrayU32(env, arr, 1, static_cast<uint32_t>(c.username_end));
  SetArrayU32(env, arr, 2, static_cast<uint32_t>(c.host_start));
  SetArrayU32(env, arr, 3, static_cast<uint32_t>(c.host_end));
  SetArrayU32(env, arr, 4, static_cast<uint32_t>(c.port));
  SetArrayU32(env, arr, 5, static_cast<uint32_t>(c.pathname_start));
  SetArrayU32(env, arr, 6, static_cast<uint32_t>(c.search_start));
  SetArrayU32(env, arr, 7, static_cast<uint32_t>(c.hash_start));
  SetArrayU32(env, arr, 8, static_cast<uint32_t>(type));
}

void ThrowInvalidUrl(napi_env env, std::string_view input, const std::optional<std::string>& base = std::nullopt) {
  napi_value msg = nullptr;
  napi_value err = nullptr;
  if (napi_create_string_utf8(env, "Invalid URL", NAPI_AUTO_LENGTH, &msg) != napi_ok ||
      napi_create_type_error(env, nullptr, msg, &err) != napi_ok || err == nullptr) {
    return;
  }

  SetNamedString(env, err, "code", "ERR_INVALID_URL");
  SetNamedString(env, err, "input", input);
  if (base.has_value()) SetNamedString(env, err, "base", *base);
  napi_throw(env, err);
}

napi_value BindingDomainToASCII(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (GetBindingState(env, info, &argc, argv) == nullptr || argc < 1) return GetUndefined(env);
  std::string input = ValueToUtf8(env, argv[0]);
  napi_value ret = nullptr;
  if (input.empty()) {
    napi_create_string_utf8(env, "", 0, &ret);
    return ret;
  }
  auto out = ada::parse<ada::url>("ws://x");
  std::string result;
  if (out && out->set_hostname(input)) result = out->get_hostname();
  napi_create_string_utf8(env, result.c_str(), result.size(), &ret);
  return ret;
}

napi_value BindingDomainToUnicode(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (GetBindingState(env, info, &argc, argv) == nullptr || argc < 1) return GetUndefined(env);
  std::string input = ValueToUtf8(env, argv[0]);
  napi_value ret = nullptr;
  if (input.empty()) {
    napi_create_string_utf8(env, "", 0, &ret);
    return ret;
  }
  auto out = ada::parse<ada::url>("ws://x");
  std::string result;
  if (out && out->set_hostname(input)) result = ada::idna::to_unicode(out->get_hostname());
  napi_create_string_utf8(env, result.c_str(), result.size(), &ret);
  return ret;
}

napi_value BindingCanParse(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (GetBindingState(env, info, &argc, argv) == nullptr || argc < 1) {
    return GetUndefined(env);
  }
  std::string input = ValueToUtf8(env, argv[0]);
  bool out_bool = false;
  if (argc > 1 && ValueIsString(env, argv[1])) {
    std::string base = ValueToUtf8(env, argv[1]);
    std::string_view base_view(base);
    out_bool = ada::can_parse(input, &base_view);
  } else {
    out_bool = ada::can_parse(input);
  }
  napi_value out = nullptr;
  napi_get_boolean(env, out_bool, &out);
  return out;
}

napi_value BindingGetOrigin(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (GetBindingState(env, info, &argc, argv) == nullptr || argc < 1) return GetUndefined(env);
  std::string href = ValueToUtf8(env, argv[0]);
  auto parsed = ada::parse<ada::url_aggregator>(href);
  if (!parsed) {
    ThrowInvalidUrl(env, href);
    return GetUndefined(env);
  }
  const std::string origin(parsed->get_origin());
  napi_value out = nullptr;
  napi_create_string_utf8(env, origin.c_str(), origin.size(), &out);
  return out;
}

napi_value BindingFormat(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  if (GetBindingState(env, info, &argc, argv) == nullptr || argc < 1) return GetUndefined(env);

  std::string href = ValueToUtf8(env, argv[0]);
  bool hash = ValueToBool(env, argc > 1 ? argv[1] : nullptr, true);
  bool unicode = ValueToBool(env, argc > 2 ? argv[2] : nullptr, false);
  bool search = ValueToBool(env, argc > 3 ? argv[3] : nullptr, true);
  bool auth = ValueToBool(env, argc > 4 ? argv[4] : nullptr, true);

  auto out = ada::parse<ada::url>(href);
  if (!out) {
    ThrowInvalidUrl(env, href);
    return GetUndefined(env);
  }
  if (!hash) out->hash = std::nullopt;
  if (unicode && out->has_hostname()) out->host = ada::idna::to_unicode(out->get_hostname());
  if (!search) out->query = std::nullopt;
  if (!auth) {
    out->username = "";
    out->password = "";
  }
  const std::string result(out->get_href());
  napi_value ret = nullptr;
  napi_create_string_utf8(env, result.c_str(), result.size(), &ret);
  return ret;
}

napi_value BindingParse(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  UrlBindingState* state = GetBindingState(env, info, &argc, argv);
  if (state == nullptr || argc < 1) return GetUndefined(env);

  std::string input = ValueToUtf8(env, argv[0]);
  bool raise_exception = ValueToBool(env, argc > 2 ? argv[2] : nullptr, false);

  std::optional<std::string> base;
  ada::result<ada::url_aggregator> base_parsed;
  ada::url_aggregator* base_ptr = nullptr;

  if (argc > 1 && ValueIsString(env, argv[1])) {
    base = ValueToUtf8(env, argv[1]);
    base_parsed = ada::parse<ada::url_aggregator>(*base);
    if (!base_parsed) {
      if (raise_exception) ThrowInvalidUrl(env, input, base);
      return GetUndefined(env);
    }
    base_ptr = &base_parsed.value();
  }

  auto out = ada::parse<ada::url_aggregator>(input, base_ptr);
  if (!out) {
    if (raise_exception) ThrowInvalidUrl(env, input, base);
    return GetUndefined(env);
  }

  UpdateComponents(env, state, out->get_components(), out->type);
  const std::string href(out->get_href());
  napi_value ret = nullptr;
  napi_create_string_utf8(env, href.c_str(), href.size(), &ret);
  return ret;
}

napi_value BindingPathToFileURL(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  UrlBindingState* state = GetBindingState(env, info, &argc, argv);
  if (state == nullptr || argc < 2) return GetUndefined(env);

  std::string input = ValueToUtf8(env, argv[0]);
  bool windows = ValueToBool(env, argv[1], false);

  auto out = ada::parse<ada::url_aggregator>(EncodePathChars(input, windows), nullptr);
  if (!out) {
    ThrowInvalidUrl(env, input);
    return GetUndefined(env);
  }

  if (windows && argc > 2 && ValueIsString(env, argv[2])) {
    std::string host = ValueToUtf8(env, argv[2]);
    if (!host.empty()) out->set_hostname(host);
  }

  UpdateComponents(env, state, out->get_components(), out->type);
  const std::string href(out->get_href());
  napi_value ret = nullptr;
  napi_create_string_utf8(env, href.c_str(), href.size(), &ret);
  return ret;
}

napi_value BindingUpdate(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  UrlBindingState* state = GetBindingState(env, info, &argc, argv);
  if (state == nullptr || argc < 3) return GetUndefined(env);

  std::string href = ValueToUtf8(env, argv[0]);
  uint32_t action = 0;
  if (napi_get_value_uint32(env, argv[1], &action) != napi_ok) return GetUndefined(env);
  std::string new_value = ValueToUtf8(env, argv[2]);
  std::string_view new_value_view(new_value);

  auto out = ada::parse<ada::url_aggregator>(href);
  if (!out) {
    napi_value f = nullptr;
    napi_get_boolean(env, false, &f);
    return f;
  }

  bool result = true;
  switch (action) {
    case kPathname:
      result = out->set_pathname(new_value_view);
      break;
    case kHash:
      out->set_hash(new_value_view);
      break;
    case kHost:
      result = out->set_host(new_value_view);
      break;
    case kHostname:
      result = out->set_hostname(new_value_view);
      break;
    case kHref:
      result = out->set_href(new_value_view);
      break;
    case kPassword:
      result = out->set_password(new_value_view);
      break;
    case kPort:
      result = out->set_port(new_value_view);
      break;
    case kProtocol:
      result = out->set_protocol(new_value_view);
      break;
    case kSearch:
      out->set_search(new_value_view);
      break;
    case kUsername:
      result = out->set_username(new_value_view);
      break;
    default:
      result = false;
      break;
  }

  if (!result) {
    napi_value f = nullptr;
    napi_get_boolean(env, false, &f);
    return f;
  }

  UpdateComponents(env, state, out->get_components(), out->type);
  const std::string result_href(out->get_href());
  napi_value ret = nullptr;
  napi_create_string_utf8(env, result_href.c_str(), result_href.size(), &ret);
  return ret;
}

}  // namespace

napi_value EdgeInstallUrlBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  auto* state = new UrlBindingState();
  if (napi_wrap(env, binding, state, DeleteBindingState, nullptr, nullptr) != napi_ok) {
    delete state;
    return nullptr;
  }

  napi_value components = nullptr;
  void* components_data = nullptr;
  napi_value components_ab = nullptr;
  if (napi_create_arraybuffer(
          env, sizeof(uint32_t) * kUrlComponentsLength, &components_data, &components_ab) == napi_ok &&
      components_data != nullptr && components_ab != nullptr &&
      napi_create_typedarray(
          env, napi_uint32_array, kUrlComponentsLength, components_ab, 0, &components) == napi_ok &&
      components != nullptr) {
    auto* values = static_cast<uint32_t*>(components_data);
    for (uint32_t i = 0; i < kUrlComponentsLength; ++i) values[i] = 0;
    napi_set_named_property(env, binding, "urlComponents", components);
    napi_create_reference(env, components, 1, &state->url_components_ref);
  }

  SetMethod(env, binding, "format", BindingFormat, state);
  SetMethod(env, binding, "domainToASCII", BindingDomainToASCII, state);
  SetMethod(env, binding, "domainToUnicode", BindingDomainToUnicode, state);
  SetMethod(env, binding, "pathToFileURL", BindingPathToFileURL, state);
  SetMethod(env, binding, "parse", BindingParse, state);
  SetMethod(env, binding, "update", BindingUpdate, state);
  SetMethod(env, binding, "canParse", BindingCanParse, state);
  SetMethod(env, binding, "getOrigin", BindingGetOrigin, state);

  return binding;
}

namespace edge_url {

std::string PathToFileURLString(std::string_view absolute_path) {
#if defined(_WIN32)
  constexpr bool windows = true;
#else
  constexpr bool windows = false;
#endif
  auto out = ada::parse<ada::url_aggregator>(EncodePathChars(absolute_path, windows), nullptr);
  if (!out) return std::string();
  return std::string(out->get_href());
}

}  // namespace edge_url
