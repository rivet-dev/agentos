#include "internal_binding/binding_initializers.h"

#include <cstdlib>
#include <cerrno>
#include <climits>
#include <cstring>
#include <string>
#include <vector>

#if !defined(_WIN32)
#include <grp.h>
#include <pwd.h>
#include <sys/types.h>
#include <unistd.h>
#endif

#include "internal_binding/helpers.h"

namespace internal_binding {

namespace {

std::string ValueToUtf8(napi_env env, napi_value value) {
  if (value == nullptr) return {};
  size_t len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return {};
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, out.data(), out.size(), &copied) != napi_ok) return {};
  out.resize(copied);
  return out;
}

void ThrowFeatureUnavailable(napi_env env, const char* name) {
  const std::string msg = std::string(name) + " is not supported on this platform";
  napi_throw_error(env, "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM", msg.c_str());
}

bool GetUint32Arg(napi_env env, napi_value value, uint32_t* out) {
  if (value == nullptr || out == nullptr) return false;
  return napi_get_value_uint32(env, value, out) == napi_ok;
}

const char* ErrnoName(int err) {
  switch (err) {
#if defined(EPERM)
    case EPERM:
      return "EPERM";
#endif
#if defined(EACCES)
    case EACCES:
      return "EACCES";
#endif
#if defined(EINVAL)
    case EINVAL:
      return "EINVAL";
#endif
#if defined(ESRCH)
    case ESRCH:
      return "ESRCH";
#endif
    default:
      return "UNKNOWN";
  }
}

void ThrowErrnoSystemError(napi_env env, int err) {
  if (err == 0) err = EINVAL;
  std::string msg = std::string(ErrnoName(err)) + ", " + std::strerror(err);
  napi_throw_error(env, "ERR_SYSTEM_ERROR", msg.c_str());
}

#if !defined(_WIN32)
enum class IdResolveStatus { kOk, kUnknown, kInvalidType };

bool ParseUnsignedId(const std::string& raw, uint32_t* out) {
  if (raw.empty() || out == nullptr) return false;
  char* end = nullptr;
  errno = 0;
  unsigned long parsed = std::strtoul(raw.c_str(), &end, 10);
  if (errno != 0 || end == raw.c_str() || *end != '\0' || parsed > UINT_MAX) {
    return false;
  }
  *out = static_cast<uint32_t>(parsed);
  return true;
}

IdResolveStatus ResolveUidValue(napi_env env, napi_value value, uid_t* out_uid) {
  if (value == nullptr || out_uid == nullptr) return IdResolveStatus::kInvalidType;
  uint32_t numeric = 0;
  if (napi_get_value_uint32(env, value, &numeric) == napi_ok) {
    *out_uid = static_cast<uid_t>(numeric);
    return IdResolveStatus::kOk;
  }
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_string) return IdResolveStatus::kInvalidType;

  const std::string raw = ValueToUtf8(env, value);
  uint32_t parsed = 0;
  if (ParseUnsignedId(raw, &parsed)) {
    *out_uid = static_cast<uid_t>(parsed);
    return IdResolveStatus::kOk;
  }

  passwd* pw = ::getpwnam(raw.c_str());
  if (pw == nullptr) return IdResolveStatus::kUnknown;
  *out_uid = pw->pw_uid;
  return IdResolveStatus::kOk;
}

IdResolveStatus ResolveGidValue(napi_env env, napi_value value, gid_t* out_gid) {
  if (value == nullptr || out_gid == nullptr) return IdResolveStatus::kInvalidType;
  uint32_t numeric = 0;
  if (napi_get_value_uint32(env, value, &numeric) == napi_ok) {
    *out_gid = static_cast<gid_t>(numeric);
    return IdResolveStatus::kOk;
  }
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_string) return IdResolveStatus::kInvalidType;

  const std::string raw = ValueToUtf8(env, value);
  uint32_t parsed = 0;
  if (ParseUnsignedId(raw, &parsed)) {
    *out_gid = static_cast<gid_t>(parsed);
    return IdResolveStatus::kOk;
  }

  group* gr = ::getgrnam(raw.c_str());
  if (gr == nullptr) return IdResolveStatus::kUnknown;
  *out_gid = gr->gr_gid;
  return IdResolveStatus::kOk;
}

