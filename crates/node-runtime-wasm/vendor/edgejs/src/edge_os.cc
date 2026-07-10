#include "edge_os.h"

#include <array>
#include <cerrno>
#include <climits>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

#if !defined(_WIN32)
#include <arpa/inet.h>
#endif

#if !defined(_WIN32)
#include <dlfcn.h>
#include <signal.h>
#endif

#include "uv.h"

#if defined(__wasi__) || defined(__wasm32__)
#define DUMMY_UV_STUBS 1
#endif

namespace {
bool IsFunctionValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

void ClearPendingExceptionIfAny(napi_env env) {
  bool pending = false;
  if (napi_is_exception_pending(env, &pending) != napi_ok || !pending) return;
  napi_value ignored = nullptr;
  napi_get_and_clear_last_exception(env, &ignored);
}

int NormalizePriorityPid(int32_t pid) {
  // In Node.js APIs, pid=0 means current process.
  if (pid == 0) return static_cast<int>(uv_os_getpid());
  return static_cast<int>(pid);
}

bool IsBigEndian() {
  const uint16_t value = 0x0102;
  return reinterpret_cast<const uint8_t*>(&value)[0] == 0x01;
}

void SetNamedString(napi_env env, napi_value obj, const char* key, const char* value) {
  napi_value str = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &str) == napi_ok && str != nullptr) {
    napi_set_named_property(env, obj, key, str);
  }
}

void SetNamedInt32(napi_env env, napi_value obj, const char* key, int32_t value) {
  napi_value val = nullptr;
  if (napi_create_int32(env, value, &val) == napi_ok && val != nullptr) {
    napi_set_named_property(env, obj, key, val);
  }
}

void SetNamedUInt32(napi_env env, napi_value obj, const char* key, uint32_t value) {
  napi_value val = nullptr;
  if (napi_create_uint32(env, value, &val) == napi_ok && val != nullptr) {
    napi_set_named_property(env, obj, key, val);
  }
}

void SetNamedBool(napi_env env, napi_value obj, const char* key, bool value) {
  napi_value val = nullptr;
  if (napi_get_boolean(env, value, &val) == napi_ok && val != nullptr) {
    napi_set_named_property(env, obj, key, val);
  }
}

void SetNamedNull(napi_env env, napi_value obj, const char* key) {
  napi_value val = nullptr;
  if (napi_get_null(env, &val) == napi_ok && val != nullptr) {
    napi_set_named_property(env, obj, key, val);
  }
}

void SetElementString(napi_env env, napi_value array, uint32_t index, const char* value) {
  napi_value str = nullptr;
  if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &str) == napi_ok && str != nullptr) {
    napi_set_element(env, array, index, str);
  }
}

void SetElementDouble(napi_env env, napi_value array, uint32_t index, double value) {
  napi_value num = nullptr;
  if (napi_create_double(env, value, &num) == napi_ok && num != nullptr) {
    napi_set_element(env, array, index, num);
  }
}

void SetElementBool(napi_env env, napi_value array, uint32_t index, bool value) {
  napi_value b = nullptr;
  if (napi_get_boolean(env, value, &b) == napi_ok && b != nullptr) {
    napi_set_element(env, array, index, b);
  }
}

void SetContextError(napi_env env, napi_value ctx, const char* syscall, int err) {
  if (ctx == nullptr) return;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, ctx, &type) != napi_ok || type != napi_object) return;
  SetNamedString(env, ctx, "syscall", syscall);
  SetNamedString(env, ctx, "code", uv_err_name(err));
  SetNamedString(env, ctx, "message", uv_strerror(err));
  SetNamedInt32(env, ctx, "errno", err);
}

napi_value GetOptionalContextArg(napi_env env, napi_value arg) {
  if (arg == nullptr) return nullptr;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, arg, &type) != napi_ok || type != napi_object) return nullptr;
  return arg;
}

napi_value GetGlobal(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;
  return global;
}

napi_value CreateNullPrototypeObject(napi_env env) {
  napi_value global = GetGlobal(env);
  if (global == nullptr) return nullptr;

  napi_value object_ctor = nullptr;
  if (napi_get_named_property(env, global, "Object", &object_ctor) != napi_ok ||
      object_ctor == nullptr || !IsFunctionValue(env, object_ctor)) {
    return nullptr;
  }

  napi_value create_fn = nullptr;
  if (napi_get_named_property(env, object_ctor, "create", &create_fn) != napi_ok ||
      create_fn == nullptr || !IsFunctionValue(env, create_fn)) {
    return nullptr;
  }

  napi_value null_value = nullptr;
  if (napi_get_null(env, &null_value) != napi_ok || null_value == nullptr) return nullptr;

  napi_value out = nullptr;
  napi_value argv[1] = {null_value};
  if (napi_call_function(env, object_ctor, create_fn, 1, argv, &out) != napi_ok || out == nullptr) {
    ClearPendingExceptionIfAny(env);
    return nullptr;
  }
  return out;
}

napi_value CreateBestEffortNullPrototypeObject(napi_env env) {
  napi_value out = CreateNullPrototypeObject(env);
  if (out != nullptr) return out;
  if (napi_create_object(env, &out) != napi_ok) return nullptr;
  return out;
}

bool IsStringEqualTo(napi_env env, napi_value value, const char* expected) {
  if (value == nullptr || expected == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) return false;

  size_t value_len = 0;
  if (napi_get_value_string_utf8(env, value, nullptr, 0, &value_len) != napi_ok) return false;
  std::string str;
  str.resize(value_len + 1);
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, value, str.data(), value_len + 1, &copied) != napi_ok) return false;
  str.resize(copied);
  return str == expected;
}