IdResolveStatus ResolveUserNameForInitgroups(napi_env env, napi_value value, std::string* out_user) {
  if (value == nullptr || out_user == nullptr) return IdResolveStatus::kInvalidType;

  uint32_t numeric = 0;
  if (napi_get_value_uint32(env, value, &numeric) == napi_ok) {
    passwd* pw = ::getpwuid(static_cast<uid_t>(numeric));
    if (pw == nullptr || pw->pw_name == nullptr) return IdResolveStatus::kUnknown;
    *out_user = pw->pw_name;
    return IdResolveStatus::kOk;
  }

  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, value, &t) != napi_ok || t != napi_string) return IdResolveStatus::kInvalidType;

  const std::string raw = ValueToUtf8(env, value);
  uint32_t parsed = 0;
  if (ParseUnsignedId(raw, &parsed)) {
    passwd* pw = ::getpwuid(static_cast<uid_t>(parsed));
    if (pw == nullptr || pw->pw_name == nullptr) return IdResolveStatus::kUnknown;
    *out_user = pw->pw_name;
    return IdResolveStatus::kOk;
  }

  passwd* pw = ::getpwnam(raw.c_str());
  if (pw == nullptr || pw->pw_name == nullptr) return IdResolveStatus::kUnknown;
  *out_user = pw->pw_name;
  return IdResolveStatus::kOk;
}
#endif