bool ParseUserInfoBufferEncoding(napi_env env, napi_value options, bool* use_buffer) {
  if (use_buffer == nullptr) return false;
  *use_buffer = false;
  if (options == nullptr) return true;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, options, &type) != napi_ok || type != napi_object) return true;

  bool has_encoding = false;
  if (napi_has_named_property(env, options, "encoding", &has_encoding) != napi_ok) return false;
  if (!has_encoding) return true;

  napi_value encoding = nullptr;
  if (napi_get_named_property(env, options, "encoding", &encoding) != napi_ok) return false;
  *use_buffer = IsStringEqualTo(env, encoding, "buffer");
  return true;
}

napi_value BindingGetAvailableParallelism(napi_env env, napi_callback_info info) {
  (void)info;
  const uint32_t value = uv_available_parallelism();
  napi_value out = nullptr;
  if (napi_create_uint32(env, value, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetFreeMem(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value out = nullptr;

#ifdef DUMMY_UV_STUBS
  uint64_t free_memory = 4 * 1024 * 1024;
#else
  uint64_t free_memory = uv_get_free_memory();
#endif

  if (napi_create_double(env, static_cast<double>(free_memory), &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetTotalMem(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value out = nullptr;
#ifdef DUMMY_UV_STUBS
  uint64_t total_memory = 4 * 1024 * 1024;
#else
  uint64_t total_memory = uv_get_total_memory();
#endif

  if (napi_create_double(env, static_cast<double>(total_memory), &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetLoadAvg(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) {
    return nullptr;
  }
  std::array<double, 3> avg{0.0, 0.0, 0.0};
#ifndef DUMMY_UV_STUBS
  uv_loadavg(avg.data());
#endif
  for (uint32_t i = 0; i < 3; i++) {
    napi_value v = nullptr;
    if (napi_create_double(env, avg[i], &v) != napi_ok || v == nullptr) continue;
    napi_set_element(env, argv[0], i, v);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value BindingGetUptime(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value ctx = argc > 0 ? GetOptionalContextArg(env, argv[0]) : nullptr;
  double uptime = 0.0;

#ifdef DUMMY_UV_STUBS
  static const uint64_t kStartHr = uv_hrtime();
  const uint64_t now = uv_hrtime();
  uptime = static_cast<double>(now - kStartHr) / 1e9;
  if (uptime <= 0.0) uptime = 1e-6;
#else
  const int err = uv_uptime(&uptime);
  if (err != 0) {
    SetContextError(env, ctx, "uv_uptime", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
#endif

  napi_value out = nullptr;
  if (napi_create_double(env, uptime, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetHostname(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value ctx = argc > 0 ? GetOptionalContextArg(env, argv[0]) : nullptr;
  std::array<char, UV_MAXHOSTNAMESIZE> host{};
  size_t len = host.size();
  const int err = uv_os_gethostname(host.data(), &len);
  if (err != 0) {
    SetContextError(env, ctx, "uv_os_gethostname", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, host.data(), len, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetHomeDirectory(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value ctx = argc > 0 ? GetOptionalContextArg(env, argv[0]) : nullptr;
#if defined(PATH_MAX)
  std::array<char, PATH_MAX> home{};
#else
  std::array<char, 4096> home{};
#endif
  size_t len = home.size();
  const int err = uv_os_homedir(home.data(), &len);
  if (err != 0) {
    SetContextError(env, ctx, "uv_os_homedir", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, home.data(), len, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetOSInformation(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value ctx = argc > 0 ? GetOptionalContextArg(env, argv[0]) : nullptr;
  uv_utsname_t info_out;
  const int err = uv_os_uname(&info_out);
  if (err != 0) {
    SetContextError(env, ctx, "uv_os_uname", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 4, &out) != napi_ok || out == nullptr) return nullptr;
  SetElementString(env, out, 0, info_out.sysname);
  SetElementString(env, out, 1, info_out.version);
  SetElementString(env, out, 2, info_out.release);
  SetElementString(env, out, 3, info_out.machine);
  return out;
}

napi_value BindingGetCPUs(napi_env env, napi_callback_info info) {
  (void)info;
#ifdef DUMMY_UV_STUBS
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 7, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  uint32_t i = 0;
  SetElementString(env, out, i++, "");
  SetElementDouble(env, out, i++, 4.0 * 1000.0 * 1000.0);
  SetElementDouble(env, out, i++, 0.0);
  SetElementDouble(env, out, i++, 0.0);
  SetElementDouble(env, out, i++, 0.0);
  SetElementDouble(env, out, i++, 0.0);
  SetElementDouble(env, out, i++, 0.0);
  return out;
#else
  uv_cpu_info_t* cpu_infos = nullptr;
  int count = 0;
  const int err = uv_cpu_info(&cpu_infos, &count);
  if (err != 0) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, static_cast<size_t>(count) * 7, &out) != napi_ok || out == nullptr) {
    uv_free_cpu_info(cpu_infos, count);
    return nullptr;
  }
  uint32_t i = 0;
  for (int idx = 0; idx < count; idx++) {
    const uv_cpu_info_t& cpu = cpu_infos[idx];
    SetElementString(env, out, i++, cpu.model ? cpu.model : "");
    SetElementDouble(env, out, i++, static_cast<double>(cpu.speed));
    SetElementDouble(env, out, i++, static_cast<double>(cpu.cpu_times.user));
    SetElementDouble(env, out, i++, static_cast<double>(cpu.cpu_times.nice));
    SetElementDouble(env, out, i++, static_cast<double>(cpu.cpu_times.sys));
    SetElementDouble(env, out, i++, static_cast<double>(cpu.cpu_times.idle));
    SetElementDouble(env, out, i++, static_cast<double>(cpu.cpu_times.irq));
  }
  uv_free_cpu_info(cpu_infos, count);
  return out;
#endif
}

napi_value BindingGetInterfaceAddresses(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value ctx = argc > 0 ? GetOptionalContextArg(env, argv[0]) : nullptr;

  uv_interface_address_t* interfaces = nullptr;
  int count = 0;

#ifdef DUMMY_UV_STUBS
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 7, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  uint32_t i = 0;
  SetElementString(env, out, i++, "lo");
  SetElementString(env, out, i++, "127.0.0.1");
  SetElementString(env, out, i++, "255.0.0.0");
  SetElementString(env, out, i++, "IPv4");
  SetElementString(env, out, i++, "00:00:00:00:00:00");
  SetElementBool(env, out, i++, true);
  napi_value scope = nullptr;
  if (napi_create_int32(env, -1, &scope) == napi_ok && scope != nullptr) {
    napi_set_element(env, out, i++, scope);
  } else {
    i++;
  }
#else
  const int err = uv_interface_addresses(&interfaces, &count);
  if (err == UV_ENOSYS) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  if (err != 0) {
    SetContextError(env, ctx, "uv_interface_addresses", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value out = nullptr;
  if (napi_create_array_with_length(env, static_cast<size_t>(count) * 7, &out) != napi_ok || out == nullptr) {
    uv_free_interface_addresses(interfaces, count);
    return nullptr;
  }

  uint32_t i = 0;
  for (int idx = 0; idx < count; idx++) {
    const uv_interface_address_t& iface = interfaces[idx];

    char addr[INET6_ADDRSTRLEN] = {0};
    char netmask[INET6_ADDRSTRLEN] = {0};
    const char* family = "unknown";
    int32_t scope_id = -1;
    if (iface.address.address4.sin_family == AF_INET) {
      family = "IPv4";
      uv_ip4_name(&iface.address.address4, addr, sizeof(addr));
      uv_ip4_name(&iface.netmask.netmask4, netmask, sizeof(netmask));
    } else if (iface.address.address4.sin_family == AF_INET6) {
      family = "IPv6";
      uv_ip6_name(&iface.address.address6, addr, sizeof(addr));
      uv_ip6_name(&iface.netmask.netmask6, netmask, sizeof(netmask));
      scope_id = static_cast<int32_t>(iface.address.address6.sin6_scope_id);
    } else {
      std::snprintf(addr, sizeof(addr), "<unknown sa family>");
    }

    char mac[18] = {0};
    std::snprintf(mac, sizeof(mac), "%02x:%02x:%02x:%02x:%02x:%02x",
                  static_cast<unsigned char>(iface.phys_addr[0]),
                  static_cast<unsigned char>(iface.phys_addr[1]),
                  static_cast<unsigned char>(iface.phys_addr[2]),
                  static_cast<unsigned char>(iface.phys_addr[3]),
                  static_cast<unsigned char>(iface.phys_addr[4]),
                  static_cast<unsigned char>(iface.phys_addr[5]));

    SetElementString(env, out, i++, iface.name ? iface.name : "");
    SetElementString(env, out, i++, addr);
    SetElementString(env, out, i++, netmask);
    SetElementString(env, out, i++, family);
    SetElementString(env, out, i++, mac);
    SetElementBool(env, out, i++, iface.is_internal != 0);
    napi_value scope = nullptr;
    if (napi_create_int32(env, scope_id, &scope) == napi_ok && scope != nullptr) {
      napi_set_element(env, out, i++, scope);
    } else {
      i++;
    }
  }

  uv_free_interface_addresses(interfaces, count);
#endif

  return out;
}

napi_value BindingSetPriority(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3) {
    return nullptr;
  }

  int32_t pid = 0;
  int32_t priority = 0;
  if (napi_get_value_int32(env, argv[0], &pid) != napi_ok) return nullptr;
  if (napi_get_value_int32(env, argv[1], &priority) != napi_ok) return nullptr;
  napi_value ctx = GetOptionalContextArg(env, argv[2]);

  const int effective_pid = NormalizePriorityPid(pid);
  int err = uv_os_setpriority(effective_pid, priority);
  if (err != 0) SetContextError(env, ctx, "uv_os_setpriority", err);
  napi_value out = nullptr;
  if (napi_create_int32(env, err, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetPriority(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 2) {
    return nullptr;
  }

  int32_t pid = 0;
  if (napi_get_value_int32(env, argv[0], &pid) != napi_ok) return nullptr;
  napi_value ctx = GetOptionalContextArg(env, argv[1]);

  const int effective_pid = NormalizePriorityPid(pid);
  int priority = 0;
  const int err = uv_os_getpriority(effective_pid, &priority);
  if (err != 0) {
    SetContextError(env, ctx, "uv_os_getpriority", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_int32(env, priority, &out) != napi_ok) return nullptr;
  return out;
}

napi_value ToStringOrBuffer(napi_env env, const char* value, bool as_buffer) {
  if (as_buffer) {
    const size_t len = value == nullptr ? 0 : std::strlen(value);
    napi_value buffer = nullptr;
    if (napi_create_buffer_copy(env, len, value, nullptr, &buffer) != napi_ok || buffer == nullptr) return nullptr;
    return buffer;
  }
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, value == nullptr ? "" : value, NAPI_AUTO_LENGTH, &out) != napi_ok) return nullptr;
  return out;
}

napi_value BindingGetUserInfo(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  bool as_buffer = false;
  if (argc >= 1 && !ParseUserInfoBufferEncoding(env, argv[0], &as_buffer)) return nullptr;
  napi_value ctx = argc >= 2 ? GetOptionalContextArg(env, argv[1]) : nullptr;

  uv_passwd_t pwd;
  const int err = uv_os_get_passwd(&pwd);
  if (err != 0) {
    SetContextError(env, ctx, "uv_os_get_passwd", err);
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value out = nullptr;
  out = CreateBestEffortNullPrototypeObject(env);
  if (out == nullptr) {
    uv_os_free_passwd(&pwd);
    return nullptr;
  }

#if defined(_WIN32)
  SetNamedInt32(env, out, "uid", -1);
  SetNamedInt32(env, out, "gid", -1);
#else
  SetNamedUInt32(env, out, "uid", static_cast<uint32_t>(pwd.uid));
  SetNamedUInt32(env, out, "gid", static_cast<uint32_t>(pwd.gid));
#endif

  napi_value username = ToStringOrBuffer(env, pwd.username, as_buffer);
  napi_value homedir = ToStringOrBuffer(env, pwd.homedir, as_buffer);
  napi_value shell = nullptr;
  if (pwd.shell == nullptr) {
    napi_get_null(env, &shell);
  } else {
    shell = ToStringOrBuffer(env, pwd.shell, as_buffer);
  }
  if (username != nullptr) napi_set_named_property(env, out, "username", username);
  if (homedir != nullptr) napi_set_named_property(env, out, "homedir", homedir);
  if (shell != nullptr) napi_set_named_property(env, out, "shell", shell);

  uv_os_free_passwd(&pwd);
  return out;
}

void SetMethod(napi_env env, napi_value obj, const char* name, napi_callback cb) {
  napi_value fn = nullptr;
  if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
    napi_set_named_property(env, obj, name, fn);
  }
}

napi_value CreateSignalsObject(napi_env env) {
  napi_value obj = CreateBestEffortNullPrototypeObject(env);
  if (obj == nullptr) return nullptr;
#ifdef SIGHUP
  SetNamedInt32(env, obj, "SIGHUP", SIGHUP);
#endif
#ifdef SIGINT
  SetNamedInt32(env, obj, "SIGINT", SIGINT);
#endif
#ifdef SIGQUIT
  SetNamedInt32(env, obj, "SIGQUIT", SIGQUIT);
#endif
#ifdef SIGILL
  SetNamedInt32(env, obj, "SIGILL", SIGILL);
#endif
#ifdef SIGTRAP
  SetNamedInt32(env, obj, "SIGTRAP", SIGTRAP);
#endif
#ifdef SIGABRT
  SetNamedInt32(env, obj, "SIGABRT", SIGABRT);
#endif
#ifdef SIGIOT
  SetNamedInt32(env, obj, "SIGIOT", SIGIOT);
#endif
#ifdef SIGBUS
  SetNamedInt32(env, obj, "SIGBUS", SIGBUS);
#endif
#ifdef SIGFPE
  SetNamedInt32(env, obj, "SIGFPE", SIGFPE);
#endif
#ifdef SIGKILL
  SetNamedInt32(env, obj, "SIGKILL", SIGKILL);
#endif
#ifdef SIGSEGV
  SetNamedInt32(env, obj, "SIGSEGV", SIGSEGV);
#endif
#ifdef SIGALRM
  SetNamedInt32(env, obj, "SIGALRM", SIGALRM);
#endif
#ifdef SIGTERM
  SetNamedInt32(env, obj, "SIGTERM", SIGTERM);
#endif
#ifdef SIGUSR1
  SetNamedInt32(env, obj, "SIGUSR1", SIGUSR1);
#endif
#ifdef SIGUSR2
  SetNamedInt32(env, obj, "SIGUSR2", SIGUSR2);
#endif
#ifdef SIGBREAK
  SetNamedInt32(env, obj, "SIGBREAK", SIGBREAK);
#endif
#ifdef SIGPIPE
  SetNamedInt32(env, obj, "SIGPIPE", SIGPIPE);
#endif
#ifdef SIGCHLD
  SetNamedInt32(env, obj, "SIGCHLD", SIGCHLD);
#endif
#ifdef SIGSTKFLT
  SetNamedInt32(env, obj, "SIGSTKFLT", SIGSTKFLT);
#endif
#ifdef SIGCONT
  SetNamedInt32(env, obj, "SIGCONT", SIGCONT);
#endif
#ifdef SIGSTOP
  SetNamedInt32(env, obj, "SIGSTOP", SIGSTOP);
#endif
#ifdef SIGTSTP
  SetNamedInt32(env, obj, "SIGTSTP", SIGTSTP);
#endif
#ifdef SIGTTIN
  SetNamedInt32(env, obj, "SIGTTIN", SIGTTIN);
#endif
#ifdef SIGTTOU
  SetNamedInt32(env, obj, "SIGTTOU", SIGTTOU);
#endif
#ifdef SIGURG
  SetNamedInt32(env, obj, "SIGURG", SIGURG);
#endif
#ifdef SIGXCPU
  SetNamedInt32(env, obj, "SIGXCPU", SIGXCPU);
#endif
#ifdef SIGXFSZ
  SetNamedInt32(env, obj, "SIGXFSZ", SIGXFSZ);
#endif
#ifdef SIGVTALRM
  SetNamedInt32(env, obj, "SIGVTALRM", SIGVTALRM);
#endif
#ifdef SIGPROF
  SetNamedInt32(env, obj, "SIGPROF", SIGPROF);
#endif
#ifdef SIGWINCH
  SetNamedInt32(env, obj, "SIGWINCH", SIGWINCH);
#endif
#ifdef SIGIO
  SetNamedInt32(env, obj, "SIGIO", SIGIO);
#endif
#ifdef SIGPOLL
  SetNamedInt32(env, obj, "SIGPOLL", SIGPOLL);
#endif
#ifdef SIGLOST
  SetNamedInt32(env, obj, "SIGLOST", SIGLOST);
#endif
#ifdef SIGPWR
  SetNamedInt32(env, obj, "SIGPWR", SIGPWR);
#endif
#ifdef SIGINFO
  SetNamedInt32(env, obj, "SIGINFO", SIGINFO);
#endif
#ifdef SIGSYS
  SetNamedInt32(env, obj, "SIGSYS", SIGSYS);
#endif
#ifdef SIGUNUSED
  SetNamedInt32(env, obj, "SIGUNUSED", SIGUNUSED);
#endif
  return obj;
}

napi_value CreatePriorityObject(napi_env env) {
  napi_value obj = CreateBestEffortNullPrototypeObject(env);
  if (obj == nullptr) return nullptr;
#ifdef UV_PRIORITY_LOW
  SetNamedInt32(env, obj, "PRIORITY_LOW", UV_PRIORITY_LOW);
#endif
#ifdef UV_PRIORITY_BELOW_NORMAL
  SetNamedInt32(env, obj, "PRIORITY_BELOW_NORMAL", UV_PRIORITY_BELOW_NORMAL);
#endif
#ifdef UV_PRIORITY_NORMAL
  SetNamedInt32(env, obj, "PRIORITY_NORMAL", UV_PRIORITY_NORMAL);
#endif
#ifdef UV_PRIORITY_ABOVE_NORMAL
  SetNamedInt32(env, obj, "PRIORITY_ABOVE_NORMAL", UV_PRIORITY_ABOVE_NORMAL);
#endif
#ifdef UV_PRIORITY_HIGH
  SetNamedInt32(env, obj, "PRIORITY_HIGH", UV_PRIORITY_HIGH);
#endif
#ifdef UV_PRIORITY_HIGHEST
  SetNamedInt32(env, obj, "PRIORITY_HIGHEST", UV_PRIORITY_HIGHEST);
#endif
  return obj;
}

napi_value CreateErrnoObject(napi_env env) {
  napi_value obj = CreateBestEffortNullPrototypeObject(env);
  if (obj == nullptr) return nullptr;
#ifdef E2BIG
  SetNamedInt32(env, obj, "E2BIG", E2BIG);
#endif
#ifdef EACCES
  SetNamedInt32(env, obj, "EACCES", EACCES);
#endif
#ifdef EADDRNOTAVAIL
  SetNamedInt32(env, obj, "EADDRNOTAVAIL", EADDRNOTAVAIL);
#endif
#ifdef EADDRINUSE
  SetNamedInt32(env, obj, "EADDRINUSE", EADDRINUSE);
#endif
#ifdef EAFNOSUPPORT
  SetNamedInt32(env, obj, "EAFNOSUPPORT", EAFNOSUPPORT);
#endif
#ifdef EAGAIN
  SetNamedInt32(env, obj, "EAGAIN", EAGAIN);
#endif
#ifdef EALREADY
  SetNamedInt32(env, obj, "EALREADY", EALREADY);
#endif
#ifdef EBADF
  SetNamedInt32(env, obj, "EBADF", EBADF);
#endif
#ifdef EBADMSG
  SetNamedInt32(env, obj, "EBADMSG", EBADMSG);
#endif
#ifdef EBUSY
  SetNamedInt32(env, obj, "EBUSY", EBUSY);
#endif
#ifdef ECANCELED
  SetNamedInt32(env, obj, "ECANCELED", ECANCELED);
#endif
#ifdef ECHILD
  SetNamedInt32(env, obj, "ECHILD", ECHILD);
#endif
#ifdef ECONNABORTED
  SetNamedInt32(env, obj, "ECONNABORTED", ECONNABORTED);
#endif
#ifdef ECONNREFUSED
  SetNamedInt32(env, obj, "ECONNREFUSED", ECONNREFUSED);
#endif
#ifdef ECONNRESET
  SetNamedInt32(env, obj, "ECONNRESET", ECONNRESET);
#endif
#ifdef EDEADLK
  SetNamedInt32(env, obj, "EDEADLK", EDEADLK);
#endif
#ifdef EDESTADDRREQ
  SetNamedInt32(env, obj, "EDESTADDRREQ", EDESTADDRREQ);
#endif
#ifdef EDOM
  SetNamedInt32(env, obj, "EDOM", EDOM);
#endif
#ifdef EDQUOT
  SetNamedInt32(env, obj, "EDQUOT", EDQUOT);
#endif
#ifdef EEXIST
  SetNamedInt32(env, obj, "EEXIST", EEXIST);
#endif
#ifdef EFAULT
  SetNamedInt32(env, obj, "EFAULT", EFAULT);
#endif
#ifdef EFBIG
  SetNamedInt32(env, obj, "EFBIG", EFBIG);
#endif
#ifdef EHOSTUNREACH
  SetNamedInt32(env, obj, "EHOSTUNREACH", EHOSTUNREACH);
#endif
#ifdef EIDRM
  SetNamedInt32(env, obj, "EIDRM", EIDRM);
#endif
#ifdef EILSEQ
  SetNamedInt32(env, obj, "EILSEQ", EILSEQ);
#endif
#ifdef EINPROGRESS
  SetNamedInt32(env, obj, "EINPROGRESS", EINPROGRESS);
#endif
#ifdef EINTR
  SetNamedInt32(env, obj, "EINTR", EINTR);
#endif
#ifdef EINVAL
  SetNamedInt32(env, obj, "EINVAL", EINVAL);
#endif
#ifdef EIO
  SetNamedInt32(env, obj, "EIO", EIO);
#endif
#ifdef EISCONN
  SetNamedInt32(env, obj, "EISCONN", EISCONN);
#endif
#ifdef EISDIR
  SetNamedInt32(env, obj, "EISDIR", EISDIR);
#endif
#ifdef ELOOP
  SetNamedInt32(env, obj, "ELOOP", ELOOP);
#endif
#ifdef EMFILE
  SetNamedInt32(env, obj, "EMFILE", EMFILE);
#endif
#ifdef EMLINK
  SetNamedInt32(env, obj, "EMLINK", EMLINK);
#endif
#ifdef EMSGSIZE
  SetNamedInt32(env, obj, "EMSGSIZE", EMSGSIZE);
#endif
#ifdef EMULTIHOP
  SetNamedInt32(env, obj, "EMULTIHOP", EMULTIHOP);
#endif
#ifdef ENAMETOOLONG
  SetNamedInt32(env, obj, "ENAMETOOLONG", ENAMETOOLONG);
#endif
#ifdef ENETDOWN
  SetNamedInt32(env, obj, "ENETDOWN", ENETDOWN);
#endif
#ifdef ENETRESET
  SetNamedInt32(env, obj, "ENETRESET", ENETRESET);
#endif
#ifdef ENETUNREACH
  SetNamedInt32(env, obj, "ENETUNREACH", ENETUNREACH);
#endif
#ifdef ENFILE
  SetNamedInt32(env, obj, "ENFILE", ENFILE);
#endif
#ifdef ENOBUFS
  SetNamedInt32(env, obj, "ENOBUFS", ENOBUFS);
#endif
#ifdef ENODATA
  SetNamedInt32(env, obj, "ENODATA", ENODATA);
#endif
#ifdef ENODEV
  SetNamedInt32(env, obj, "ENODEV", ENODEV);
#endif
#ifdef ENOENT
  SetNamedInt32(env, obj, "ENOENT", ENOENT);
#endif
#ifdef ENOEXEC
  SetNamedInt32(env, obj, "ENOEXEC", ENOEXEC);
#endif
#ifdef ENOLCK
  SetNamedInt32(env, obj, "ENOLCK", ENOLCK);
#endif
#ifdef ENOLINK
  SetNamedInt32(env, obj, "ENOLINK", ENOLINK);
#endif
#ifdef ENOMEM
  SetNamedInt32(env, obj, "ENOMEM", ENOMEM);
#endif
#ifdef ENOMSG
  SetNamedInt32(env, obj, "ENOMSG", ENOMSG);
#endif
#ifdef ENOPROTOOPT
  SetNamedInt32(env, obj, "ENOPROTOOPT", ENOPROTOOPT);
#endif
#ifdef ENOSPC
  SetNamedInt32(env, obj, "ENOSPC", ENOSPC);
#endif
#ifdef ENOSR
  SetNamedInt32(env, obj, "ENOSR", ENOSR);
#endif
#ifdef ENOSTR
  SetNamedInt32(env, obj, "ENOSTR", ENOSTR);
#endif
#ifdef ENOSYS
  SetNamedInt32(env, obj, "ENOSYS", ENOSYS);
#endif
#ifdef ENOTCONN
  SetNamedInt32(env, obj, "ENOTCONN", ENOTCONN);
#endif
#ifdef ENOTDIR
  SetNamedInt32(env, obj, "ENOTDIR", ENOTDIR);
#endif
#ifdef ENOTEMPTY
  SetNamedInt32(env, obj, "ENOTEMPTY", ENOTEMPTY);
#endif
#ifdef ENOTSOCK
  SetNamedInt32(env, obj, "ENOTSOCK", ENOTSOCK);
#endif
#ifdef ENOTSUP
  SetNamedInt32(env, obj, "ENOTSUP", ENOTSUP);
#endif
#ifdef ENOTTY
  SetNamedInt32(env, obj, "ENOTTY", ENOTTY);
#endif
#ifdef ENXIO
  SetNamedInt32(env, obj, "ENXIO", ENXIO);
#endif
#ifdef EOPNOTSUPP
  SetNamedInt32(env, obj, "EOPNOTSUPP", EOPNOTSUPP);
#endif
#ifdef EOVERFLOW
  SetNamedInt32(env, obj, "EOVERFLOW", EOVERFLOW);
#endif
#ifdef EPROTONOSUPPORT
  SetNamedInt32(env, obj, "EPROTONOSUPPORT", EPROTONOSUPPORT);
#endif
#ifdef EPERM
  SetNamedInt32(env, obj, "EPERM", EPERM);
#endif
#ifdef EPIPE
  SetNamedInt32(env, obj, "EPIPE", EPIPE);
#endif
#ifdef EPROTO
  SetNamedInt32(env, obj, "EPROTO", EPROTO);
#endif
#ifdef EPROTOTYPE
  SetNamedInt32(env, obj, "EPROTOTYPE", EPROTOTYPE);
#endif
#ifdef ERANGE
  SetNamedInt32(env, obj, "ERANGE", ERANGE);
#endif
#ifdef EROFS
  SetNamedInt32(env, obj, "EROFS", EROFS);
#endif
#ifdef ESPIPE
  SetNamedInt32(env, obj, "ESPIPE", ESPIPE);
#endif
#ifdef ESRCH
  SetNamedInt32(env, obj, "ESRCH", ESRCH);
#endif
#ifdef ESTALE
  SetNamedInt32(env, obj, "ESTALE", ESTALE);
#endif
#ifdef ETIME
  SetNamedInt32(env, obj, "ETIME", ETIME);
#endif
#ifdef ETIMEDOUT
  SetNamedInt32(env, obj, "ETIMEDOUT", ETIMEDOUT);
#endif
#ifdef ETXTBSY
  SetNamedInt32(env, obj, "ETXTBSY", ETXTBSY);
#endif
#ifdef EWOULDBLOCK
  SetNamedInt32(env, obj, "EWOULDBLOCK", EWOULDBLOCK);
#endif
#ifdef EXDEV
  SetNamedInt32(env, obj, "EXDEV", EXDEV);
#endif
#ifdef WSAEINTR
  SetNamedInt32(env, obj, "WSAEINTR", WSAEINTR);
#endif
#ifdef WSAEBADF
  SetNamedInt32(env, obj, "WSAEBADF", WSAEBADF);
#endif
#ifdef WSAEACCES
  SetNamedInt32(env, obj, "WSAEACCES", WSAEACCES);
#endif
#ifdef WSAEFAULT
  SetNamedInt32(env, obj, "WSAEFAULT", WSAEFAULT);
#endif
#ifdef WSAEINVAL
  SetNamedInt32(env, obj, "WSAEINVAL", WSAEINVAL);
#endif
#ifdef WSAEMFILE
  SetNamedInt32(env, obj, "WSAEMFILE", WSAEMFILE);
#endif
#ifdef WSAEWOULDBLOCK
  SetNamedInt32(env, obj, "WSAEWOULDBLOCK", WSAEWOULDBLOCK);
#endif
#ifdef WSAEINPROGRESS
  SetNamedInt32(env, obj, "WSAEINPROGRESS", WSAEINPROGRESS);
#endif
#ifdef WSAEALREADY
  SetNamedInt32(env, obj, "WSAEALREADY", WSAEALREADY);
#endif
#ifdef WSAENOTSOCK
  SetNamedInt32(env, obj, "WSAENOTSOCK", WSAENOTSOCK);
#endif
#ifdef WSAEDESTADDRREQ
  SetNamedInt32(env, obj, "WSAEDESTADDRREQ", WSAEDESTADDRREQ);
#endif
#ifdef WSAEMSGSIZE
  SetNamedInt32(env, obj, "WSAEMSGSIZE", WSAEMSGSIZE);
#endif
#ifdef WSAEPROTOTYPE
  SetNamedInt32(env, obj, "WSAEPROTOTYPE", WSAEPROTOTYPE);
#endif
#ifdef WSAENOPROTOOPT
  SetNamedInt32(env, obj, "WSAENOPROTOOPT", WSAENOPROTOOPT);
#endif
#ifdef WSAEPROTONOSUPPORT
  SetNamedInt32(env, obj, "WSAEPROTONOSUPPORT", WSAEPROTONOSUPPORT);
#endif
#ifdef WSAESOCKTNOSUPPORT
  SetNamedInt32(env, obj, "WSAESOCKTNOSUPPORT", WSAESOCKTNOSUPPORT);
#endif
#ifdef WSAEOPNOTSUPP
  SetNamedInt32(env, obj, "WSAEOPNOTSUPP", WSAEOPNOTSUPP);
#endif
#ifdef WSAEPFNOSUPPORT
  SetNamedInt32(env, obj, "WSAEPFNOSUPPORT", WSAEPFNOSUPPORT);
#endif
#ifdef WSAEAFNOSUPPORT
  SetNamedInt32(env, obj, "WSAEAFNOSUPPORT", WSAEAFNOSUPPORT);
#endif
#ifdef WSAEADDRINUSE
  SetNamedInt32(env, obj, "WSAEADDRINUSE", WSAEADDRINUSE);
#endif
#ifdef WSAEADDRNOTAVAIL
  SetNamedInt32(env, obj, "WSAEADDRNOTAVAIL", WSAEADDRNOTAVAIL);
#endif
#ifdef WSAENETDOWN
  SetNamedInt32(env, obj, "WSAENETDOWN", WSAENETDOWN);
#endif
#ifdef WSAENETUNREACH
  SetNamedInt32(env, obj, "WSAENETUNREACH", WSAENETUNREACH);
#endif
#ifdef WSAENETRESET
  SetNamedInt32(env, obj, "WSAENETRESET", WSAENETRESET);
#endif
#ifdef WSAECONNABORTED
  SetNamedInt32(env, obj, "WSAECONNABORTED", WSAECONNABORTED);
#endif
#ifdef WSAECONNRESET
  SetNamedInt32(env, obj, "WSAECONNRESET", WSAECONNRESET);
#endif
#ifdef WSAENOBUFS
  SetNamedInt32(env, obj, "WSAENOBUFS", WSAENOBUFS);
#endif
#ifdef WSAEISCONN
  SetNamedInt32(env, obj, "WSAEISCONN", WSAEISCONN);
#endif
#ifdef WSAENOTCONN
  SetNamedInt32(env, obj, "WSAENOTCONN", WSAENOTCONN);
#endif
#ifdef WSAESHUTDOWN
  SetNamedInt32(env, obj, "WSAESHUTDOWN", WSAESHUTDOWN);
#endif
#ifdef WSAETOOMANYREFS
  SetNamedInt32(env, obj, "WSAETOOMANYREFS", WSAETOOMANYREFS);
#endif
#ifdef WSAETIMEDOUT
  SetNamedInt32(env, obj, "WSAETIMEDOUT", WSAETIMEDOUT);
#endif
#ifdef WSAECONNREFUSED
  SetNamedInt32(env, obj, "WSAECONNREFUSED", WSAECONNREFUSED);
#endif
#ifdef WSAELOOP
  SetNamedInt32(env, obj, "WSAELOOP", WSAELOOP);
#endif
#ifdef WSAENAMETOOLONG
  SetNamedInt32(env, obj, "WSAENAMETOOLONG", WSAENAMETOOLONG);
#endif
#ifdef WSAEHOSTDOWN
  SetNamedInt32(env, obj, "WSAEHOSTDOWN", WSAEHOSTDOWN);
#endif
#ifdef WSAEHOSTUNREACH
  SetNamedInt32(env, obj, "WSAEHOSTUNREACH", WSAEHOSTUNREACH);
#endif
#ifdef WSAENOTEMPTY
  SetNamedInt32(env, obj, "WSAENOTEMPTY", WSAENOTEMPTY);
#endif
#ifdef WSAEPROCLIM
  SetNamedInt32(env, obj, "WSAEPROCLIM", WSAEPROCLIM);
#endif
#ifdef WSAEUSERS
  SetNamedInt32(env, obj, "WSAEUSERS", WSAEUSERS);
#endif
#ifdef WSAEDQUOT
  SetNamedInt32(env, obj, "WSAEDQUOT", WSAEDQUOT);
#endif
#ifdef WSAESTALE
  SetNamedInt32(env, obj, "WSAESTALE", WSAESTALE);
#endif
#ifdef WSAEREMOTE
  SetNamedInt32(env, obj, "WSAEREMOTE", WSAEREMOTE);
#endif
#ifdef WSASYSNOTREADY
  SetNamedInt32(env, obj, "WSASYSNOTREADY", WSASYSNOTREADY);
#endif
#ifdef WSAVERNOTSUPPORTED
  SetNamedInt32(env, obj, "WSAVERNOTSUPPORTED", WSAVERNOTSUPPORTED);
#endif
#ifdef WSANOTINITIALISED
  SetNamedInt32(env, obj, "WSANOTINITIALISED", WSANOTINITIALISED);
#endif
#ifdef WSAEDISCON
  SetNamedInt32(env, obj, "WSAEDISCON", WSAEDISCON);
#endif
#ifdef WSAENOMORE
  SetNamedInt32(env, obj, "WSAENOMORE", WSAENOMORE);
#endif
#ifdef WSAECANCELLED
  SetNamedInt32(env, obj, "WSAECANCELLED", WSAECANCELLED);
#endif
#ifdef WSAEINVALIDPROCTABLE
  SetNamedInt32(env, obj, "WSAEINVALIDPROCTABLE", WSAEINVALIDPROCTABLE);
#endif
#ifdef WSAEINVALIDPROVIDER
  SetNamedInt32(env, obj, "WSAEINVALIDPROVIDER", WSAEINVALIDPROVIDER);
#endif
#ifdef WSAEPROVIDERFAILEDINIT
  SetNamedInt32(env, obj, "WSAEPROVIDERFAILEDINIT", WSAEPROVIDERFAILEDINIT);
#endif
#ifdef WSASYSCALLFAILURE
  SetNamedInt32(env, obj, "WSASYSCALLFAILURE", WSASYSCALLFAILURE);
#endif
#ifdef WSASERVICE_NOT_FOUND
  SetNamedInt32(env, obj, "WSASERVICE_NOT_FOUND", WSASERVICE_NOT_FOUND);
#endif
#ifdef WSATYPE_NOT_FOUND
  SetNamedInt32(env, obj, "WSATYPE_NOT_FOUND", WSATYPE_NOT_FOUND);
#endif
#ifdef WSA_E_NO_MORE
  SetNamedInt32(env, obj, "WSA_E_NO_MORE", WSA_E_NO_MORE);
#endif
#ifdef WSA_E_CANCELLED
  SetNamedInt32(env, obj, "WSA_E_CANCELLED", WSA_E_CANCELLED);
#endif
#ifdef WSAEREFUSED
  SetNamedInt32(env, obj, "WSAEREFUSED", WSAEREFUSED);
#endif
  return obj;
}

napi_value CreateDlopenObject(napi_env env) {
  napi_value obj = CreateBestEffortNullPrototypeObject(env);
  if (obj == nullptr) return nullptr;
#if !defined(_WIN32)
#ifdef RTLD_LAZY
  SetNamedInt32(env, obj, "RTLD_LAZY", RTLD_LAZY);
#endif
#ifdef RTLD_NOW
  SetNamedInt32(env, obj, "RTLD_NOW", RTLD_NOW);
#endif
#ifdef RTLD_GLOBAL
  SetNamedInt32(env, obj, "RTLD_GLOBAL", RTLD_GLOBAL);
#endif
#ifdef RTLD_LOCAL
  SetNamedInt32(env, obj, "RTLD_LOCAL", RTLD_LOCAL);
#endif
#else
  SetNamedInt32(env, obj, "RTLD_LAZY", 1);
  SetNamedInt32(env, obj, "RTLD_NOW", 2);
  SetNamedInt32(env, obj, "RTLD_GLOBAL", 0);
  SetNamedInt32(env, obj, "RTLD_LOCAL", 0);
#endif
  return obj;
}

napi_value CreateOsConstants(napi_env env) {
  napi_value out = CreateBestEffortNullPrototypeObject(env);
  if (out == nullptr) return nullptr;
  napi_value signals = CreateSignalsObject(env);
  napi_value priority = CreatePriorityObject(env);
  napi_value errno_obj = CreateErrnoObject(env);
  napi_value dlopen = CreateDlopenObject(env);
  if (signals != nullptr) napi_set_named_property(env, out, "signals", signals);
  if (priority != nullptr) napi_set_named_property(env, out, "priority", priority);
  if (errno_obj != nullptr) napi_set_named_property(env, out, "errno", errno_obj);
  if (dlopen != nullptr) napi_set_named_property(env, out, "dlopen", dlopen);
  SetNamedInt32(env, out, "UV_UDP_REUSEADDR", UV_UDP_REUSEADDR);
  return out;
}

}  // namespace

napi_value EdgeInstallOsBinding(napi_env env) {
  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) {
    return nullptr;
  }

  SetMethod(env, binding, "getAvailableParallelism", BindingGetAvailableParallelism);
  SetMethod(env, binding, "getCPUs", BindingGetCPUs);
  SetMethod(env, binding, "getFreeMem", BindingGetFreeMem);
  SetMethod(env, binding, "getHomeDirectory", BindingGetHomeDirectory);
  SetMethod(env, binding, "getHostname", BindingGetHostname);
  SetMethod(env, binding, "getInterfaceAddresses", BindingGetInterfaceAddresses);
  SetMethod(env, binding, "getLoadAvg", BindingGetLoadAvg);
  SetMethod(env, binding, "getOSInformation", BindingGetOSInformation);
  SetMethod(env, binding, "getPriority", BindingGetPriority);
  SetMethod(env, binding, "getTotalMem", BindingGetTotalMem);
  SetMethod(env, binding, "getUptime", BindingGetUptime);
  SetMethod(env, binding, "getUserInfo", BindingGetUserInfo);
  SetMethod(env, binding, "setPriority", BindingSetPriority);
  SetNamedBool(env, binding, "isBigEndian", IsBigEndian());

  return binding;
}

napi_value EdgeGetOsConstants(napi_env env) {
  napi_value constants = CreateOsConstants(env);
  if (constants == nullptr) return nullptr;
  napi_value signals = nullptr;
  if (napi_get_named_property(env, constants, "signals", &signals) == napi_ok && signals != nullptr) {
    napi_object_freeze(env, signals);
  }
  return constants;
}