napi_value ReadProcessEnv(napi_env env, const std::string& key) {
  if (key.empty()) return nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value process = nullptr;
  if (napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) return nullptr;

  napi_value env_obj = nullptr;
  if (napi_get_named_property(env, process, "env", &env_obj) != napi_ok || env_obj == nullptr) return nullptr;

  napi_value key_value = nullptr;
  if (napi_create_string_utf8(env, key.c_str(), key.size(), &key_value) != napi_ok || key_value == nullptr) {
    return nullptr;
  }

  bool has_key = false;
  if (napi_has_property(env, env_obj, key_value, &has_key) != napi_ok || !has_key) return nullptr;

  napi_value out = nullptr;
  if (napi_get_property(env, env_obj, key_value, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

napi_value CredentialsSafeGetenv(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return Undefined(env);
  const std::string key = ValueToUtf8(env, argv[0]);
  if (key.empty()) return Undefined(env);

  napi_value js_env_value = ReadProcessEnv(env, key);
  if (js_env_value != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, js_env_value, &type) == napi_ok &&
        type != napi_undefined &&
        type != napi_null) {
      napi_value coerced = nullptr;
      if (napi_coerce_to_string(env, js_env_value, &coerced) == napi_ok && coerced != nullptr) {
        return coerced;
      }
    }
  }

  const char* value = std::getenv(key.c_str());
  if (value == nullptr) return Undefined(env);
  napi_value out = nullptr;
  napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CredentialsGetTempDir(napi_env env, napi_callback_info /*info*/) {
  const char* candidates[] = {"TMPDIR", "TMP", "TEMP"};
  std::string value_storage;
  const char* value = nullptr;
  for (const char* key : candidates) {
    napi_value js_env_value = ReadProcessEnv(env, key);
    if (js_env_value != nullptr) {
      napi_valuetype type = napi_undefined;
      if (napi_typeof(env, js_env_value, &type) == napi_ok &&
          type != napi_undefined &&
          type != napi_null) {
        napi_value coerced = nullptr;
        if (napi_coerce_to_string(env, js_env_value, &coerced) == napi_ok && coerced != nullptr) {
          value_storage = ValueToUtf8(env, coerced);
          if (!value_storage.empty()) {
            value = value_storage.c_str();
            break;
          }
        }
      }

      // Match Node semantics: once process.env has an entry for this key,
      // do not fall back to the host environment's value for the same key.
      continue;
    }

    const char* candidate = std::getenv(key);
    if (candidate != nullptr && *candidate != '\0') {
      value = candidate;
      break;
    }
  }

  const char* fallback =
#if defined(_WIN32)
      "C:\\Temp";
#else
      "/tmp";
#endif
  if (value == nullptr) value = fallback;
  std::string normalized = value;
#if !defined(_WIN32)
  if (normalized.size() > 1 && normalized.back() == '/') {
    normalized.pop_back();
  }
#endif
  napi_value out = nullptr;
  napi_create_string_utf8(env, normalized.c_str(), NAPI_AUTO_LENGTH, &out);
  return out != nullptr ? out : Undefined(env);
}

napi_value CredentialsGetuid(napi_env env, napi_callback_info /*info*/) {
#if defined(_WIN32)
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#else
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(::getuid()), &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsGeteuid(napi_env env, napi_callback_info /*info*/) {
#if defined(_WIN32)
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#else
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(::geteuid()), &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsGetgid(napi_env env, napi_callback_info /*info*/) {
#if defined(_WIN32)
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#else
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(::getgid()), &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsGetegid(napi_env env, napi_callback_info /*info*/) {
#if defined(_WIN32)
  napi_value out = nullptr;
  napi_create_uint32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#else
  napi_value out = nullptr;
  napi_create_uint32(env, static_cast<uint32_t>(::getegid()), &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsGetgroups(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_create_array_with_length(env, 0, &out);
  if (out == nullptr) return Undefined(env);

#if !defined(_WIN32)
  const int count = ::getgroups(0, nullptr);
  if (count > 0) {
    std::vector<gid_t> groups(static_cast<size_t>(count));
    if (::getgroups(count, groups.data()) >= 0) {
      for (size_t i = 0; i < groups.size(); ++i) {
        napi_value value = nullptr;
        if (napi_create_uint32(env, static_cast<uint32_t>(groups[i]), &value) == napi_ok && value != nullptr) {
          napi_set_element(env, out, static_cast<uint32_t>(i), value);
        }
      }
    }
  }
#endif
  return out;
}

napi_value CredentialsSetuid(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "setuid");
  return nullptr;
#else
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  uid_t uid = 0;
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"uid\" argument must be a number or string.");
    return nullptr;
  }
  const IdResolveStatus status = ResolveUidValue(env, argv[0], &uid);
  if (status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"uid\" argument must be a number or string.");
    return nullptr;
  }
  napi_value out = nullptr;
  if (status == IdResolveStatus::kUnknown) {
    napi_create_int32(env, 1, &out);
    return out != nullptr ? out : Undefined(env);
  }
  if (::setuid(uid) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsSeteuid(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "seteuid");
  return nullptr;
#else
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  uid_t uid = 0;
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"uid\" argument must be a number or string.");
    return nullptr;
  }
  const IdResolveStatus status = ResolveUidValue(env, argv[0], &uid);
  if (status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"uid\" argument must be a number or string.");
    return nullptr;
  }
  napi_value out = nullptr;
  if (status == IdResolveStatus::kUnknown) {
    napi_create_int32(env, 1, &out);
    return out != nullptr ? out : Undefined(env);
  }
  if (::seteuid(uid) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsSetgid(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "setgid");
  return nullptr;
#else
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  gid_t gid = 0;
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"gid\" argument must be a number or string.");
    return nullptr;
  }
  const IdResolveStatus status = ResolveGidValue(env, argv[0], &gid);
  if (status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"gid\" argument must be a number or string.");
    return nullptr;
  }
  napi_value out = nullptr;
  if (status == IdResolveStatus::kUnknown) {
    napi_create_int32(env, 1, &out);
    return out != nullptr ? out : Undefined(env);
  }
  if (::setgid(gid) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsSetegid(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "setegid");
  return nullptr;
#else
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  gid_t gid = 0;
  if (argc < 1 || argv[0] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"gid\" argument must be a number or string.");
    return nullptr;
  }
  const IdResolveStatus status = ResolveGidValue(env, argv[0], &gid);
  if (status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"gid\" argument must be a number or string.");
    return nullptr;
  }
  napi_value out = nullptr;
  if (status == IdResolveStatus::kUnknown) {
    napi_create_int32(env, 1, &out);
    return out != nullptr ? out : Undefined(env);
  }
  if (::setegid(gid) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsSetgroups(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "setgroups");
  return nullptr;
#else
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"groups\" argument must be an array.");
    return nullptr;
  }

  bool is_array = false;
  if (napi_is_array(env, argv[0], &is_array) != napi_ok || !is_array) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"groups\" argument must be an array.");
    return nullptr;
  }

  uint32_t len = 0;
  napi_get_array_length(env, argv[0], &len);
  std::vector<gid_t> groups(static_cast<size_t>(len));
  for (uint32_t i = 0; i < len; ++i) {
    napi_value element = nullptr;
    if (napi_get_element(env, argv[0], i, &element) != napi_ok || element == nullptr) {
      napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "Each group id must be a number or string.");
      return nullptr;
    }
    gid_t gid = 0;
    const IdResolveStatus status = ResolveGidValue(env, element, &gid);
    if (status == IdResolveStatus::kInvalidType) {
      napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "Each group id must be a number or string.");
      return nullptr;
    }
    if (status == IdResolveStatus::kUnknown) {
      napi_value out = nullptr;
      napi_create_int32(env, static_cast<int32_t>(i + 1), &out);
      return out != nullptr ? out : Undefined(env);
    }
    groups[i] = gid;
  }

  if (::setgroups(groups.size(), groups.empty() ? nullptr : groups.data()) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

napi_value CredentialsInitgroups(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  ThrowFeatureUnavailable(env, "initgroups");
  return nullptr;
#else
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"user\" and \"extraGroup\" arguments are required.");
    return nullptr;
  }

  gid_t extra_group = 0;
  const IdResolveStatus group_status = ResolveGidValue(env, argv[1], &extra_group);
  if (group_status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"extraGroup\" argument must be a number or string.");
    return nullptr;
  }
  if (group_status == IdResolveStatus::kUnknown) {
    napi_value out = nullptr;
    napi_create_int32(env, 2, &out);
    return out != nullptr ? out : Undefined(env);
  }

  std::string user;
  const IdResolveStatus user_status = ResolveUserNameForInitgroups(env, argv[0], &user);
  if (user_status == IdResolveStatus::kInvalidType) {
    napi_throw_type_error(env, "ERR_INVALID_ARG_TYPE", "The \"user\" argument must be a number or string.");
    return nullptr;
  }
  if (user_status == IdResolveStatus::kUnknown) {
    napi_value out = nullptr;
    napi_create_int32(env, 1, &out);
    return out != nullptr ? out : Undefined(env);
  }

  if (::initgroups(user.c_str(), extra_group) != 0) {
    ThrowErrnoSystemError(env, errno);
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_int32(env, 0, &out);
  return out != nullptr ? out : Undefined(env);
#endif
}

void DefineMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

}  // namespace

napi_value InitCredentials(napi_env env) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return Undefined(env);

  DefineMethod(env, out, "safeGetenv", CredentialsSafeGetenv);
  DefineMethod(env, out, "getTempDir", CredentialsGetTempDir);
  DefineMethod(env, out, "getuid", CredentialsGetuid);
  DefineMethod(env, out, "geteuid", CredentialsGeteuid);
  DefineMethod(env, out, "getgid", CredentialsGetgid);
  DefineMethod(env, out, "getegid", CredentialsGetegid);
  DefineMethod(env, out, "getgroups", CredentialsGetgroups);
  DefineMethod(env, out, "setuid", CredentialsSetuid);
  DefineMethod(env, out, "seteuid", CredentialsSeteuid);
  DefineMethod(env, out, "setgid", CredentialsSetgid);
  DefineMethod(env, out, "setegid", CredentialsSetegid);
  DefineMethod(env, out, "setgroups", CredentialsSetgroups);
  DefineMethod(env, out, "initgroups", CredentialsInitgroups);

#if defined(_WIN32)
  SetBool(env, out, "implementsPosixCredentials", false);
#else
  SetBool(env, out, "implementsPosixCredentials", true);
#endif

  return out;
}

}  // namespace internal_binding
