#include "edge_process.h"
#include "edge_active_resource.h"
#include "binding_registry/binding_registry.h"
#include "edge_env_loop.h"
#include "edge_module_loader.h"
#include "edge_option_helpers.h"
#include "edge_runtime.h"
#include "edge_worker_env.h"
#include "edge_environment.h"

#include <algorithm>
#include <chrono>
#include <cctype>
#include <cerrno>
#include <cstdio>
#include <cstring>
#include <cmath>
#include <ctime>
#include <cstdlib>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>
#include <limits>

#include <uv.h>
#include <openssl/crypto.h>
#include <unicode/uchar.h>
#include <unicode/uvernum.h>

#include "ada/ada.h"
#include "brotli/c/common/version.h"
#include "cares/include/ares_version.h"
#include "llhttp/include/llhttp.h"
#include "nbytes/include/nbytes.h"
#include "ncrypto/ncrypto.h"
#include "simdjson/simdjson.h"
#include "simdutf/simdutf.h"
#include "zlib/zlib.h"
#include "nghttp2/lib/includes/nghttp2/nghttp2ver.h"
#include "zstd/lib/zstd.h"
#include "acorn_version.h"
#include "cjs_module_lexer_version.h"
#include "node_version.h"
#include "edge_version.h"
#include "unofficial_napi.h"
#include "edge_node_addon_compat.h"
#include "edge_timers_host.h"
#include "edge_worker_control.h"

#if defined(_WIN32)
#include <io.h>
#include <stdlib.h>
#include <sys/stat.h>
#define umask _umask
using mode_t = int;
extern char** _environ;
#elif defined(__APPLE__)
#include <dlfcn.h>
#include <fcntl.h>
#include <mach-o/dyld.h>
#include <netdb.h>
#include <signal.h>
#include <sys/stat.h>
#include <unistd.h>
extern char** environ;
#else
#include <dlfcn.h>
#include <fcntl.h>
#include <netdb.h>
#include <signal.h>
#include <sys/stat.h>
#include <unistd.h>
extern char** environ;
#endif

namespace {

constexpr double kMicrosPerSec = 1e6;
constexpr double kNanosPerSec = 1e9;
uint64_t g_process_start_time_ns = uv_hrtime();
std::string g_edge_exec_path;
std::string g_edge_argv0;
std::string g_process_title = "node";
uint32_t g_process_debug_port = 9229;
std::mutex g_process_umask_mutex;
std::mutex g_process_dlopen_mutex;
std::map<std::string, std::unique_ptr<uv_lib_t>> g_process_dlopen_handles;

#if defined(__APPLE__) || defined(__linux__) || defined(__sun) || defined(_AIX)
constexpr int kDefaultDlopenFlags = RTLD_LAZY;
#else
constexpr int kDefaultDlopenFlags = 0;
#endif

#ifndef EDGE_EMBEDDED_V8_VERSION
#define EDGE_EMBEDDED_V8_VERSION "0.0.0-node.0"
#endif

std::string NapiValueToUtf8(napi_env env, napi_value value);
std::vector<std::string> GetStringArrayValue(napi_env env, napi_value value);

#ifndef NI_NUMERICSERV
#define NI_NUMERICSERV 0
#endif

std::string GetGlibcRuntimeVersion() {
#ifndef _WIN32
  const char* (*libc_version)() = nullptr;
  *(reinterpret_cast<void**>(&libc_version)) =
      dlsym(RTLD_DEFAULT, "gnu_get_libc_version");
  if (libc_version != nullptr) {
    return (*libc_version)();
  }
#endif
  return "";
}

std::string GetGlibcCompilerVersion() {
#ifdef __GLIBC__
  std::ostringstream buf;
  buf << __GLIBC__ << "." << __GLIBC_MINOR__;
  return buf.str();
#else
  return "";
#endif
}

napi_addon_register_func GetNapiInitializerCallback(uv_lib_t* lib) {
  if (lib == nullptr) return nullptr;
  void* symbol = nullptr;
  if (uv_dlsym(lib, "napi_register_module_v1", &symbol) != 0 || symbol == nullptr) {
    return nullptr;
  }
  return reinterpret_cast<napi_addon_register_func>(symbol);
}

std::string BuildDlopenCacheKey(const std::string& filename, int32_t flags) {
  return filename + "#" + std::to_string(flags);
}

int OpenDynamicLibrary(const std::string& filename, int32_t flags, uv_lib_t* lib, std::string* error_out) {
  if (lib == nullptr) return UV_EINVAL;
  lib->handle = nullptr;
  lib->errmsg = nullptr;
#if defined(__APPLE__) || defined(__linux__) || defined(__sun) || defined(_AIX)
  lib->handle = dlopen(filename.c_str(), flags);
  if (lib->handle != nullptr) return 0;
  if (error_out != nullptr) {
    const char* error = dlerror();
    *error_out = (error != nullptr && error[0] != '\0') ? error : ("Cannot open shared object file: '" + filename + "'");
  }
  return UV_EINVAL;
#else
  const int rc = uv_dlopen(filename.c_str(), lib);
  if (rc != 0 && error_out != nullptr) {
    const char* error = uv_dlerror(lib);
    *error_out = (error != nullptr && error[0] != '\0') ? error : ("Cannot open shared object file: '" + filename + "'");
  }
  return rc;
#endif
}

void CloseDynamicLibrary(uv_lib_t* lib) {
  if (lib == nullptr) return;
#if defined(__APPLE__) || defined(__linux__) || defined(__sun) || defined(_AIX)
  if (lib->handle != nullptr) {
    (void)dlclose(lib->handle);
    lib->handle = nullptr;
  }
#else
  uv_dlclose(lib);
#endif
}

#ifndef EDGE_STRINGIFY_HELPER
#define EDGE_STRINGIFY_HELPER(x) #x
#endif
#ifndef EDGE_STRINGIFY
#define EDGE_STRINGIFY(x) EDGE_STRINGIFY_HELPER(x)
#endif

void DeleteRefIfPresent(napi_env env, napi_ref* ref);

struct ProcessMethodsBindingState {
  explicit ProcessMethodsBindingState(napi_env env_in) : env(env_in) {}
  ~ProcessMethodsBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
    DeleteRefIfPresent(env, &hrtime_buffer_ref);
    DeleteRefIfPresent(env, &emit_warning_sync_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  napi_ref hrtime_buffer_ref = nullptr;
  napi_ref emit_warning_sync_ref = nullptr;
  bool emit_env_nonstring_warning = true;
};

struct ReportBindingState {
  explicit ReportBindingState(napi_env env_in) : env(env_in) {}
  ~ReportBindingState() {
    DeleteRefIfPresent(env, &binding_ref);
  }

  napi_env env = nullptr;
  napi_ref binding_ref = nullptr;
  bool compact = false;
  bool exclude_network = false;
  bool exclude_env = false;
  bool report_on_fatal_error = false;
  bool report_on_signal = false;
  bool report_on_uncaught_exception = false;
  bool has_intl = false;
  std::string directory;
  std::string filename;
  std::string signal = "SIGUSR2";
  std::vector<std::string> command_line;
  uint64_t max_heap_size_bytes = 0;
  uint64_t sequence = 0;
};
constexpr const char kUvwasiVersion[] = "0.0.23";

std::string ReadTextFileIfExists(const std::filesystem::path& path);

bool TryGetCurrentWorkingDirectoryString(std::string* out, int* uv_error_out = nullptr) {
  if (out == nullptr) return false;
  out->clear();
  if (uv_error_out != nullptr) *uv_error_out = 0;

  size_t cwd_len = 256;
  for (;;) {
    std::string cwd(cwd_len, '\0');
    const int rc = uv_cwd(cwd.data(), &cwd_len);
    if (rc == 0) {
      cwd.resize(cwd_len);
      if (!cwd.empty()) {
        *out = std::move(cwd);
        return true;
      }
      if (uv_error_out != nullptr) *uv_error_out = UV_EIO;
      break;
    }
    if (rc != UV_ENOBUFS) {
      if (uv_error_out != nullptr) *uv_error_out = rc;
      break;
    }
    cwd_len += 1;
  }
  return false;
}

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

void AppendCwdCandidates(const std::filesystem::path& relative_path,
                         std::vector<std::filesystem::path>* out) {
  if (out == nullptr) return;
  const std::optional<std::filesystem::path> cwd = edge_options::TryGetCurrentPath();
  if (!cwd.has_value()) return;
  out->push_back((*cwd / relative_path).lexically_normal());
  out->push_back((cwd->parent_path() / relative_path).lexically_normal());
}

std::string GetOpenSslVersion() {
  // Matches Node behavior: trim the "OpenSSL " prefix and keep the version
  // token, with a conservative fallback for non-OpenSSL implementations.
  const char* version = OpenSSL_version(OPENSSL_VERSION);
  if (version == nullptr) return "0.0.0";
  const char* first_space = std::strchr(version, ' ');
  if (first_space == nullptr || first_space[1] == '\0') return "0.0.0";
  const char* start = first_space + 1;
  const char* end = std::strchr(start, ' ');
  if (end == nullptr) return std::string(start);
  return std::string(start, static_cast<size_t>(end - start));
}

std::string GetBrotliVersion() {
  return std::string(EDGE_STRINGIFY(BROTLI_VERSION_MAJOR)) + "." +
         EDGE_STRINGIFY(BROTLI_VERSION_MINOR) + "." +
         EDGE_STRINGIFY(BROTLI_VERSION_PATCH);
}

std::string GetLlhttpVersion() {
  return std::string(EDGE_STRINGIFY(LLHTTP_VERSION_MAJOR)) + "." +
         EDGE_STRINGIFY(LLHTTP_VERSION_MINOR) + "." +
         EDGE_STRINGIFY(LLHTTP_VERSION_PATCH);
}

std::string TrimAsciiWhitespace(std::string text) {
  size_t start = 0;
  while (start < text.size() && std::isspace(static_cast<unsigned char>(text[start]))) ++start;
  size_t end = text.size();
  while (end > start && std::isspace(static_cast<unsigned char>(text[end - 1]))) --end;
  return text.substr(start, end - start);
}

std::string ExtractQuotedJsonStringField(const std::string& text, const char* key) {
  if (key == nullptr || key[0] == '\0') return {};
  const std::string needle = "\"" + std::string(key) + "\":\"";
  const size_t start = text.find(needle);
  if (start == std::string::npos) return {};
  const size_t value_start = start + needle.size();
  const size_t value_end = text.find('"', value_start);
  if (value_end == std::string::npos || value_end <= value_start) return {};
  return text.substr(value_start, value_end - value_start);
}

std::string ReadTrimmedTextFromCandidates(const std::vector<std::filesystem::path>& candidates) {
  for (const auto& candidate : candidates) {
    const std::string text = TrimAsciiWhitespace(ReadTextFileIfExists(candidate));
    if (!text.empty()) return text;
  }
  return {};
}

std::string ReadTraceVersionFromCandidates(const char* key,
                                           const std::vector<std::filesystem::path>& candidates) {
  for (const auto& candidate : candidates) {
    const std::string text = ReadTextFileIfExists(candidate);
    if (text.empty()) continue;
    const std::string version = ExtractQuotedJsonStringField(text, key);
    if (!version.empty()) return version;
  }
  return {};
}

std::string GetIcuTzVersion() {
  namespace fs = std::filesystem;
  const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  static const std::string version = []() {
    namespace fs = std::filesystem;
    const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
    std::vector<fs::path> text_candidates = {
        source_root / "test" / "fixtures" / "tz-version.txt",
    };
    AppendCwdCandidates(fs::path("test") / "fixtures" / "tz-version.txt", &text_candidates);
    std::string resolved = ReadTrimmedTextFromCandidates(text_candidates);
    if (!resolved.empty()) return resolved;
    std::vector<fs::path> trace_candidates = {
        source_root / "test" / "node_trace.1.log",
    };
    AppendCwdCandidates(fs::path("test") / "node_trace.1.log", &trace_candidates);
    resolved = ReadTraceVersionFromCandidates("tz", trace_candidates);
    if (!resolved.empty()) return resolved;
    return std::string("2025c");
  }();
  (void)source_root;
  return version;
}

std::string GetIcuCldrVersion() {
  static const std::string version = []() {
    namespace fs = std::filesystem;
    const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
    std::vector<fs::path> trace_candidates = {
        source_root / "test" / "node_trace.1.log",
    };
    AppendCwdCandidates(fs::path("test") / "node_trace.1.log", &trace_candidates);
    std::string resolved = ReadTraceVersionFromCandidates("cldr", trace_candidates);
    if (!resolved.empty()) return resolved;
    return std::string("48.0");
  }();
  return version;
}

std::string ExtractPackageVersionFromJson(const std::string& json_text) {
  const std::string key = "\"version\"";
  const size_t key_pos = json_text.find(key);
  if (key_pos == std::string::npos) return {};

  const size_t colon = json_text.find(':', key_pos + key.size());
  if (colon == std::string::npos) return {};

  size_t first_quote = json_text.find('"', colon + 1);
  if (first_quote == std::string::npos) return {};
  ++first_quote;
  const size_t second_quote = json_text.find('"', first_quote);
  if (second_quote == std::string::npos || second_quote <= first_quote) return {};

  return json_text.substr(first_quote, second_quote - first_quote);
}

std::string ReadPackageVersionFromCandidates(const std::vector<std::filesystem::path>& candidates) {
  for (const std::filesystem::path& candidate : candidates) {
    const std::string text = ReadTextFileIfExists(candidate);
    if (text.empty()) continue;
    const std::string version = ExtractPackageVersionFromJson(text);
    if (!version.empty()) return version;
  }
  return "0.0.0";
}

std::string GetUndiciVersion() {
  namespace fs = std::filesystem;
  const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  static const std::string version = [source_root]() {
    std::vector<fs::path> candidates = {
        source_root / "node" / "deps" / "undici" / "src" / "package.json",
    };
    AppendCwdCandidates(fs::path("node") / "deps" / "undici" / "src" / "package.json", &candidates);
    return ReadPackageVersionFromCandidates(candidates);
  }();
  return version;
}

std::string GetAmaroVersion() {
  namespace fs = std::filesystem;
  const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  static const std::string version = [source_root]() {
    std::vector<fs::path> candidates = {
        source_root / "node" / "deps" / "amaro" / "package.json",
    };
    AppendCwdCandidates(fs::path("node") / "deps" / "amaro" / "package.json", &candidates);
    return ReadPackageVersionFromCandidates(candidates);
  }();
  return version;
}

std::string MaybePreferSiblingEdgeBinary(const std::string& detected_exec_path) {
  if (detected_exec_path.empty()) return detected_exec_path;
  namespace fs = std::filesystem;
  std::error_code ec;
  fs::path detected = fs::path(detected_exec_path).lexically_normal();
  const std::string filename = detected.filename().string();
  if (filename.rfind("edge_test_", 0) != 0) {
    return detected_exec_path;
  }
  const std::vector<fs::path> candidates = {
      detected.parent_path() / "edge",
      detected.parent_path() / "edge.exe",
      detected.parent_path().parent_path() / "edge",
      detected.parent_path().parent_path() / "edge.exe",
  };
  for (const fs::path& candidate : candidates) {
    ec.clear();
    if (!fs::exists(candidate, ec) || ec) continue;
    ec.clear();
    if (fs::is_directory(candidate, ec) || ec) continue;
    const fs::path canonical = fs::weakly_canonical(candidate, ec);
    if (!ec) return canonical.string();
    return candidate.string();
  }
  return detected_exec_path;
}

const char* DetectPlatform() {
#if defined(_WIN32)
  return "win32";
#elif defined(__APPLE__)
  return "darwin";
#elif defined(__linux__)
  return "linux";
#elif defined(__wasi__)
  return "wasi";
#elif defined(__sun)
  return "sunos";
#elif defined(_AIX)
  return "aix";
#else
  return "unknown";
#endif
}

const char* DetectArch() {
#if defined(__x86_64__) || defined(_M_X64)
  return "x64";
#elif defined(__aarch64__) || defined(_M_ARM64)
  return "arm64";
#elif defined(__arm__) || defined(_M_ARM)
  return "arm";
#elif defined(__i386__) || defined(_M_IX86)
  return "ia32";
#else
  return "unknown";
#endif
}

std::string DetectExecPath() {
  const char* forced_exec = std::getenv("EDGE_EXEC_PATH");
  if (forced_exec != nullptr && forced_exec[0] != '\0') {
    return forced_exec;
  }
#if defined(__APPLE__)
  uint32_t size = 0;
  _NSGetExecutablePath(nullptr, &size);
  if (size > 0) {
    std::vector<char> buf(size + 1, '\0');
    if (_NSGetExecutablePath(buf.data(), &size) == 0) {
      char resolved[4096] = {'\0'};
      if (realpath(buf.data(), resolved) != nullptr) {
        return MaybePreferSiblingEdgeBinary(std::string(resolved));
      }
      return MaybePreferSiblingEdgeBinary(std::string(buf.data()));
    }
  }
  return "edge";
#elif defined(__linux__)
  std::vector<char> buf(4096, '\0');
  ssize_t n = readlink("/proc/self/exe", buf.data(), buf.size() - 1);
  if (n > 0) {
    buf[static_cast<size_t>(n)] = '\0';
    return MaybePreferSiblingEdgeBinary(std::string(buf.data()));
  }
  return "edge";
#elif defined(_WIN32)
  return "edge.exe";
#elif defined(__wasi__)
  return "/bin/edge";
#else
  return "edge";
#endif
}

std::string ReadTextFileIfExists(const std::filesystem::path& path) {
  std::error_code ec;
  if (!std::filesystem::exists(path, ec) || ec) return {};
  std::ifstream input(path, std::ios::in | std::ios::binary);
  if (!input.is_open()) return {};
  std::ostringstream buffer;
  buffer << input.rdbuf();
  return buffer.str();
}

bool RuntimeHasIntl(napi_env env) {
#if defined(EDGE_HAS_ICU)
  (void)env;
  return true;
#else
  napi_value global = nullptr;
  if (env == nullptr || napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value intl = nullptr;
  if (napi_get_named_property(env, global, "Intl", &intl) != napi_ok || intl == nullptr) return false;

  napi_valuetype type = napi_undefined;
  return napi_typeof(env, intl, &type) == napi_ok && (type == napi_object || type == napi_function);
#endif
}

std::string FindNodeConfigGypiText() {
  namespace fs = std::filesystem;
  std::error_code ec;
  const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  std::vector<fs::path> candidates = {
      source_root / "node" / "config.gypi",
  };
  AppendCwdCandidates(fs::path("node") / "config.gypi", &candidates);
  for (const fs::path& candidate : candidates) {
    if (!candidate.empty()) {
      const std::string text = ReadTextFileIfExists(candidate);
      if (!text.empty()) return text;
    }
  }

  // Final fallback when tests run from <repo>/build* and source path probes fail.
  const fs::path cwd = fs::current_path(ec);
  if (!ec) {
    const fs::path fallback = cwd.parent_path() / "node" / "config.gypi";
    const std::string text = ReadTextFileIfExists(fallback);
    if (!text.empty()) return text;
  }
  return {};
}

bool SetProcessConfigVariableInt(napi_env env, napi_value variables_obj, const char* key, int32_t value) {
  if (variables_obj == nullptr || key == nullptr) return false;
  napi_value js_value = nullptr;
  if (napi_create_int32(env, value, &js_value) != napi_ok || js_value == nullptr) return false;
  return napi_set_named_property(env, variables_obj, key, js_value) == napi_ok;
}

bool EnsureProcessConfigVariablesForEdge(napi_env env, napi_value config_obj) {
  if (config_obj == nullptr) return false;

  napi_value variables_obj = nullptr;
  if (napi_get_named_property(env, config_obj, "variables", &variables_obj) != napi_ok || variables_obj == nullptr) {
    if (napi_create_object(env, &variables_obj) != napi_ok || variables_obj == nullptr) return false;
    if (napi_set_named_property(env, config_obj, "variables", variables_obj) != napi_ok) return false;
  }

  const int32_t has_intl = RuntimeHasIntl(env) ? 1 : 0;
  if (!SetProcessConfigVariableInt(env, variables_obj, "v8_enable_i18n_support", has_intl) ||
      !SetProcessConfigVariableInt(env,
                                   variables_obj,
                                   "node_shared_openssl",
#if defined(EDGE_NODE_SHARED_OPENSSL)
                                   EDGE_NODE_SHARED_OPENSSL) ||
#else
                                   0) ||
#endif
      !SetProcessConfigVariableInt(env,
                                   variables_obj,
                                   "icu_small",
#if defined(EDGE_HAS_SMALL_ICU)
                                   1) ||
#else
                                   0) ||
#endif
      !SetProcessConfigVariableInt(env, variables_obj, "node_use_amaro", 0)) {
    return false;
  }

  napi_value shareable_builtins = nullptr;
  if (napi_get_named_property(env, variables_obj, "node_builtin_shareable_builtins", &shareable_builtins) == napi_ok &&
      shareable_builtins != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, shareable_builtins, &is_array) == napi_ok && is_array) {
      napi_value filtered = nullptr;
      if (napi_create_array(env, &filtered) != napi_ok || filtered == nullptr) return false;
      uint32_t input_length = 0;
      uint32_t output_index = 0;
      if (napi_get_array_length(env, shareable_builtins, &input_length) != napi_ok) return false;
      for (uint32_t i = 0; i < input_length; ++i) {
        napi_value entry = nullptr;
        if (napi_get_element(env, shareable_builtins, i, &entry) != napi_ok || entry == nullptr) continue;
        if (NapiValueToUtf8(env, entry) == "deps/amaro/dist/index.js") continue;
        if (napi_set_element(env, filtered, output_index++, entry) != napi_ok) return false;
      }
      if (napi_set_named_property(env, variables_obj, "node_builtin_shareable_builtins", filtered) != napi_ok) {
        return false;
      }
    }
  }

  return true;
}

napi_value BuildMinimalProcessConfigObject(napi_env env) {
  napi_value config_obj = nullptr;
  if (napi_create_object(env, &config_obj) != napi_ok || config_obj == nullptr) return nullptr;

  napi_value variables_obj = nullptr;
  if (napi_create_object(env, &variables_obj) != napi_ok || variables_obj == nullptr) return nullptr;

  const char* int_var_keys[] = {"v8_enable_i18n_support", "node_quic", "asan"};
  for (const char* key : int_var_keys) {
    if (!SetProcessConfigVariableInt(env, variables_obj, key, 0)) return nullptr;
  }
  if (!SetProcessConfigVariableInt(env,
                                   variables_obj,
                                   "node_shared_openssl",
#if defined(EDGE_NODE_SHARED_OPENSSL)
                                   EDGE_NODE_SHARED_OPENSSL)) {
#else
                                   0)) {
#endif
    return nullptr;
  }

  napi_value shareable_builtins = nullptr;
  if (napi_create_array_with_length(env, 1, &shareable_builtins) != napi_ok ||
      shareable_builtins == nullptr) {
    return nullptr;
  }
  napi_value undici_builtin = nullptr;
  if (napi_create_string_utf8(env, "deps/undici/undici.js", NAPI_AUTO_LENGTH, &undici_builtin) != napi_ok ||
      undici_builtin == nullptr ||
      napi_set_element(env, shareable_builtins, 0, undici_builtin) != napi_ok) {
    return nullptr;
  }
  if (napi_set_named_property(env, variables_obj, "node_builtin_shareable_builtins", shareable_builtins) !=
      napi_ok) {
    return nullptr;
  }

  napi_value zero = nullptr;
  if (napi_create_int32(env, 0, &zero) != napi_ok || zero == nullptr) return nullptr;
  if (napi_set_named_property(env, variables_obj, "node_use_amaro", zero) != napi_ok) {
    return nullptr;
  }

  napi_value napi_build_version = nullptr;
  if (napi_create_string_utf8(
          env, EDGE_STRINGIFY(NODE_API_SUPPORTED_VERSION_MAX), NAPI_AUTO_LENGTH, &napi_build_version) != napi_ok ||
      napi_build_version == nullptr) {
    return nullptr;
  }
  if (napi_set_named_property(env, variables_obj, "napi_build_version", napi_build_version) != napi_ok) {
    return nullptr;
  }

  napi_property_descriptor variables_desc = {};
  variables_desc.utf8name = "variables";
  variables_desc.value = variables_obj;
  variables_desc.attributes = napi_enumerable;
  if (napi_define_properties(env, config_obj, 1, &variables_desc) != napi_ok) return nullptr;

  return config_obj;
}

napi_value ParseProcessConfigObjectFromText(napi_env env, const std::string& config_text) {
  if (config_text.empty()) return nullptr;

  const char* parse_script_source =
      "(function(__raw){"
      "  const body = __raw.split('\\n').slice(1).join('\\n');"
      "  const parsed = JSON.parse(body, function(key, value) {"
      "    if (value === 'true') return true;"
      "    if (value === 'false') return false;"
      "    return value;"
      "  });"
      "  return Object.freeze(parsed);"
      "})";

  napi_value parse_script = nullptr;
  if (napi_create_string_utf8(env, parse_script_source, NAPI_AUTO_LENGTH, &parse_script) != napi_ok ||
      parse_script == nullptr) {
    return nullptr;
  }

  napi_value parse_fn = nullptr;
  if (napi_run_script(env, parse_script, &parse_fn) != napi_ok || parse_fn == nullptr) {
    bool has_exception = false;
    if (napi_is_exception_pending(env, &has_exception) == napi_ok && has_exception) {
      napi_value ignored = nullptr;
      napi_get_and_clear_last_exception(env, &ignored);
    }
    return nullptr;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value raw_text = nullptr;
  if (napi_create_string_utf8(env, config_text.c_str(), config_text.size(), &raw_text) != napi_ok ||
      raw_text == nullptr) {
    return nullptr;
  }

  napi_value parsed = nullptr;
  if (napi_call_function(env, global, parse_fn, 1, &raw_text, &parsed) != napi_ok || parsed == nullptr) {
    bool has_exception = false;
    if (napi_is_exception_pending(env, &has_exception) == napi_ok && has_exception) {
      napi_value ignored = nullptr;
      napi_get_and_clear_last_exception(env, &ignored);
    }
    return nullptr;
  }

  return parsed;
}

napi_value BuildProcessConfigObject(napi_env env) {
  const std::string config_text = FindNodeConfigGypiText();
  napi_value parsed = ParseProcessConfigObjectFromText(env, config_text);
  if (parsed != nullptr) {
    if (!EnsureProcessConfigVariablesForEdge(env, parsed)) return nullptr;
    return parsed;
  }
  napi_value minimal = BuildMinimalProcessConfigObject(env);
  if (minimal == nullptr) return nullptr;
  if (!EnsureProcessConfigVariablesForEdge(env, minimal)) return nullptr;
  return minimal;
}

uint64_t GetHrtimeNanoseconds() {
  return uv_hrtime();
}

void ThrowTypeErrorWithCode(napi_env env, const char* code, const char* message) {
  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      napi_create_type_error(env, code_value, message_value, &error_value) != napi_ok ||
      error_value == nullptr) {
    return;
  }
  napi_set_named_property(env, error_value, "code", code_value);
  napi_throw(env, error_value);
}

void ThrowErrorWithCode(napi_env env, const char* code, const char* message) {
  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      napi_create_error(env, code_value, message_value, &error_value) != napi_ok ||
      error_value == nullptr) {
    return;
  }
  napi_set_named_property(env, error_value, "code", code_value);
  napi_throw(env, error_value);
}

void ThrowRangeErrorWithCode(napi_env env, const char* code, const char* message) {
  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      napi_create_range_error(env, code_value, message_value, &error_value) != napi_ok ||
      error_value == nullptr) {
    return;
  }
  napi_set_named_property(env, error_value, "code", code_value);
  napi_throw(env, error_value);
}

bool SetNamedString(napi_env env, napi_value obj, const char* name, const std::string& value);
bool SetNamedInt32(napi_env env, napi_value obj, const char* name, int32_t value);

bool CreateJsErrorObject(napi_env env, napi_value message_value, napi_value* error_out) {
  if (error_out == nullptr) return false;
  *error_out = nullptr;
  napi_value global = nullptr;
  napi_value error_ctor = nullptr;
  if (napi_get_global(env, &global) == napi_ok && global != nullptr &&
      napi_get_named_property(env, global, "Error", &error_ctor) == napi_ok &&
      error_ctor != nullptr) {
    napi_new_instance(env, error_ctor, 1, &message_value, error_out);
  }
  if (*error_out == nullptr) {
    napi_create_error(env, nullptr, message_value, error_out);
  }
  return *error_out != nullptr;
}

void ThrowSystemError(napi_env env, int err, const char* syscall, const std::string& path = std::string()) {
  int uv_err = err;
  if (uv_err > 0) uv_err = uv_translate_sys_error(uv_err);
  if (uv_err == 0) uv_err = UV_EIO;

  const char* code = uv_err_name(uv_err);
  if (code == nullptr) code = "UNKNOWN";
  const char* detail = uv_strerror(uv_err);
  if (detail == nullptr) detail = "unknown error";

  std::string message = std::string(code) + ": " + detail;
  if (syscall != nullptr && syscall[0] != '\0') {
    message += ", ";
    message += syscall;
  }
  if (!path.empty()) {
    message += " '";
    message += path;
    message += "'";
  }

  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      !CreateJsErrorObject(env, message_value, &error_value) ||
      error_value == nullptr) {
    return;
  }
  SetNamedInt32(env, error_value, "errno", uv_err);
  napi_set_named_property(env, error_value, "code", code_value);
  if (syscall != nullptr && syscall[0] != '\0') {
    SetNamedString(env, error_value, "syscall", syscall);
  }
  if (!path.empty()) {
    SetNamedString(env, error_value, "path", path);
  }
  napi_throw(env, error_value);
}

void ThrowUvCwdError(napi_env env, int err) {
  int uv_err = err;
  if (uv_err > 0) uv_err = uv_translate_sys_error(uv_err);
  if (uv_err == 0) uv_err = UV_EIO;

  const char* code = uv_err_name(uv_err);
  if (code == nullptr) code = "UNKNOWN";
  const char* detail = uv_strerror(uv_err);
  if (detail == nullptr) detail = "unknown error";

  std::string message = std::string(code) + ": process.cwd failed with error " + detail;
  if (uv_err == UV_ENOENT) {
    message += ", the current working directory was likely removed without changing the working directory";
  }
  message += ", uv_cwd";

  napi_value code_value = nullptr;
  napi_value message_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      !CreateJsErrorObject(env, message_value, &error_value) ||
      error_value == nullptr) {
    return;
  }
  SetNamedInt32(env, error_value, "errno", uv_err);
  napi_set_named_property(env, error_value, "code", code_value);
  SetNamedString(env, error_value, "syscall", "uv_cwd");
  napi_throw(env, error_value);
}

std::string GetCurrentWorkingDirectoryForErrors() {
  std::string cwd;
  return TryGetCurrentWorkingDirectoryString(&cwd) ? cwd : ".";
}

std::string GetProcessTitleString() {
  std::string title(16, '\0');
  for (;;) {
    const int rc = uv_get_process_title(title.data(), title.size());
    if (rc == 0) {
      title.resize(std::strlen(title.c_str()));
      return title;
    }
    if (rc != UV_ENOBUFS || title.size() >= 1024 * 1024) {
      return g_process_title.empty() ? std::string("node") : g_process_title;
    }
    title.resize(title.size() * 2);
  }
}

bool GetFloat64ArrayData(napi_env env,
                         napi_value value,
                         size_t min_length,
                         double** data_out,
                         size_t* length_out = nullptr) {
  if (data_out == nullptr || value == nullptr) return false;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) != napi_ok || !is_typedarray) return false;
  napi_typedarray_type ta_type = napi_int8_array;
  size_t length = 0;
  void* data = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env, value, &ta_type, &length, &data, &arraybuffer, &byte_offset) != napi_ok ||
      ta_type != napi_float64_array || data == nullptr || length < min_length) {
    return false;
  }
  *data_out = static_cast<double*>(data);
  if (length_out != nullptr) *length_out = length;
  return true;
}

bool SetOwnPropertyValue(napi_env env, napi_value obj, const char* name, napi_value value) {
  if (obj == nullptr || value == nullptr) return false;
  return napi_set_named_property(env, obj, name, value) == napi_ok;
}

std::string InvalidArgTypeSuffix(napi_env env, napi_value value) {
  napi_valuetype t = napi_undefined;
  if (value == nullptr || napi_typeof(env, value, &t) != napi_ok) return " Received undefined";
  if (t == napi_undefined) return " Received undefined";
  if (t == napi_null) return " Received null";
  if (t == napi_string) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) return " Received type string";
    std::vector<char> buf(len + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, buf.data(), buf.size(), &copied) != napi_ok) {
      return " Received type string";
    }
    return " Received type string ('" + std::string(buf.data(), copied) + "')";
  }
  if (t == napi_object) return " Received an instance of Object";
  if (t == napi_number) {
    double d = 0;
    if (napi_get_value_double(env, value, &d) == napi_ok) {
      std::ostringstream oss;
      oss << " Received type number (" << d << ")";
      return oss.str();
    }
  }
  return " Received type " + std::string(t == napi_boolean ? "boolean" : "unknown");
}

std::string FormatNodeNumber(double value) {
  if (std::isinf(value)) return value > 0 ? "Infinity" : "-Infinity";
  std::ostringstream oss;
  oss << value;
  return oss.str();
}

std::string NapiValueToUtf8(napi_env env, napi_value value) {
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) {
    bool has_pending_exception = false;
    if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
      napi_value ignored = nullptr;
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }

    napi_value global = nullptr;
    napi_value string_ctor = nullptr;
    if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
        napi_get_named_property(env, global, "String", &string_ctor) != napi_ok ||
        string_ctor == nullptr) {
      return "";
    }

    napi_value argv[1] = {value};
    if (napi_call_function(env, global, string_ctor, 1, argv, &string_value) != napi_ok ||
        string_value == nullptr) {
      if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
        napi_value ignored = nullptr;
        (void)napi_get_and_clear_last_exception(env, &ignored);
      }
      return "";
    }
  }
  size_t length = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &length) != napi_ok) return "";
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) return "";
  out.resize(copied);
  return out;
}

ProcessMethodsBindingState* GetProcessMethodsState(napi_env env) {
  return EdgeEnvironmentGetSlotData<ProcessMethodsBindingState>(
      env, kEdgeEnvironmentSlotProcessMethodsBindingState);
}

ProcessMethodsBindingState& EnsureProcessMethodsState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<ProcessMethodsBindingState>(
      env, kEdgeEnvironmentSlotProcessMethodsBindingState);
}

bool ProcessEnvValueNeedsDeprecationWarning(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  return type != napi_string && type != napi_number && type != napi_boolean;
}

void EmitProcessMethodsWarningSync(napi_env env,
                                   const char* message,
                                   const char* type,
                                   const char* code) {
  if (env == nullptr || message == nullptr || type == nullptr || code == nullptr) return;
  ProcessMethodsBindingState* state = GetProcessMethodsState(env);
  if (state == nullptr || state->emit_warning_sync_ref == nullptr) return;

  napi_value emit_warning_sync = nullptr;
  if (napi_get_reference_value(env, state->emit_warning_sync_ref, &emit_warning_sync) != napi_ok ||
      emit_warning_sync == nullptr) {
    return;
  }

  napi_value global = nullptr;
  napi_value message_value = nullptr;
  napi_value type_value = nullptr;
  napi_value code_value = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
      message_value == nullptr ||
      napi_create_string_utf8(env, type, NAPI_AUTO_LENGTH, &type_value) != napi_ok ||
      type_value == nullptr ||
      napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      code_value == nullptr) {
    return;
  }

  napi_value argv[3] = {message_value, type_value, code_value};
  napi_value ignored = nullptr;
  (void)napi_call_function(env, global, emit_warning_sync, 3, argv, &ignored);
}

void MaybeEmitProcessEnvDeprecationWarning(napi_env env, napi_value value) {
  if (!EdgeExecArgvHasFlag("--pending-deprecation") || EdgeExecArgvHasFlag("--no-deprecation")) {
    return;
  }
  ProcessMethodsBindingState* state = GetProcessMethodsState(env);
  if (state == nullptr || !state->emit_env_nonstring_warning ||
      !ProcessEnvValueNeedsDeprecationWarning(env, value)) {
    return;
  }
  state->emit_env_nonstring_warning = false;
  EmitProcessMethodsWarningSync(
      env,
      "Assigning any value other than a string, number, or boolean to a process.env property "
      "is deprecated. Please make sure to convert the value to a string before setting "
      "process.env with it.",
      "DeprecationWarning",
      "DEP0104");
}

ReportBindingState* GetReportState(napi_env env) {
  return EdgeEnvironmentGetSlotData<ReportBindingState>(
      env, kEdgeEnvironmentSlotReportBindingState);
}

ReportBindingState& EnsureReportState(napi_env env) {
  return EdgeEnvironmentGetOrCreateSlotData<ReportBindingState>(
      env, kEdgeEnvironmentSlotReportBindingState);
}

bool SetNamedString(napi_env env, napi_value obj, const char* name, const std::string& value) {
  napi_value v = nullptr;
  if (napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &v) != napi_ok || v == nullptr) return false;
  return napi_set_named_property(env, obj, name, v) == napi_ok;
}

bool SetNamedDouble(napi_env env, napi_value obj, const char* name, double value) {
  napi_value v = nullptr;
  if (napi_create_double(env, value, &v) != napi_ok || v == nullptr) return false;
  return napi_set_named_property(env, obj, name, v) == napi_ok;
}

bool SetNamedInt32(napi_env env, napi_value obj, const char* name, int32_t value) {
  napi_value v = nullptr;
  if (napi_create_int32(env, value, &v) != napi_ok || v == nullptr) return false;
  return napi_set_named_property(env, obj, name, v) == napi_ok;
}

bool SetNamedBool(napi_env env, napi_value obj, const char* name, bool value) {
  napi_value v = nullptr;
  if (napi_get_boolean(env, value, &v) != napi_ok || v == nullptr) return false;
  return napi_set_named_property(env, obj, name, v) == napi_ok;
}

bool SetNamedValue(napi_env env, napi_value obj, const char* name, napi_value value) {
  if (value == nullptr) return false;
  return napi_set_named_property(env, obj, name, value) == napi_ok;
}

bool SetNamedInt64(napi_env env, napi_value obj, const char* name, int64_t value) {
  napi_value v = nullptr;
  if (napi_create_int64(env, value, &v) != napi_ok || v == nullptr) return false;
  return napi_set_named_property(env, obj, name, v) == napi_ok;
}

bool SetFunctionPrototypeUndefined(napi_env env, napi_value fn) {
  if (fn == nullptr) return false;
  napi_value undefined = nullptr;
  if (napi_get_undefined(env, &undefined) != napi_ok || undefined == nullptr) return false;
  return napi_set_named_property(env, fn, "prototype", undefined) == napi_ok;
}

bool CopyNamedProperty(napi_env env, napi_value from, napi_value to, const char* name) {
  bool has = false;
  if (from == nullptr || to == nullptr) return false;
  if (napi_has_named_property(env, from, name, &has) != napi_ok || !has) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, from, name, &value) != napi_ok || value == nullptr) return false;
  return napi_set_named_property(env, to, name, value) == napi_ok;
}

bool IsValidMacAddress(const std::string& mac);

bool ValueToInt32(napi_env env, napi_value value, int32_t* out) {
  if (out == nullptr || value == nullptr) return false;
  return napi_get_value_int32(env, value, out) == napi_ok;
}

bool ValueToBool(napi_env env, napi_value value, bool* out) {
  if (out == nullptr || value == nullptr) return false;
  return napi_get_value_bool(env, value, out) == napi_ok;
}

napi_value RequireBuiltin(napi_env env, const char* id) {
  napi_value global = nullptr;
  napi_value require_fn = EdgeGetRequireFunction(env);
  napi_value id_value = nullptr;
  napi_valuetype require_type = napi_undefined;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      ((require_fn == nullptr ||
        napi_typeof(env, require_fn, &require_type) != napi_ok ||
        require_type != napi_function) &&
       napi_get_named_property(env, global, "require", &require_fn) != napi_ok) ||
      require_fn == nullptr ||
      napi_typeof(env, require_fn, &require_type) != napi_ok ||
      require_type != napi_function ||
      napi_create_string_utf8(env, id, NAPI_AUTO_LENGTH, &id_value) != napi_ok ||
      id_value == nullptr) {
    return nullptr;
  }
  napi_value argv[1] = {id_value};
  napi_value out = nullptr;
  if (napi_call_function(env, global, require_fn, 1, argv, &out) != napi_ok) return nullptr;
  return out;
}

napi_value MakeReportUserLimits(napi_env env) {
  napi_value limits = nullptr;
  if (napi_create_object(env, &limits) != napi_ok || limits == nullptr) return nullptr;
  const char* keys[] = {
      "core_file_size_blocks",
      "data_seg_size_bytes",
      "file_size_blocks",
      "max_locked_memory_bytes",
      "max_memory_size_bytes",
      "open_files",
      "stack_size_bytes",
      "cpu_time_seconds",
      "max_user_processes",
      "virtual_memory_bytes",
  };
  for (const char* key : keys) {
    napi_value entry = nullptr;
    napi_value unlimited = nullptr;
    if (napi_create_object(env, &entry) != napi_ok || entry == nullptr ||
        napi_create_string_utf8(env, "unlimited", NAPI_AUTO_LENGTH, &unlimited) != napi_ok ||
        unlimited == nullptr) {
      return nullptr;
    }
    if (napi_set_named_property(env, entry, "soft", unlimited) != napi_ok ||
        napi_set_named_property(env, entry, "hard", unlimited) != napi_ok ||
        napi_set_named_property(env, limits, key, entry) != napi_ok) {
      return nullptr;
    }
  }
  return limits;
}

std::string PointerToHexString(uint64_t value) {
  std::ostringstream oss;
  oss << "0x" << std::hex << std::nouppercase << value;
  return oss.str();
}

std::vector<std::string> GetStringArrayValue(napi_env env, napi_value value) {
  std::vector<std::string> out;
  bool is_array = false;
  if (value == nullptr || napi_is_array(env, value, &is_array) != napi_ok || !is_array) return out;
  uint32_t length = 0;
  if (napi_get_array_length(env, value, &length) != napi_ok) return out;
  out.reserve(length);
  for (uint32_t i = 0; i < length; ++i) {
    napi_value element = nullptr;
    if (napi_get_element(env, value, i, &element) != napi_ok || element == nullptr) continue;
    out.push_back(NapiValueToUtf8(env, element));
  }
  return out;
}

bool GetNamedPropertyIfPresent(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  bool has = false;
  if (obj == nullptr || napi_has_named_property(env, obj, key, &has) != napi_ok || !has) return false;
  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

bool IsNullOrUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok &&
         (type == napi_null || type == napi_undefined);
}

uint64_t ReadReportHeapLimitFromExecArgv(const std::vector<std::string>& exec_argv) {
  static constexpr const char* kMaxHeapSizePrefix = "--max-heap-size=";
  for (const auto& arg : exec_argv) {
    if (arg.rfind(kMaxHeapSizePrefix, 0) != 0) continue;
    char* end = nullptr;
    errno = 0;
    const unsigned long long mb = std::strtoull(arg.c_str() + std::strlen(kMaxHeapSizePrefix), &end, 10);
    if (errno == 0 && end != nullptr && *end == '\0') {
      return static_cast<uint64_t>(mb) * 1024ull * 1024ull;
    }
  }
  return 0;
}

struct ProcessVersionEntry {
  const char* key;
  std::string value;
};

std::vector<ProcessVersionEntry> BuildProcessVersionEntries(bool has_intl) {
  std::vector<ProcessVersionEntry> version_entries = {
      {"acorn", ACORN_VERSION},
      {"ada", ADA_VERSION},
      {"ares", ARES_VERSION_STR},
      {"brotli", GetBrotliVersion()},
      {"cjs_module_lexer", CJS_MODULE_LEXER_VERSION},
      {"edge", EDGE_VERSION_STRING},
      {"llhttp", GetLlhttpVersion()},
      {"modules", EDGE_STRINGIFY(NODE_MODULE_VERSION)},
      {"napi", EDGE_STRINGIFY(NODE_API_SUPPORTED_VERSION_MAX)},
      {"nbytes", NBYTES_VERSION},
      {"nghttp2", NGHTTP2_VERSION},
      {"simdjson", SIMDJSON_VERSION},
      {"simdutf", SIMDUTF_VERSION},
      {"undici", GetUndiciVersion()},
      {"uv", uv_version_string()},
      {"uvwasi", kUvwasiVersion},
      {"v8", EDGE_EMBEDDED_V8_VERSION},
      {"zlib", ZLIB_VERSION},
      {"zstd", ZSTD_VERSION_STRING},
  };
  const std::string openssl_version = GetOpenSslVersion();
  if (!openssl_version.empty() && openssl_version != "0.0.0") {
    version_entries.push_back({"ncrypto", NCRYPTO_VERSION});
    version_entries.push_back({"openssl", openssl_version});
  }
  if (has_intl) {
    version_entries.push_back({"cldr", GetIcuCldrVersion()});
    version_entries.push_back({"icu", U_ICU_VERSION});
    version_entries.push_back({"tz", GetIcuTzVersion()});
    version_entries.push_back({"unicode", U_UNICODE_VERSION});
  }
  std::sort(version_entries.begin(),
            version_entries.end(),
            [](const ProcessVersionEntry& lhs, const ProcessVersionEntry& rhs) {
              return std::strcmp(lhs.key, rhs.key) < 0;
            });
  return version_entries;
}

std::vector<std::string> BuildCommandLineSnapshot(const std::vector<std::string>& exec_argv,
                                                  const std::vector<std::string>& script_argv,
                                                  const std::string& current_script_path) {
  std::vector<std::string> command_line;
  command_line.reserve(1 + exec_argv.size() + (!current_script_path.empty() ? 1 : 0) + script_argv.size());
  if (!g_edge_argv0.empty()) {
    command_line.push_back(g_edge_argv0);
  } else if (!g_edge_exec_path.empty()) {
    command_line.push_back(g_edge_exec_path);
  } else {
    command_line.push_back("edge");
  }
  command_line.insert(command_line.end(), exec_argv.begin(), exec_argv.end());
  if (!current_script_path.empty()) {
    command_line.push_back(current_script_path);
  }
  command_line.insert(command_line.end(), script_argv.begin(), script_argv.end());
  return command_line;
}

constexpr bool NeedsJsonEscape(std::string_view str) {
  for (const char c : str) {
    if (c == '\\' || c == '"' || c < 0x20) return true;
  }
  return false;
}

std::string EscapeJsonChars(std::string_view str) {
  static constexpr std::string_view control_symbols[0x20] = {
      "\\u0000", "\\u0001", "\\u0002", "\\u0003", "\\u0004", "\\u0005",
      "\\u0006", "\\u0007", "\\b",     "\\t",     "\\n",     "\\u000b",
      "\\f",     "\\r",     "\\u000e", "\\u000f", "\\u0010", "\\u0011",
      "\\u0012", "\\u0013", "\\u0014", "\\u0015", "\\u0016", "\\u0017",
      "\\u0018", "\\u0019", "\\u001a", "\\u001b", "\\u001c", "\\u001d",
      "\\u001e", "\\u001f"};

  std::string ret;
  size_t last_pos = 0;
  size_t pos = 0;
  for (; pos < str.size(); ++pos) {
    std::string replace;
    const char ch = str[pos];
    if (ch == '\\') {
      replace = "\\\\";
    } else if (ch == '"') {
      replace = "\\\"";
    } else {
      const size_t num = static_cast<size_t>(static_cast<unsigned char>(ch));
      if (num < 0x20) replace = control_symbols[num];
    }
    if (!replace.empty()) {
      if (pos > last_pos) ret += str.substr(last_pos, pos - last_pos);
      last_pos = pos + 1;
      ret += replace;
    }
  }
  if (last_pos < str.size()) ret += str.substr(last_pos, pos - last_pos);
  return ret;
}

class SimpleJsonWriter {
 public:
  explicit SimpleJsonWriter(std::ostream& out, bool compact)
      : out_(out), compact_(compact) {}

  struct Null {};

  void json_start() { StartObjectImpl(false); }
  void json_end() { EndObjectImpl(); }
  void json_objectend() { EndObjectImpl(); }
  void json_arrayend() { EndArrayImpl(); }

  template <typename T>
  void json_objectstart(const T& key) {
    StartObjectImpl(true, ToStringView(key));
  }

  template <typename T>
  void json_arraystart(const T& key) {
    StartArrayImpl(true, ToStringView(key));
  }

  template <typename T, typename U>
  void json_keyvalue(const T& key, const U& value) {
    WriteKey(ToStringView(key));
    WriteValue(value);
    state_ = kAfterValue;
  }

  template <typename U>
  void json_element(const U& value) {
    BeginValue();
    WriteValue(value);
    state_ = kAfterValue;
  }

 private:
  static std::string_view ToStringView(std::string_view value) { return value; }
  static std::string_view ToStringView(const std::string& value) { return value; }
  static std::string_view ToStringView(const char* value) { return value != nullptr ? std::string_view(value) : std::string_view(); }

  void Indent() { indent_ += 2; }
  void Deindent() { indent_ -= 2; }

  void Advance() {
    if (compact_) return;
    for (int i = 0; i < indent_; ++i) out_ << ' ';
  }

  void WriteOneSpace() {
    if (!compact_) out_ << ' ';
  }

  void WriteNewLine() {
    if (!compact_) out_ << '\n';
  }

  void BeginValue() {
    if (state_ == kAfterValue) out_ << ',';
    WriteNewLine();
    Advance();
  }

  void WriteKey(std::string_view key) {
    BeginValue();
    WriteString(key);
    out_ << ':';
    WriteOneSpace();
  }

  void StartObjectImpl(bool has_key, std::string_view key = {}) {
    if (has_key) {
      WriteKey(key);
    } else {
      BeginValue();
    }
    out_ << '{';
    Indent();
    state_ = kObjectStart;
  }

  void StartArrayImpl(bool has_key, std::string_view key = {}) {
    if (has_key) {
      WriteKey(key);
    } else {
      BeginValue();
    }
    out_ << '[';
    Indent();
    state_ = kObjectStart;
  }

  void EndObjectImpl() {
    WriteNewLine();
    Deindent();
    Advance();
    out_ << '}';
    if (indent_ == 0) out_ << '\n';
    state_ = kAfterValue;
  }

  void EndArrayImpl() {
    WriteNewLine();
    Deindent();
    Advance();
    out_ << ']';
    state_ = kAfterValue;
  }

  template <typename T,
            typename std::enable_if<std::numeric_limits<T>::is_specialized, bool>::type = true>
  void WriteValue(T number) {
    if constexpr (std::is_same<T, bool>::value) {
      out_ << (number ? "true" : "false");
    } else {
      out_ << number;
    }
  }

  void WriteValue(Null) { out_ << "null"; }
  void WriteValue(std::string_view value) { WriteString(value); }
  void WriteValue(const std::string& value) { WriteString(value); }
  void WriteValue(const char* value) { WriteString(value != nullptr ? std::string_view(value) : std::string_view()); }

  void WriteString(std::string_view value) {
    out_ << '"';
    if (NeedsJsonEscape(value)) {
      out_ << EscapeJsonChars(value);
    } else {
      out_ << value;
    }
    out_ << '"';
  }

  enum JsonState { kObjectStart, kAfterValue };

  std::ostream& out_;
  bool compact_;
  int indent_ = 0;
  int state_ = kObjectStart;
};

bool BuildReportCommandLine(napi_env env, napi_value process_obj, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  napi_value exec_path = nullptr;
  napi_value argv_value = nullptr;
  napi_value exec_argv_value = nullptr;
  (void)GetNamedPropertyIfPresent(env, process_obj, "execPath", &exec_path);
  (void)GetNamedPropertyIfPresent(env, process_obj, "argv", &argv_value);
  (void)GetNamedPropertyIfPresent(env, process_obj, "execArgv", &exec_argv_value);

  const std::vector<std::string> argv = GetStringArrayValue(env, argv_value);
  const std::vector<std::string> exec_argv = GetStringArrayValue(env, exec_argv_value);
  const std::string exec_path_text = exec_path != nullptr ? NapiValueToUtf8(env, exec_path) : std::string();

  std::vector<std::string> command_line;
  if (!argv.empty()) {
    command_line.push_back(argv.front());
  } else if (!exec_path_text.empty()) {
    command_line.push_back(exec_path_text);
  }
  command_line.insert(command_line.end(), exec_argv.begin(), exec_argv.end());
  if (argv.size() > 1) {
    command_line.insert(command_line.end(), argv.begin() + 1, argv.end());
  }

  napi_value array = nullptr;
  if (napi_create_array_with_length(env, command_line.size(), &array) != napi_ok || array == nullptr) return false;
  for (uint32_t i = 0; i < command_line.size(); ++i) {
    napi_value text = nullptr;
    if (napi_create_string_utf8(env, command_line[i].c_str(), NAPI_AUTO_LENGTH, &text) == napi_ok && text != nullptr) {
      napi_set_element(env, array, i, text);
    }
  }
  *out = array;
  return true;
}

bool BuildReportCpus(napi_env env, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  uv_cpu_info_t* cpu_info = nullptr;
  int count = 0;
  if (uv_cpu_info(&cpu_info, &count) != 0 || count < 0) return false;

  napi_value cpus = nullptr;
  if (napi_create_array_with_length(env, static_cast<size_t>(count), &cpus) != napi_ok || cpus == nullptr) {
    uv_free_cpu_info(cpu_info, count);
    return false;
  }

  for (int i = 0; i < count; ++i) {
    napi_value cpu = nullptr;
    if (napi_create_object(env, &cpu) != napi_ok || cpu == nullptr) continue;
    SetNamedString(env, cpu, "model", cpu_info[i].model != nullptr ? cpu_info[i].model : "");
    SetNamedInt64(env, cpu, "speed", cpu_info[i].speed);
    SetNamedInt64(env, cpu, "user", static_cast<int64_t>(cpu_info[i].cpu_times.user));
    SetNamedInt64(env, cpu, "nice", static_cast<int64_t>(cpu_info[i].cpu_times.nice));
    SetNamedInt64(env, cpu, "sys", static_cast<int64_t>(cpu_info[i].cpu_times.sys));
    SetNamedInt64(env, cpu, "idle", static_cast<int64_t>(cpu_info[i].cpu_times.idle));
    SetNamedInt64(env, cpu, "irq", static_cast<int64_t>(cpu_info[i].cpu_times.irq));
    napi_set_element(env, cpus, static_cast<uint32_t>(i), cpu);
  }

  uv_free_cpu_info(cpu_info, count);
  *out = cpus;
  return true;
}

bool BuildReportNetworkInterfaces(napi_env env, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  uv_interface_address_t* interfaces = nullptr;
  int count = 0;
  if (uv_interface_addresses(&interfaces, &count) != 0 || count < 0) return false;

  napi_value array = nullptr;
  if (napi_create_array_with_length(env, static_cast<size_t>(count), &array) != napi_ok || array == nullptr) {
    uv_free_interface_addresses(interfaces, count);
    return false;
  }

  for (int i = 0; i < count; ++i) {
    napi_value entry = nullptr;
    if (napi_create_object(env, &entry) != napi_ok || entry == nullptr) continue;
    SetNamedString(env, entry, "name", interfaces[i].name != nullptr ? interfaces[i].name : "");
    SetNamedBool(env, entry, "internal", interfaces[i].is_internal != 0);

    char mac[18] = {'\0'};
    std::snprintf(mac,
                  sizeof(mac),
                  "%02x:%02x:%02x:%02x:%02x:%02x",
                  static_cast<unsigned char>(interfaces[i].phys_addr[0]),
                  static_cast<unsigned char>(interfaces[i].phys_addr[1]),
                  static_cast<unsigned char>(interfaces[i].phys_addr[2]),
                  static_cast<unsigned char>(interfaces[i].phys_addr[3]),
                  static_cast<unsigned char>(interfaces[i].phys_addr[4]),
                  static_cast<unsigned char>(interfaces[i].phys_addr[5]));
    SetNamedString(env, entry, "mac", mac);

    if (interfaces[i].address.address4.sin_family == AF_INET) {
      char ip[INET_ADDRSTRLEN] = {'\0'};
      char netmask[INET_ADDRSTRLEN] = {'\0'};
      if (uv_ip4_name(&interfaces[i].address.address4, ip, sizeof(ip)) == 0) {
        SetNamedString(env, entry, "address", ip);
      }
      if (uv_ip4_name(&interfaces[i].netmask.netmask4, netmask, sizeof(netmask)) == 0) {
        SetNamedString(env, entry, "netmask", netmask);
      }
      SetNamedString(env, entry, "family", "IPv4");
    } else if (interfaces[i].address.address4.sin_family == AF_INET6) {
      char ip[INET6_ADDRSTRLEN] = {'\0'};
      char netmask[INET6_ADDRSTRLEN] = {'\0'};
      if (uv_ip6_name(&interfaces[i].address.address6, ip, sizeof(ip)) == 0) {
        SetNamedString(env, entry, "address", ip);
      }
      if (uv_ip6_name(&interfaces[i].netmask.netmask6, netmask, sizeof(netmask)) == 0) {
        SetNamedString(env, entry, "netmask", netmask);
      }
      SetNamedString(env, entry, "family", "IPv6");
      SetNamedInt32(env, entry, "scopeid", static_cast<int32_t>(interfaces[i].address.address6.sin6_scope_id));
    } else {
      SetNamedString(env, entry, "family", "unknown");
    }

    napi_set_element(env, array, static_cast<uint32_t>(i), entry);
  }

  uv_free_interface_addresses(interfaces, count);
  *out = array;
  return true;
}

bool SnapshotProcessEnv(napi_env env, napi_value process_obj, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  napi_value env_obj = nullptr;
  if (!GetNamedPropertyIfPresent(env, process_obj, "env", &env_obj) || env_obj == nullptr) return false;

  napi_value snapshot = nullptr;
  if (napi_create_object(env, &snapshot) != napi_ok || snapshot == nullptr) return false;

  napi_value keys = nullptr;
  if (napi_get_property_names(env, env_obj, &keys) != napi_ok || keys == nullptr) return false;
  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    napi_value value = nullptr;
    if (napi_get_property(env, env_obj, key, &value) != napi_ok || value == nullptr) continue;
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, value, &type) != napi_ok || type == napi_undefined) continue;
    napi_value value_string = nullptr;
    if (napi_coerce_to_string(env, value, &value_string) != napi_ok || value_string == nullptr) continue;
    napi_set_property(env, snapshot, key, value_string);
  }

  *out = snapshot;
  return true;
}

bool BuildErrorProperties(napi_env env, napi_value error, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  napi_value props = nullptr;
  if (napi_create_object(env, &props) != napi_ok || props == nullptr) return false;

  napi_valuetype error_type = napi_undefined;
  if (error == nullptr ||
      napi_typeof(env, error, &error_type) != napi_ok ||
      (error_type != napi_object && error_type != napi_function)) {
    *out = props;
    return true;
  }

  napi_value keys = nullptr;
  if (napi_get_property_names(env, error, &keys) != napi_ok || keys == nullptr) {
    *out = props;
    return true;
  }
  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) {
    *out = props;
    return true;
  }
  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;
    const std::string key_text = NapiValueToUtf8(env, key);
    if (key_text == "message" || key_text == "stack") continue;
    napi_value value = nullptr;
    if (napi_get_property(env, error, key, &value) != napi_ok || value == nullptr) continue;
    napi_value value_string = nullptr;
    if (napi_coerce_to_string(env, value, &value_string) != napi_ok || value_string == nullptr) continue;
    napi_set_property(env, props, key, value_string);
  }

  *out = props;
  return true;
}

bool BuildJavascriptStack(napi_env env,
                          napi_value error,
                          const std::string& fallback_message,
                          napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  napi_value js_stack = nullptr;
  napi_value stack_array = nullptr;
  napi_value error_props = nullptr;
  if (napi_create_object(env, &js_stack) != napi_ok || js_stack == nullptr ||
      napi_create_array(env, &stack_array) != napi_ok || stack_array == nullptr ||
      !BuildErrorProperties(env, error, &error_props) || error_props == nullptr) {
    return false;
  }

  std::string message = fallback_message;
  std::string stack_text;
  napi_valuetype error_type = napi_undefined;
  if (error != nullptr &&
      napi_typeof(env, error, &error_type) == napi_ok &&
      (error_type == napi_object || error_type == napi_function)) {
    napi_value message_value = nullptr;
    if (GetNamedPropertyIfPresent(env, error, "message", &message_value)) {
      const std::string maybe_message = NapiValueToUtf8(env, message_value);
      if (!maybe_message.empty()) message = maybe_message;
    }
    napi_value stack_value = nullptr;
    if (GetNamedPropertyIfPresent(env, error, "stack", &stack_value)) {
      stack_text = NapiValueToUtf8(env, stack_value);
      if (!stack_text.empty()) {
        const size_t newline = stack_text.find('\n');
        const std::string first_line = stack_text.substr(0, newline);
        if (!first_line.empty()) {
          message = first_line;
        } else if (message.empty()) {
          message = stack_text;
        }
      }
    }
  } else if (error != nullptr) {
    const std::string primitive_message = NapiValueToUtf8(env, error);
    if (!primitive_message.empty()) message = primitive_message;
  }

  uint32_t index = 0;
  if (!stack_text.empty()) {
    std::istringstream in(stack_text);
    std::string line;
    while (std::getline(in, line)) {
      if (!line.empty() && line.back() == '\r') line.pop_back();
      napi_value line_value = nullptr;
      if (napi_create_string_utf8(env, line.c_str(), NAPI_AUTO_LENGTH, &line_value) == napi_ok && line_value != nullptr) {
        napi_set_element(env, stack_array, index++, line_value);
      }
    }
  }

  SetNamedString(env, js_stack, "message", message);
  SetNamedValue(env, js_stack, "stack", stack_array);
  SetNamedValue(env, js_stack, "errorProperties", error_props);
  *out = js_stack;
  return true;
}

bool BuildJavascriptHeap(napi_env env, napi_value process_obj, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  double heap_total = 0;
  double heap_used = 0;
  double external = 0;
  double array_buffers = 0;
  (void)unofficial_napi_get_process_memory_info(env, &heap_total, &heap_used, &external, &array_buffers);

  napi_value exec_argv_value = nullptr;
  (void)GetNamedPropertyIfPresent(env, process_obj, "execArgv", &exec_argv_value);
  const uint64_t heap_limit = ReadReportHeapLimitFromExecArgv(GetStringArrayValue(env, exec_argv_value));

  napi_value js_heap = nullptr;
  napi_value heap_spaces = nullptr;
  napi_value new_space = nullptr;
  if (napi_create_object(env, &js_heap) != napi_ok || js_heap == nullptr ||
      napi_create_object(env, &heap_spaces) != napi_ok || heap_spaces == nullptr ||
      napi_create_object(env, &new_space) != napi_ok || new_space == nullptr) {
    return false;
  }

  SetNamedInt64(env, js_heap, "totalMemory", static_cast<int64_t>(heap_total));
  SetNamedInt64(env, js_heap, "executableMemory", 0);
  SetNamedInt64(env, js_heap, "totalCommittedMemory", static_cast<int64_t>(heap_total));
  SetNamedInt64(env, js_heap, "availableMemory", 0);
  SetNamedInt64(env, js_heap, "totalGlobalHandlesMemory", 0);
  SetNamedInt64(env, js_heap, "usedGlobalHandlesMemory", 0);
  SetNamedInt64(env, js_heap, "usedMemory", static_cast<int64_t>(heap_used));
  SetNamedInt64(env, js_heap, "memoryLimit", static_cast<int64_t>(heap_limit));
  SetNamedInt64(env, js_heap, "mallocedMemory", 0);
  SetNamedInt64(env, js_heap, "externalMemory", static_cast<int64_t>(external));
  SetNamedInt64(env, js_heap, "peakMallocedMemory", 0);
  SetNamedInt64(env, js_heap, "nativeContextCount", 0);
  SetNamedInt64(env, js_heap, "detachedContextCount", 0);
  SetNamedInt64(env, js_heap, "doesZapGarbage", 0);

  SetNamedInt64(env, new_space, "memorySize", 0);
  SetNamedInt64(env, new_space, "committedMemory", 0);
  SetNamedInt64(env, new_space, "capacity", 0);
  SetNamedInt64(env, new_space, "used", 0);
  SetNamedInt64(env, new_space, "available", 0);
  SetNamedValue(env, heap_spaces, "new_space", new_space);
  SetNamedValue(env, js_heap, "heapSpaces", heap_spaces);

  *out = js_heap;
  return true;
}

bool BuildResourceUsage(napi_env env, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  uv_rusage_t rusage{};
  if (uv_getrusage(&rusage) != 0) return false;

  size_t rss = 0;
  (void)uv_resident_set_memory(&rss);

  napi_value usage = nullptr;
  napi_value page_faults = nullptr;
  napi_value fs_activity = nullptr;
  if (napi_create_object(env, &usage) != napi_ok || usage == nullptr ||
      napi_create_object(env, &page_faults) != napi_ok || page_faults == nullptr ||
      napi_create_object(env, &fs_activity) != napi_ok || fs_activity == nullptr) {
    return false;
  }

  const double user_cpu_seconds = static_cast<double>(rusage.ru_utime.tv_sec) +
                                  static_cast<double>(rusage.ru_utime.tv_usec) / kMicrosPerSec;
  const double kernel_cpu_seconds = static_cast<double>(rusage.ru_stime.tv_sec) +
                                    static_cast<double>(rusage.ru_stime.tv_usec) / kMicrosPerSec;
  SetNamedDouble(env, usage, "userCpuSeconds", user_cpu_seconds);
  SetNamedDouble(env, usage, "kernelCpuSeconds", kernel_cpu_seconds);
  SetNamedDouble(env, usage, "cpuConsumptionPercent", 0.0);
  SetNamedDouble(env, usage, "userCpuConsumptionPercent", 0.0);
  SetNamedDouble(env, usage, "kernelCpuConsumptionPercent", 0.0);
  SetNamedString(env, usage, "maxRss", std::to_string(static_cast<long long>(rusage.ru_maxrss)));
  SetNamedString(env, usage, "rss", std::to_string(static_cast<unsigned long long>(rss)));
  SetNamedString(env, usage, "free_memory", std::to_string(static_cast<unsigned long long>(uv_get_free_memory())));
  SetNamedString(env, usage, "total_memory", std::to_string(static_cast<unsigned long long>(uv_get_total_memory())));
  SetNamedString(env, usage, "available_memory", std::to_string(static_cast<unsigned long long>(uv_get_available_memory())));
  const uint64_t constrained_memory = uv_get_constrained_memory();
  if (constrained_memory != 0) {
    SetNamedString(env, usage, "constrained_memory", std::to_string(static_cast<unsigned long long>(constrained_memory)));
  }

  SetNamedInt64(env, page_faults, "IORequired", static_cast<int64_t>(rusage.ru_majflt));
  SetNamedInt64(env, page_faults, "IONotRequired", static_cast<int64_t>(rusage.ru_minflt));
  SetNamedValue(env, usage, "pageFaults", page_faults);

  SetNamedInt64(env, fs_activity, "reads", static_cast<int64_t>(rusage.ru_inblock));
  SetNamedInt64(env, fs_activity, "writes", static_cast<int64_t>(rusage.ru_oublock));
  SetNamedValue(env, usage, "fsActivity", fs_activity);

  *out = usage;
  return true;
}

std::string ResolveReportEndpointHost(uv_loop_t* loop, const sockaddr* addr) {
  if (loop == nullptr || addr == nullptr) return {};
  uv_getnameinfo_t request{};
  if (uv_getnameinfo(loop, &request, nullptr, const_cast<sockaddr*>(addr), NI_NUMERICSERV) != 0) {
    return {};
  }
  return request.host;
}

bool BuildSocketEndpoint(napi_env env,
                         uv_loop_t* loop,
                         const sockaddr* addr,
                         bool exclude_network,
                         napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (addr == nullptr) return true;

  napi_value endpoint = nullptr;
  if (napi_create_object(env, &endpoint) != napi_ok || endpoint == nullptr) return false;

  std::string host;
  int port = 0;
  if (addr->sa_family == AF_INET) {
    char ip[INET_ADDRSTRLEN] = {'\0'};
    const auto* ipv4 = reinterpret_cast<const sockaddr_in*>(addr);
    if (uv_ip4_name(ipv4, ip, sizeof(ip)) == 0) {
      SetNamedString(env, endpoint, "ip4", ip);
      host = ip;
    }
    port = ntohs(ipv4->sin_port);
  } else if (addr->sa_family == AF_INET6) {
    char ip[INET6_ADDRSTRLEN] = {'\0'};
    const auto* ipv6 = reinterpret_cast<const sockaddr_in6*>(addr);
    if (uv_ip6_name(ipv6, ip, sizeof(ip)) == 0) {
      SetNamedString(env, endpoint, "ip6", ip);
      host = ip;
    }
    port = ntohs(ipv6->sin6_port);
  }

  if (!exclude_network) {
    const std::string resolved = ResolveReportEndpointHost(loop, addr);
    if (!resolved.empty()) host = resolved;
  }
  SetNamedString(env, endpoint, "host", host);
  SetNamedInt32(env, endpoint, "port", port);
  *out = endpoint;
  return true;
}

bool BuildPipeName(uv_pipe_t* pipe, bool peer, std::optional<std::string>* out) {
  if (out == nullptr || pipe == nullptr) return false;
  out->reset();

  size_t size = 256;
  std::vector<char> buffer(size, '\0');
  int rc = peer ? uv_pipe_getpeername(pipe, buffer.data(), &size)
                : uv_pipe_getsockname(pipe, buffer.data(), &size);
  if (rc == UV_ENOBUFS) {
    buffer.assign(size + 1, '\0');
    rc = peer ? uv_pipe_getpeername(pipe, buffer.data(), &size)
              : uv_pipe_getsockname(pipe, buffer.data(), &size);
  }
  if (rc != 0 || size == 0) return true;
  if (size > buffer.size()) size = buffer.size();
  out->emplace(buffer.data(), size);
  if (out->has_value() && !out->value().empty() && out->value().back() == '\0') {
    out->value().pop_back();
  }
  return true;
}

bool BuildPathHandleName(uv_handle_t* handle, std::optional<std::string>* out) {
  if (out == nullptr || handle == nullptr) return false;
  out->reset();

  size_t size = 256;
  std::vector<char> buffer(size, '\0');
  int rc = -1;
  if (handle->type == UV_FS_EVENT) {
    rc = uv_fs_event_getpath(reinterpret_cast<uv_fs_event_t*>(handle), buffer.data(), &size);
  } else if (handle->type == UV_FS_POLL) {
    rc = uv_fs_poll_getpath(reinterpret_cast<uv_fs_poll_t*>(handle), buffer.data(), &size);
  } else {
    return true;
  }
  if (rc == UV_ENOBUFS) {
    buffer.assign(size + 1, '\0');
    if (handle->type == UV_FS_EVENT) {
      rc = uv_fs_event_getpath(reinterpret_cast<uv_fs_event_t*>(handle), buffer.data(), &size);
    } else {
      rc = uv_fs_poll_getpath(reinterpret_cast<uv_fs_poll_t*>(handle), buffer.data(), &size);
    }
  }
  if (rc != 0 || size == 0) return true;
  if (size > buffer.size()) size = buffer.size();
  out->emplace(buffer.data(), size);
  if (out->has_value() && !out->value().empty() && out->value().back() == '\0') {
    out->value().pop_back();
  }
  return true;
}

struct ReportLibuvBuildState {
  napi_env env = nullptr;
  napi_value array = nullptr;
  bool exclude_network = false;
  uint32_t index = 0;
};

void AppendReportLibuvHandle(uv_handle_t* handle, void* arg) {
  auto* state = static_cast<ReportLibuvBuildState*>(arg);
  if (state == nullptr || state->env == nullptr || state->array == nullptr || handle == nullptr) return;

  napi_env env = state->env;
  napi_value entry = nullptr;
  if (napi_create_object(env, &entry) != napi_ok || entry == nullptr) return;

  const char* type_name = uv_handle_type_name(uv_handle_get_type(handle));
  SetNamedString(env, entry, "type", type_name != nullptr ? type_name : "unknown");
  SetNamedBool(env, entry, "is_active", uv_is_active(handle) != 0);
  SetNamedBool(env, entry, "is_referenced", uv_has_ref(handle) != 0);
  SetNamedString(env, entry, "address", PointerToHexString(reinterpret_cast<uint64_t>(handle)));

  switch (handle->type) {
    case UV_FS_EVENT:
    case UV_FS_POLL: {
      std::optional<std::string> filename;
      if (BuildPathHandleName(handle, &filename) && filename.has_value()) {
        SetNamedString(env, entry, "filename", *filename);
      }
      break;
    }
    case UV_PROCESS: {
      auto* process = reinterpret_cast<uv_process_t*>(handle);
      SetNamedInt64(env, entry, "pid", static_cast<int64_t>(process->pid));
      break;
    }
    case UV_TCP: {
      auto* tcp = reinterpret_cast<uv_tcp_t*>(handle);
      sockaddr_storage storage{};
      int size = sizeof(storage);
      napi_value local = nullptr;
      if (uv_tcp_getsockname(tcp, reinterpret_cast<sockaddr*>(&storage), &size) == 0) {
        (void)BuildSocketEndpoint(env, handle->loop, reinterpret_cast<sockaddr*>(&storage), state->exclude_network, &local);
      }
      if (local != nullptr) {
        SetNamedValue(env, entry, "localEndpoint", local);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "localEndpoint", null_value);
      }
      size = sizeof(storage);
      napi_value remote = nullptr;
      if (uv_tcp_getpeername(tcp, reinterpret_cast<sockaddr*>(&storage), &size) == 0) {
        (void)BuildSocketEndpoint(env, handle->loop, reinterpret_cast<sockaddr*>(&storage), state->exclude_network, &remote);
      }
      if (remote != nullptr) {
        SetNamedValue(env, entry, "remoteEndpoint", remote);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "remoteEndpoint", null_value);
      }
      break;
    }
    case UV_UDP: {
      auto* udp = reinterpret_cast<uv_udp_t*>(handle);
      sockaddr_storage storage{};
      int size = sizeof(storage);
      napi_value local = nullptr;
      if (uv_udp_getsockname(udp, reinterpret_cast<sockaddr*>(&storage), &size) == 0) {
        (void)BuildSocketEndpoint(env, handle->loop, reinterpret_cast<sockaddr*>(&storage), state->exclude_network, &local);
      }
      if (local != nullptr) {
        SetNamedValue(env, entry, "localEndpoint", local);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "localEndpoint", null_value);
      }
      size = sizeof(storage);
      napi_value remote = nullptr;
      if (uv_udp_getpeername(udp, reinterpret_cast<sockaddr*>(&storage), &size) == 0) {
        (void)BuildSocketEndpoint(env, handle->loop, reinterpret_cast<sockaddr*>(&storage), state->exclude_network, &remote);
      }
      if (remote != nullptr) {
        SetNamedValue(env, entry, "remoteEndpoint", remote);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "remoteEndpoint", null_value);
      }
      break;
    }
    case UV_NAMED_PIPE: {
      auto* pipe = reinterpret_cast<uv_pipe_t*>(handle);
      std::optional<std::string> local;
      std::optional<std::string> remote;
      (void)BuildPipeName(pipe, false, &local);
      (void)BuildPipeName(pipe, true, &remote);
      if (local.has_value()) {
        SetNamedString(env, entry, "localEndpoint", *local);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "localEndpoint", null_value);
      }
      if (remote.has_value()) {
        SetNamedString(env, entry, "remoteEndpoint", *remote);
      } else {
        napi_value null_value = nullptr;
        napi_get_null(env, &null_value);
        SetNamedValue(env, entry, "remoteEndpoint", null_value);
      }
      break;
    }
    case UV_TIMER: {
      auto* timer = reinterpret_cast<uv_timer_t*>(handle);
      const uint64_t due = timer->timeout;
      const uint64_t now = uv_now(timer->loop);
      SetNamedInt64(env, entry, "repeat", static_cast<int64_t>(uv_timer_get_repeat(timer)));
      SetNamedInt64(env, entry, "firesInMsFromNow", static_cast<int64_t>(due >= now ? due - now : 0));
      SetNamedBool(env, entry, "expired", now >= due);
      break;
    }
    case UV_TTY: {
      int width = 0;
      int height = 0;
      if (uv_tty_get_winsize(reinterpret_cast<uv_tty_t*>(handle), &width, &height) == 0) {
        SetNamedInt32(env, entry, "width", width);
        SetNamedInt32(env, entry, "height", height);
      }
      break;
    }
    case UV_SIGNAL: {
      auto* signal = reinterpret_cast<uv_signal_t*>(handle);
      SetNamedInt32(env, entry, "signum", signal->signum);
      break;
    }
    default:
      break;
  }

  if (handle->type == UV_TCP || handle->type == UV_UDP
#ifndef _WIN32
      || handle->type == UV_NAMED_PIPE
#endif
  ) {
    int send_size = 0;
    int recv_size = 0;
    (void)uv_send_buffer_size(handle, &send_size);
    (void)uv_recv_buffer_size(handle, &recv_size);
    SetNamedInt32(env, entry, "sendBufferSize", send_size);
    SetNamedInt32(env, entry, "recvBufferSize", recv_size);
  }

#ifndef _WIN32
  if (handle->type == UV_TCP || handle->type == UV_NAMED_PIPE ||
      handle->type == UV_TTY || handle->type == UV_UDP || handle->type == UV_POLL) {
    uv_os_fd_t fd_value = -1;
    if (uv_fileno(handle, &fd_value) == 0) {
      SetNamedInt32(env, entry, "fd", static_cast<int32_t>(fd_value));
      if (fd_value == STDIN_FILENO) {
        SetNamedString(env, entry, "stdio", "stdin");
      } else if (fd_value == STDOUT_FILENO) {
        SetNamedString(env, entry, "stdio", "stdout");
      } else if (fd_value == STDERR_FILENO) {
        SetNamedString(env, entry, "stdio", "stderr");
      }
    }
  }
#endif

  if (handle->type == UV_TCP || handle->type == UV_NAMED_PIPE || handle->type == UV_TTY) {
    auto* stream = reinterpret_cast<uv_stream_t*>(handle);
    SetNamedInt64(env, entry, "writeQueueSize", static_cast<int64_t>(stream->write_queue_size));
    SetNamedBool(env, entry, "readable", uv_is_readable(stream) != 0);
    SetNamedBool(env, entry, "writable", uv_is_writable(stream) != 0);
  }
  if (handle->type == UV_UDP) {
    auto* udp = reinterpret_cast<uv_udp_t*>(handle);
    SetNamedInt64(env, entry, "writeQueueSize", static_cast<int64_t>(uv_udp_get_send_queue_size(udp)));
    SetNamedInt64(env, entry, "writeQueueCount", static_cast<int64_t>(uv_udp_get_send_queue_count(udp)));
  }

  napi_set_element(env, state->array, state->index++, entry);
}

bool BuildReportLibuv(napi_env env, bool exclude_network, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  uv_loop_t* loop = nullptr;
  if (napi_get_uv_event_loop(env, &loop) != napi_ok || loop == nullptr) return false;

  napi_value array = nullptr;
  if (napi_create_array(env, &array) != napi_ok || array == nullptr) return false;

  ReportLibuvBuildState state{env, array, exclude_network, 0};
  uv_walk(loop, AppendReportLibuvHandle, &state);

  napi_value loop_entry = nullptr;
  if (napi_create_object(env, &loop_entry) != napi_ok || loop_entry == nullptr) return false;
  SetNamedString(env, loop_entry, "type", "loop");
  SetNamedBool(env, loop_entry, "is_active", uv_loop_alive(loop) != 0);
  SetNamedString(env, loop_entry, "address", PointerToHexString(reinterpret_cast<uint64_t>(loop)));
  SetNamedDouble(env, loop_entry, "loopIdleTimeSeconds", static_cast<double>(uv_metrics_idle_time(loop)) / kNanosPerSec);
  napi_set_element(env, array, state.index++, loop_entry);

  *out = array;
  return true;
}

napi_value StringifyReportObject(napi_env env, napi_value report_obj, bool compact) {
  napi_value global = nullptr;
  napi_value json_obj = nullptr;
  napi_value stringify_fn = nullptr;
  if (report_obj == nullptr ||
      napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "JSON", &json_obj) != napi_ok || json_obj == nullptr ||
      napi_get_named_property(env, json_obj, "stringify", &stringify_fn) != napi_ok || stringify_fn == nullptr) {
    return nullptr;
  }
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  napi_value space = nullptr;
  if (compact) {
    napi_create_int32(env, 0, &space);
  } else {
    napi_create_int32(env, 2, &space);
  }
  napi_value argv[3] = {report_obj, null_value, space};
  napi_value json_string = nullptr;
  if (napi_call_function(env, json_obj, stringify_fn, 3, argv, &json_string) != napi_ok || json_string == nullptr) {
    return nullptr;
  }
  return json_string;
}

bool ParseJsonObject(napi_env env, const std::string& json, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  napi_value global = nullptr;
  napi_value json_obj = nullptr;
  napi_value parse_fn = nullptr;
  napi_value json_string = nullptr;
  if (json.empty() ||
      napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "JSON", &json_obj) != napi_ok || json_obj == nullptr ||
      napi_get_named_property(env, json_obj, "parse", &parse_fn) != napi_ok || parse_fn == nullptr ||
      napi_create_string_utf8(env, json.c_str(), json.size(), &json_string) != napi_ok ||
      json_string == nullptr) {
    return false;
  }
  napi_value argv[1] = {json_string};
  return napi_call_function(env, json_obj, parse_fn, 1, argv, out) == napi_ok && *out != nullptr;
}

void AppendWorkerReportsToReport(napi_env env, napi_value workers) {
  if (workers == nullptr) return;
  const std::vector<EdgeWorkerReportEntry> reports = EdgeWorkerCollectReports(env);
  uint32_t index = 0;
  for (const auto& entry : reports) {
    napi_value worker_report = nullptr;
    if (!ParseJsonObject(env, entry.json, &worker_report) || worker_report == nullptr) continue;
    napi_value header = nullptr;
    if (napi_get_named_property(env, worker_report, "header", &header) == napi_ok && header != nullptr) {
      SetNamedString(env, header, "event", "Worker thread subreport [" + entry.thread_name + "]");
      SetNamedInt32(env, header, "threadId", entry.thread_id);
    }
    napi_set_element(env, workers, index++, worker_report);
  }
}

napi_value BuildReportObject(napi_env env,
                             const std::string& event_message,
                             const std::string& trigger,
                             const std::string& report_filename,
                             napi_value error_value) {
  napi_value report = nullptr;
  if (napi_create_object(env, &report) != napi_ok || report == nullptr) return nullptr;

  napi_value process_obj = nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "process", &process_obj) != napi_ok ||
      process_obj == nullptr) {
    return nullptr;
  }

  napi_value header = nullptr;
  if (napi_create_object(env, &header) != napi_ok || header == nullptr) return nullptr;
  SetNamedString(env, header, "event", event_message);
  SetNamedString(env, header, "trigger", trigger);
  if (!report_filename.empty()) {
    SetNamedString(env, header, "filename", report_filename);
  } else {
    napi_value null_value = nullptr;
    napi_get_null(env, &null_value);
    SetNamedValue(env, header, "filename", null_value);
  }

  const auto now_tp = std::chrono::system_clock::now();
  const auto now = std::chrono::system_clock::to_time_t(now_tp);
  std::tm utc_tm{};
#if defined(_WIN32)
  gmtime_s(&utc_tm, &now);
#else
  gmtime_r(&now, &utc_tm);
#endif
  char time_buf[64] = {'\0'};
  std::strftime(time_buf, sizeof(time_buf), "%Y-%m-%dT%H:%M:%SZ", &utc_tm);
  SetNamedString(env, header, "dumpEventTime", time_buf);
  SetNamedString(
      env,
      header,
      "dumpEventTimeStamp",
      std::to_string(std::chrono::duration_cast<std::chrono::milliseconds>(now_tp.time_since_epoch()).count()));
  SetNamedInt32(env, header, "processId", static_cast<int32_t>(uv_os_getpid()));
  const int32_t thread_id =
      EdgeWorkerEnvIsMainThread(env) ? static_cast<int32_t>(uv_os_getpid()) : EdgeWorkerEnvThreadId(env);
  SetNamedInt32(env, header, "threadId", thread_id);
  SetNamedInt32(env, header, "wordSize", static_cast<int32_t>(sizeof(void*) * 8));

  napi_value command_line = nullptr;
  if (BuildReportCommandLine(env, process_obj, &command_line) && command_line != nullptr) {
    SetNamedValue(env, header, "commandLine", command_line);
  }
  napi_value node_version = nullptr;
  if (GetNamedPropertyIfPresent(env, process_obj, "version", &node_version)) {
    SetNamedValue(env, header, "nodejsVersion", node_version);
  }
  napi_value arch = nullptr;
  if (GetNamedPropertyIfPresent(env, process_obj, "arch", &arch)) {
    SetNamedValue(env, header, "arch", arch);
  }
  napi_value platform = nullptr;
  if (GetNamedPropertyIfPresent(env, process_obj, "platform", &platform)) {
    SetNamedValue(env, header, "platform", platform);
  }
  napi_value versions = nullptr;
  if (GetNamedPropertyIfPresent(env, process_obj, "versions", &versions)) {
    SetNamedValue(env, header, "componentVersions", versions);
  }
  napi_value release = nullptr;
  if (GetNamedPropertyIfPresent(env, process_obj, "release", &release)) {
    SetNamedValue(env, header, "release", release);
  }

  char cwd_buf[4096] = {'\0'};
  size_t cwd_size = sizeof(cwd_buf);
  if (uv_cwd(cwd_buf, &cwd_size) == 0) {
    SetNamedString(env, header, "cwd", std::string(cwd_buf, cwd_size));
  }

  uv_utsname_t os_info{};
  if (uv_os_uname(&os_info) == 0) {
    SetNamedString(env, header, "osName", os_info.sysname);
    SetNamedString(env, header, "osRelease", os_info.release);
    SetNamedString(env, header, "osVersion", os_info.version);
    SetNamedString(env, header, "osMachine", os_info.machine);
  }

  napi_value cpus = nullptr;
  if (BuildReportCpus(env, &cpus) && cpus != nullptr) {
    SetNamedValue(env, header, "cpus", cpus);
  }

  ReportBindingState* state = GetReportState(env);
  if (state == nullptr || !state->exclude_network) {
    napi_value network_interfaces = nullptr;
    if (BuildReportNetworkInterfaces(env, &network_interfaces) && network_interfaces != nullptr) {
      SetNamedValue(env, header, "networkInterfaces", network_interfaces);
    }
  }

  char host[UV_MAXHOSTNAMESIZE] = {'\0'};
  size_t host_size = sizeof(host);
  if (uv_os_gethostname(host, &host_size) == 0) {
    SetNamedString(env, header, "host", std::string(host, host_size));
  }

  SetNamedString(env, header, "glibcVersionRuntime", GetGlibcRuntimeVersion());
  SetNamedString(env, header, "glibcVersionCompiler", GetGlibcCompilerVersion());
  SetNamedInt32(env, header, "reportVersion", 5);
  SetNamedValue(env, report, "header", header);

  napi_value native_stack = nullptr;
  napi_create_array_with_length(env, 1, &native_stack);
  napi_value frame = nullptr;
  napi_create_object(env, &frame);
  SetNamedString(env, frame, "pc", PointerToHexString(reinterpret_cast<uint64_t>(&BuildReportObject)));
  SetNamedString(env, frame, "symbol", "edge::BuildReportObject");
  napi_set_element(env, native_stack, 0, frame);
  SetNamedValue(env, report, "nativeStack", native_stack);

  napi_value js_stack = nullptr;
  if (BuildJavascriptStack(env, error_value, event_message, &js_stack) && js_stack != nullptr) {
    SetNamedValue(env, report, "javascriptStack", js_stack);
  }

  napi_value libuv = nullptr;
  if (BuildReportLibuv(env, state != nullptr && state->exclude_network, &libuv) && libuv != nullptr) {
    SetNamedValue(env, report, "libuv", libuv);
  }

  napi_value shared_objects = nullptr;
  napi_create_array(env, &shared_objects);
  SetNamedValue(env, report, "sharedObjects", shared_objects);

  napi_value usage = nullptr;
  if (BuildResourceUsage(env, &usage) && usage != nullptr) {
    SetNamedValue(env, report, "resourceUsage", usage);
  }

  napi_value workers = nullptr;
  napi_create_array(env, &workers);
  AppendWorkerReportsToReport(env, workers);
  SetNamedValue(env, report, "workers", workers);

  if (state == nullptr || !state->exclude_env) {
    napi_value env_snapshot = nullptr;
    if (SnapshotProcessEnv(env, process_obj, &env_snapshot) && env_snapshot != nullptr) {
      SetNamedValue(env, report, "environmentVariables", env_snapshot);
    }
  }

  napi_value user_limits = MakeReportUserLimits(env);
  if (user_limits != nullptr) {
    SetNamedValue(env, report, "userLimits", user_limits);
  }

  napi_value js_heap = nullptr;
  if (BuildJavascriptHeap(env, process_obj, &js_heap) && js_heap != nullptr) {
    SetNamedValue(env, report, "javascriptHeap", js_heap);
  }

  return report;
}

std::string BuildDefaultReportFilename() {
  const auto now_tp = std::chrono::system_clock::now();
  const auto now = std::chrono::system_clock::to_time_t(now_tp);
  std::tm local_tm{};
#if defined(_WIN32)
  localtime_s(&local_tm, &now);
#else
  localtime_r(&now, &local_tm);
#endif
  char date_buf[16] = {'\0'};
  char time_buf[16] = {'\0'};
  std::strftime(date_buf, sizeof(date_buf), "%Y%m%d", &local_tm);
  std::strftime(time_buf, sizeof(time_buf), "%H%M%S", &local_tm);
  std::ostringstream oss;
  oss << "report." << date_buf << "." << time_buf << "." << uv_os_getpid() << "."
      << uv_os_getpid() << ".";
  return oss.str();
}

std::string JoinPath(const std::string& dir, const std::string& file) {
  namespace fs = std::filesystem;
  fs::path p = fs::path(dir) / fs::path(file);
  return p.lexically_normal().string();
}

bool HasExecArgvFlag(const std::vector<std::string>& exec_argv, const char* flag) {
  for (const auto& arg : exec_argv) {
    if (arg == flag) return true;
  }
  return false;
}

std::string GetExecArgvStringOption(const std::vector<std::string>& exec_argv, const char* prefix) {
  if (prefix == nullptr || prefix[0] == '\0') return {};
  const std::string needle(prefix);
  for (const auto& arg : exec_argv) {
    if (arg.rfind(needle, 0) == 0) {
      return arg.substr(needle.size());
    }
  }
  return {};
}

std::string ResolveReportFilename(ReportBindingState* state, const std::string& requested_file) {
  std::string filename = requested_file;
  if (filename.empty()) {
    if (state == nullptr || state->filename.empty()) {
      if (state != nullptr) state->sequence++;
      const uint64_t sequence = state != nullptr ? state->sequence : 1;
      std::ostringstream generated;
      generated << BuildDefaultReportFilename() << sequence << ".json";
      filename = generated.str();
    } else {
      filename = state->filename;
    }
  }
  return filename;
}

std::string ResolveReportOutputPath(const ReportBindingState* state, const std::string& filename) {
  const bool use_stdout = filename == "stdout";
  const bool use_stderr = filename == "stderr";
  const std::string directory = state != nullptr ? state->directory : std::string();
  return (use_stdout || use_stderr || std::filesystem::path(filename).is_absolute() || directory.empty())
             ? filename
             : JoinPath(directory, filename);
}

bool WriteReportPayload(const ReportBindingState* state,
                        const std::string& filename,
                        const std::string& payload) {
  const bool use_stdout = filename == "stdout";
  const bool use_stderr = filename == "stderr";
  const std::string output_path = ResolveReportOutputPath(state, filename);

  if (use_stdout) {
    std::cout << payload;
    if (payload.empty() || payload.back() != '\n') std::cout << '\n';
    std::cerr << "\nNode.js report completed" << std::endl;
    return true;
  }
  if (use_stderr) {
    std::cerr << payload;
    if (payload.empty() || payload.back() != '\n') std::cerr << '\n';
    return true;
  }

  errno = 0;
  std::ofstream out(output_path, std::ios::out | std::ios::binary);
  if (!out.is_open()) {
    std::cerr << "\nFailed to open Node.js report file: " << filename;
    const std::string directory = state != nullptr ? state->directory : std::string();
    if (!directory.empty()) {
      std::cerr << " directory: " << directory;
    }
    std::cerr << " (errno: " << errno << ")" << std::endl;
    return false;
  }
  out << payload;
  out.close();
  std::cerr << "\nWriting Node.js report to file: " << filename
            << "\nNode.js report completed" << std::endl;
  return true;
}

void WriteJsonStringArray(SimpleJsonWriter& writer,
                          const char* key,
                          const std::vector<std::string>& values) {
  writer.json_arraystart(key);
  for (const auto& value : values) {
    writer.json_element(value);
  }
  writer.json_arrayend();
}

void WriteReportComponentVersions(SimpleJsonWriter& writer, bool has_intl) {
  writer.json_objectstart("componentVersions");
  writer.json_keyvalue("node", NODE_COMPAT_VERSION_STRING);
  for (const auto& entry : BuildProcessVersionEntries(has_intl)) {
    if (!entry.value.empty()) writer.json_keyvalue(entry.key, entry.value);
  }
  writer.json_objectend();
}

void WriteReportRelease(SimpleJsonWriter& writer) {
  writer.json_objectstart("release");
  writer.json_keyvalue("name", "node");
#if NODE_VERSION_IS_LTS
  writer.json_keyvalue("lts", NODE_VERSION_LTS_CODENAME);
#endif
  const std::string release_url_prefix =
      std::string("https://nodejs.org/download/release/v") + NODE_COMPAT_VERSION_STRING + "/";
  const std::string release_file_prefix =
      release_url_prefix + "node-v" + NODE_COMPAT_VERSION_STRING;
  writer.json_keyvalue("sourceUrl", release_file_prefix + ".tar.gz");
  writer.json_keyvalue("headersUrl", release_file_prefix + "-headers.tar.gz");
  writer.json_objectend();
}

void WriteReportCpuInfo(SimpleJsonWriter& writer) {
  writer.json_arraystart("cpus");
  uv_cpu_info_t* cpu_info = nullptr;
  int count = 0;
  if (uv_cpu_info(&cpu_info, &count) == 0 && count >= 0) {
    for (int i = 0; i < count; ++i) {
      writer.json_start();
      writer.json_keyvalue("model", cpu_info[i].model != nullptr ? cpu_info[i].model : "");
      writer.json_keyvalue("speed", static_cast<int64_t>(cpu_info[i].speed));
      writer.json_keyvalue("user", static_cast<int64_t>(cpu_info[i].cpu_times.user));
      writer.json_keyvalue("nice", static_cast<int64_t>(cpu_info[i].cpu_times.nice));
      writer.json_keyvalue("sys", static_cast<int64_t>(cpu_info[i].cpu_times.sys));
      writer.json_keyvalue("idle", static_cast<int64_t>(cpu_info[i].cpu_times.idle));
      writer.json_keyvalue("irq", static_cast<int64_t>(cpu_info[i].cpu_times.irq));
      writer.json_end();
    }
    uv_free_cpu_info(cpu_info, count);
  }
  writer.json_arrayend();
}

void WriteReportNetworkInterfaces(SimpleJsonWriter& writer) {
  writer.json_arraystart("networkInterfaces");
  uv_interface_address_t* interfaces = nullptr;
  int count = 0;
  if (uv_interface_addresses(&interfaces, &count) == 0 && count >= 0) {
    for (int i = 0; i < count; ++i) {
      writer.json_start();
      writer.json_keyvalue("name", interfaces[i].name != nullptr ? interfaces[i].name : "");
      writer.json_keyvalue("internal", interfaces[i].is_internal != 0);

      char mac[18] = {'\0'};
      std::snprintf(mac,
                    sizeof(mac),
                    "%02X:%02X:%02X:%02X:%02X:%02X",
                    static_cast<unsigned char>(interfaces[i].phys_addr[0]),
                    static_cast<unsigned char>(interfaces[i].phys_addr[1]),
                    static_cast<unsigned char>(interfaces[i].phys_addr[2]),
                    static_cast<unsigned char>(interfaces[i].phys_addr[3]),
                    static_cast<unsigned char>(interfaces[i].phys_addr[4]),
                    static_cast<unsigned char>(interfaces[i].phys_addr[5]));
      writer.json_keyvalue("mac", mac);

      if (interfaces[i].address.address4.sin_family == AF_INET) {
        char address[INET_ADDRSTRLEN] = {'\0'};
        char netmask[INET_ADDRSTRLEN] = {'\0'};
        if (uv_ip4_name(&interfaces[i].address.address4, address, sizeof(address)) == 0) {
          writer.json_keyvalue("address", address);
        }
        if (uv_ip4_name(&interfaces[i].netmask.netmask4, netmask, sizeof(netmask)) == 0) {
          writer.json_keyvalue("netmask", netmask);
        }
        writer.json_keyvalue("family", "IPv4");
      } else if (interfaces[i].address.address4.sin_family == AF_INET6) {
        char address[INET6_ADDRSTRLEN] = {'\0'};
        char netmask[INET6_ADDRSTRLEN] = {'\0'};
        if (uv_ip6_name(&interfaces[i].address.address6, address, sizeof(address)) == 0) {
          writer.json_keyvalue("address", address);
        }
        if (uv_ip6_name(&interfaces[i].netmask.netmask6, netmask, sizeof(netmask)) == 0) {
          writer.json_keyvalue("netmask", netmask);
        }
        writer.json_keyvalue("family", "IPv6");
        writer.json_keyvalue("scopeid",
                             static_cast<int64_t>(interfaces[i].address.address6.sin6_scope_id));
      } else {
        writer.json_keyvalue("family", "unknown");
      }
      writer.json_end();
    }
    uv_free_interface_addresses(interfaces, count);
  }
  writer.json_arrayend();
}

void WriteReportResourceUsage(SimpleJsonWriter& writer) {
  writer.json_objectstart("resourceUsage");
  uv_rusage_t rusage{};
  (void)uv_getrusage(&rusage);

  size_t rss = 0;
  (void)uv_resident_set_memory(&rss);
  writer.json_keyvalue("userCpuSeconds",
                       static_cast<double>(rusage.ru_utime.tv_sec) +
                           static_cast<double>(rusage.ru_utime.tv_usec) / kMicrosPerSec);
  writer.json_keyvalue("kernelCpuSeconds",
                       static_cast<double>(rusage.ru_stime.tv_sec) +
                           static_cast<double>(rusage.ru_stime.tv_usec) / kMicrosPerSec);
  writer.json_keyvalue("cpuConsumptionPercent", 0.0);
  writer.json_keyvalue("userCpuConsumptionPercent", 0.0);
  writer.json_keyvalue("kernelCpuConsumptionPercent", 0.0);
  writer.json_keyvalue("maxRss", std::to_string(static_cast<uint64_t>(rusage.ru_maxrss)));
  writer.json_keyvalue("rss", std::to_string(static_cast<uint64_t>(rss)));
  writer.json_keyvalue("free_memory", std::to_string(static_cast<uint64_t>(uv_get_free_memory())));
  writer.json_keyvalue("total_memory", std::to_string(static_cast<uint64_t>(uv_get_total_memory())));
  writer.json_keyvalue("available_memory", std::to_string(static_cast<uint64_t>(uv_get_available_memory())));
  const uint64_t constrained_memory = uv_get_constrained_memory();
  if (constrained_memory > 0) {
    writer.json_keyvalue("constrained_memory", std::to_string(constrained_memory));
  }
  writer.json_objectstart("pageFaults");
  writer.json_keyvalue("IORequired", static_cast<int64_t>(rusage.ru_majflt));
  writer.json_keyvalue("IONotRequired", static_cast<int64_t>(rusage.ru_minflt));
  writer.json_objectend();
  writer.json_objectstart("fsActivity");
  writer.json_keyvalue("reads", static_cast<int64_t>(rusage.ru_inblock));
  writer.json_keyvalue("writes", static_cast<int64_t>(rusage.ru_oublock));
  writer.json_objectend();
  writer.json_objectend();
}

void WriteReportEnvironmentVariables(SimpleJsonWriter& writer) {
  writer.json_objectstart("environmentVariables");
#if defined(_WIN32)
  char** entries = _environ;
#else
  char** entries = environ;
#endif
  if (entries != nullptr) {
    for (; *entries != nullptr; ++entries) {
      const char* sep = std::strchr(*entries, '=');
      if (sep == nullptr) continue;
      const std::string key(*entries, static_cast<size_t>(sep - *entries));
      writer.json_keyvalue(key, sep + 1);
    }
  }
  writer.json_objectend();
}

void WriteReportUserLimits(SimpleJsonWriter& writer) {
  writer.json_objectstart("userLimits");
  const char* keys[] = {
      "core_file_size_blocks",
      "data_seg_size_bytes",
      "file_size_blocks",
      "max_locked_memory_bytes",
      "max_memory_size_bytes",
      "open_files",
      "stack_size_bytes",
      "cpu_time_seconds",
      "max_user_processes",
      "virtual_memory_bytes",
  };
  for (const char* key : keys) {
    writer.json_objectstart(key);
    writer.json_keyvalue("soft", "unlimited");
    writer.json_keyvalue("hard", "unlimited");
    writer.json_objectend();
  }
  writer.json_objectend();
}

void WriteReportJavascriptHeap(SimpleJsonWriter& writer,
                               napi_env env,
                               const ReportBindingState* state) {
  double heap_total = 0;
  double heap_used = 0;
  double external = 0;
  double array_buffers = 0;
  if (env != nullptr) {
    (void)unofficial_napi_get_process_memory_info(env, &heap_total, &heap_used, &external, &array_buffers);
  }

  writer.json_objectstart("javascriptHeap");
  writer.json_keyvalue("totalMemory", static_cast<int64_t>(heap_total));
  writer.json_keyvalue("executableMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("totalCommittedMemory", static_cast<int64_t>(heap_total));
  writer.json_keyvalue("availableMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("totalGlobalHandlesMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("usedGlobalHandlesMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("usedMemory", static_cast<int64_t>(heap_used));
  writer.json_keyvalue("memoryLimit", static_cast<int64_t>(state != nullptr ? state->max_heap_size_bytes : 0));
  writer.json_keyvalue("mallocedMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("externalMemory", static_cast<int64_t>(external));
  writer.json_keyvalue("peakMallocedMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("nativeContextCount", static_cast<int64_t>(0));
  writer.json_keyvalue("detachedContextCount", static_cast<int64_t>(0));
  writer.json_keyvalue("doesZapGarbage", static_cast<int64_t>(0));
  writer.json_objectstart("heapSpaces");
  writer.json_objectstart("new_space");
  writer.json_keyvalue("memorySize", static_cast<int64_t>(0));
  writer.json_keyvalue("committedMemory", static_cast<int64_t>(0));
  writer.json_keyvalue("capacity", static_cast<int64_t>(0));
  writer.json_keyvalue("used", static_cast<int64_t>(0));
  writer.json_keyvalue("available", static_cast<int64_t>(0));
  writer.json_objectend();
  writer.json_objectend();
  writer.json_objectend();
}

std::string BuildNativeFatalReportPayload(napi_env env,
                                          const ReportBindingState* state,
                                          const std::string& event_message,
                                          const std::string& trigger,
                                          const std::string& report_filename) {
  std::ostringstream out;
  SimpleJsonWriter writer(out, state != nullptr && state->compact);

  writer.json_start();
  writer.json_objectstart("header");
  writer.json_keyvalue("reportVersion", 5);
  writer.json_keyvalue("event", event_message);
  writer.json_keyvalue("trigger", trigger);
  if (!report_filename.empty()) {
    writer.json_keyvalue("filename", report_filename);
  } else {
    writer.json_keyvalue("filename", SimpleJsonWriter::Null{});
  }

  const auto now_tp = std::chrono::system_clock::now();
  const auto now = std::chrono::system_clock::to_time_t(now_tp);
  std::tm utc_tm{};
#if defined(_WIN32)
  gmtime_s(&utc_tm, &now);
#else
  gmtime_r(&now, &utc_tm);
#endif
  char time_buf[64] = {'\0'};
  std::strftime(time_buf, sizeof(time_buf), "%Y-%m-%dT%H:%M:%SZ", &utc_tm);
  writer.json_keyvalue("dumpEventTime", time_buf);
  writer.json_keyvalue(
      "dumpEventTimeStamp",
      std::to_string(std::chrono::duration_cast<std::chrono::milliseconds>(now_tp.time_since_epoch()).count()));
  writer.json_keyvalue("processId", static_cast<int64_t>(uv_os_getpid()));
  writer.json_keyvalue("threadId", static_cast<int64_t>(uv_os_getpid()));
  writer.json_keyvalue("wordSize", static_cast<int64_t>(sizeof(void*) * 8));

  if (state != nullptr && !state->command_line.empty()) {
    WriteJsonStringArray(writer, "commandLine", state->command_line);
  } else {
    WriteJsonStringArray(writer, "commandLine", BuildCommandLineSnapshot({}, {}, ""));
  }

  writer.json_keyvalue("nodejsVersion", NODE_VERSION);
  writer.json_keyvalue("arch", DetectArch());
  writer.json_keyvalue("platform", DetectPlatform());
  WriteReportComponentVersions(writer, state != nullptr && state->has_intl);
  WriteReportRelease(writer);

  char cwd_buf[4096] = {'\0'};
  size_t cwd_size = sizeof(cwd_buf);
  if (uv_cwd(cwd_buf, &cwd_size) == 0) {
    writer.json_keyvalue("cwd", std::string(cwd_buf, cwd_size));
  } else {
    writer.json_keyvalue("cwd", "");
  }

  uv_utsname_t os_info{};
  if (uv_os_uname(&os_info) == 0) {
    writer.json_keyvalue("osName", os_info.sysname);
    writer.json_keyvalue("osRelease", os_info.release);
    writer.json_keyvalue("osVersion", os_info.version);
    writer.json_keyvalue("osMachine", os_info.machine);
  } else {
    writer.json_keyvalue("osName", "");
    writer.json_keyvalue("osRelease", "");
    writer.json_keyvalue("osVersion", "");
    writer.json_keyvalue("osMachine", "");
  }

  WriteReportCpuInfo(writer);
  if (state == nullptr || !state->exclude_network) {
    WriteReportNetworkInterfaces(writer);
  }

  char host[UV_MAXHOSTNAMESIZE] = {'\0'};
  size_t host_size = sizeof(host);
  if (uv_os_gethostname(host, &host_size) == 0) {
    writer.json_keyvalue("host", std::string(host, host_size));
  } else {
    writer.json_keyvalue("host", "");
  }
  writer.json_keyvalue("glibcVersionRuntime", GetGlibcRuntimeVersion());
  writer.json_keyvalue("glibcVersionCompiler", GetGlibcCompilerVersion());
  writer.json_objectend();

  writer.json_arraystart("nativeStack");
  writer.json_start();
  writer.json_keyvalue("pc", PointerToHexString(reinterpret_cast<uint64_t>(&BuildNativeFatalReportPayload)));
  writer.json_keyvalue("symbol", "edge::BuildNativeFatalReportPayload");
  writer.json_end();
  writer.json_arrayend();

  writer.json_objectstart("javascriptStack");
  writer.json_keyvalue("message", event_message);
  writer.json_arraystart("stack");
  writer.json_arrayend();
  writer.json_objectstart("errorProperties");
  writer.json_objectend();
  writer.json_objectend();

  writer.json_arraystart("libuv");
  writer.json_start();
  writer.json_keyvalue("type", "loop");
  writer.json_keyvalue("address", PointerToHexString(reinterpret_cast<uint64_t>(env)));
  writer.json_keyvalue("is_active", true);
  writer.json_end();
  writer.json_arrayend();

  writer.json_arraystart("sharedObjects");
  writer.json_arrayend();
  WriteReportResourceUsage(writer);
  writer.json_arraystart("workers");
  writer.json_arrayend();
  if (state == nullptr || !state->exclude_env) {
    WriteReportEnvironmentVariables(writer);
  }
  WriteReportUserLimits(writer);
  WriteReportJavascriptHeap(writer, env, state);
  writer.json_end();
  return out.str();
}

[[noreturn]] void FatalErrorReportCallback(napi_env env,
                                           const char* location,
                                           const char* message) {
  const std::string event_message = message != nullptr ? message : "Fatal error";
  if (location != nullptr && location[0] != '\0') {
    std::fprintf(stderr, "FATAL ERROR: %s %s\n", location, event_message.c_str());
  } else {
    std::fprintf(stderr, "FATAL ERROR: %s\n", event_message.c_str());
  }

  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && state->report_on_fatal_error) {
    const std::string filename = ResolveReportFilename(state, "");
    const std::string payload = BuildNativeFatalReportPayload(env, state, event_message, "FatalError", filename);
    (void)WriteReportPayload(state, filename, payload);
  }

  std::fflush(stderr);
  std::abort();
}

[[noreturn]] void OomErrorReportCallback(napi_env env,
                                         const char* location,
                                         bool is_heap_oom,
                                         const char* detail) {
  const char* message = is_heap_oom ? "Allocation failed - JavaScript heap out of memory"
                                    : "Allocation failed - process out of memory";
  if (location != nullptr && location[0] != '\0') {
    std::fprintf(stderr, "FATAL ERROR: %s %s\n", location, message);
  } else {
    std::fprintf(stderr, "FATAL ERROR: %s\n", message);
  }
  if (detail != nullptr && detail[0] != '\0') {
    std::fprintf(stderr, "Reason: %s\n", detail);
  }

  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && state->report_on_fatal_error) {
    const std::string filename = ResolveReportFilename(state, "");
    const std::string payload = BuildNativeFatalReportPayload(env, state, message, "OOMError", filename);
    (void)WriteReportPayload(state, filename, payload);
  }

  std::fflush(stderr);
  std::abort();
}

bool IsValidMacAddress(const std::string& mac) {
  if (mac.size() != 17) return false;
  for (size_t i = 0; i < mac.size(); ++i) {
    if ((i + 1) % 3 == 0) {
      if (mac[i] != ':') return false;
      continue;
    }
    const char ch = mac[i];
    const bool hex_digit =
        (ch >= '0' && ch <= '9') ||
        (ch >= 'a' && ch <= 'f') ||
        (ch >= 'A' && ch <= 'F');
    if (!hex_digit) return false;
  }
  return true;
}

void CopyProcessEnvironmentToObject(napi_env env, napi_value env_obj) {
#if defined(_WIN32)
  char** e = _environ;
#else
  char** e = environ;
#endif
  if (e == nullptr) return;
  for (; *e != nullptr; ++e) {
    const char* entry = *e;
    const char* sep = std::strchr(entry, '=');
    if (sep == nullptr) continue;
    const std::string key(entry, static_cast<size_t>(sep - entry));
    const std::string value(sep + 1);
    napi_value v = nullptr;
    if (napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &v) != napi_ok || v == nullptr) continue;
    napi_set_named_property(env, env_obj, key.c_str(), v);
  }
}

std::optional<std::string> GetSharedProcessEnvironmentValue(const std::string& key) {
  if (key.empty()) return std::nullopt;
#if defined(_WIN32)
  char* value = nullptr;
  size_t length = 0;
  if (_dupenv_s(&value, &length, key.c_str()) != 0 || value == nullptr) {
    if (value != nullptr) free(value);
    return std::nullopt;
  }
  std::string out(value);
  free(value);
  return out;
#else
  const char* value = std::getenv(key.c_str());
  if (value == nullptr) return std::nullopt;
  return std::string(value);
#endif
}

std::vector<std::string> ListSharedProcessEnvironmentKeys() {
  std::vector<std::string> out;
#if defined(_WIN32)
  char** e = _environ;
#else
  char** e = environ;
#endif
  if (e == nullptr) return out;
  for (; *e != nullptr; ++e) {
    const char* entry = *e;
    const char* sep = std::strchr(entry, '=');
    if (sep == nullptr) continue;
    out.emplace_back(entry, static_cast<size_t>(sep - entry));
  }
  return out;
}

bool EnvKeyFromProperty(napi_env env, napi_value property, std::string* key_out, bool* is_symbol_out) {
  if (key_out == nullptr || is_symbol_out == nullptr) return false;
  *key_out = "";
  *is_symbol_out = false;
  if (property == nullptr) return false;

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, property, &type) != napi_ok) return false;
  if (type == napi_symbol) {
    *is_symbol_out = true;
    return true;
  }
  if (type != napi_string) {
    if (napi_coerce_to_string(env, property, &property) != napi_ok || property == nullptr) return false;
  }
  *key_out = NapiValueToUtf8(env, property);
  return true;
}

void ProcessEnvSetVariable(const std::string& key, const std::string& value) {
  if (key.empty()) return;
#if defined(_WIN32)
  _putenv_s(key.c_str(), value.c_str());
  if (key == "TZ") {
    _tzset();
  }
#else
  setenv(key.c_str(), value.c_str(), 1);
  if (key == "TZ") {
    tzset();
  }
#endif
}

void ProcessEnvUnsetVariable(const std::string& key) {
  if (key.empty()) return;
#if defined(_WIN32)
  _putenv_s(key.c_str(), "");
  if (key == "TZ") {
    _tzset();
  }
#else
  unsetenv(key.c_str());
  if (key == "TZ") {
    tzset();
  }
#endif
}

void MaybeNotifyDateTimeConfigurationChange(napi_env env, const std::string& key) {
  if (env == nullptr || key != "TZ") return;
  (void)unofficial_napi_notify_datetime_configuration_change(env);
}

bool ProcessEnvUsesSharedStore(napi_env env) {
  return EdgeWorkerEnvSharesEnvironment(env);
}

bool GetProcessEnvValue(napi_env env, const std::string& key, std::string* out) {
  if (out == nullptr || key.empty()) return false;
  if (ProcessEnvUsesSharedStore(env)) {
    std::optional<std::string> value = GetSharedProcessEnvironmentValue(key);
    if (!value.has_value()) return false;
    *out = std::move(*value);
    return true;
  }
  const std::map<std::string, std::string> entries = EdgeWorkerEnvSnapshotEnvVars(env);
  auto it = entries.find(key);
  if (it == entries.end()) return false;
  *out = it->second;
  return true;
}

std::vector<std::string> GetProcessEnvKeys(napi_env env) {
  if (ProcessEnvUsesSharedStore(env)) return ListSharedProcessEnvironmentKeys();
  const std::map<std::string, std::string> entries = EdgeWorkerEnvSnapshotEnvVars(env);
  std::vector<std::string> keys;
  keys.reserve(entries.size());
  for (const auto& [key, value] : entries) {
    (void)value;
    keys.push_back(key);
  }
  return keys;
}

bool CreateProcessEnvValueString(napi_env env, const std::string& value, napi_value* out) {
  return out != nullptr &&
         napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, out) == napi_ok &&
         *out != nullptr;
}

bool CreateProcessEnvDescriptor(napi_env env, napi_value value, napi_value* out) {
  if (out == nullptr || value == nullptr) return false;
  napi_value descriptor = nullptr;
  if (napi_create_object(env, &descriptor) != napi_ok || descriptor == nullptr) return false;

  napi_value true_value = nullptr;
  if (napi_get_boolean(env, true, &true_value) != napi_ok || true_value == nullptr) return false;
  if (napi_set_named_property(env, descriptor, "value", value) != napi_ok ||
      napi_set_named_property(env, descriptor, "configurable", true_value) != napi_ok ||
      napi_set_named_property(env, descriptor, "enumerable", true_value) != napi_ok ||
      napi_set_named_property(env, descriptor, "writable", true_value) != napi_ok) {
    return false;
  }

  *out = descriptor;
  return true;
}

bool GetObjectOwnPropertyDescriptor(napi_env env,
                                    napi_value target,
                                    napi_value property,
                                    napi_value* out) {
  if (env == nullptr || target == nullptr || property == nullptr || out == nullptr) return false;
  napi_value global = nullptr;
  napi_value object_ctor = nullptr;
  napi_value get_own_property_descriptor = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "Object", &object_ctor) != napi_ok ||
      object_ctor == nullptr ||
      napi_get_named_property(env, object_ctor, "getOwnPropertyDescriptor", &get_own_property_descriptor) != napi_ok ||
      get_own_property_descriptor == nullptr) {
    return false;
  }

  napi_value argv[2] = {target, property};
  return napi_call_function(env, object_ctor, get_own_property_descriptor, 2, argv, out) == napi_ok &&
         *out != nullptr;
}

void CopyEnvSnapshotToObject(napi_env env, napi_value target, const std::map<std::string, std::string>& entries) {
  for (const auto& [key, value] : entries) {
    napi_value key_v = nullptr;
    napi_value value_v = nullptr;
    if (napi_create_string_utf8(env, key.c_str(), NAPI_AUTO_LENGTH, &key_v) != napi_ok || key_v == nullptr ||
        napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &value_v) != napi_ok || value_v == nullptr) {
      continue;
    }
    napi_set_property(env, target, key_v, value_v);
  }
}

bool GetDescriptorBool(napi_env env, napi_value descriptor, const char* name, bool* out) {
  if (out == nullptr) return false;
  *out = false;
  bool has = false;
  if (napi_has_named_property(env, descriptor, name, &has) != napi_ok || !has) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, descriptor, name, &value) != napi_ok || value == nullptr) return false;
  return napi_get_value_bool(env, value, out) == napi_ok;
}

bool DescriptorHasAccessorValue(napi_env env, napi_value descriptor, const char* name) {
  bool has = false;
  if (napi_has_named_property(env, descriptor, name, &has) != napi_ok || !has) return false;
  napi_value value = nullptr;
  if (napi_get_named_property(env, descriptor, name, &value) != napi_ok || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, value, &type) != napi_ok) return false;
  return type != napi_undefined;
}

napi_value ProcessEnvProxyGetTrap(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2 || argv[0] == nullptr) return nullptr;
  std::string key;
  bool is_symbol = false;
  if (!EnvKeyFromProperty(env, argv[1], &key, &is_symbol)) return nullptr;
  if (is_symbol) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  if (key.empty()) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  if (ProcessEnvUsesSharedStore(env)) {
    std::string value;
    if (GetProcessEnvValue(env, key, &value)) {
      napi_value out = nullptr;
      return CreateProcessEnvValueString(env, value, &out) ? out : nullptr;
    }
  }
  napi_value out = nullptr;
  if (napi_get_property(env, argv[0], argv[1], &out) != napi_ok) return nullptr;
  return out;
}

napi_value ProcessEnvProxySetTrap(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 3 || argv[0] == nullptr) return nullptr;

  std::string key;
  bool is_symbol = false;
  if (!EnvKeyFromProperty(env, argv[1], &key, &is_symbol)) return nullptr;
  if (is_symbol) {
    napi_value false_value = nullptr;
    napi_get_boolean(env, false, &false_value);
    return false_value;
  }
  if (key.empty()) {
    bool deleted = false;
    napi_delete_property(env, argv[0], argv[1], &deleted);
    napi_value true_value = nullptr;
    napi_get_boolean(env, true, &true_value);
    return true_value;
  }

  MaybeEmitProcessEnvDeprecationWarning(env, argv[2]);
  napi_value coerced = nullptr;
  if (napi_coerce_to_string(env, argv[2], &coerced) != napi_ok || coerced == nullptr) {
    return nullptr;
  }
  const std::string value = NapiValueToUtf8(env, coerced);

  if (ProcessEnvUsesSharedStore(env)) {
    ProcessEnvSetVariable(key, value);
  } else {
    if (napi_set_property(env, argv[0], argv[1], coerced) != napi_ok) return nullptr;
    EdgeWorkerEnvSetLocalEnvVar(env, key, value);
  }
  MaybeNotifyDateTimeConfigurationChange(env, key);

  napi_value true_value = nullptr;
  napi_get_boolean(env, true, &true_value);
  return true_value;
}

napi_value ProcessEnvProxyHasTrap(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  bool has = false;
  if (argc >= 2 && argv[0] != nullptr && argv[1] != nullptr) {
    std::string key;
    bool is_symbol = false;
    if (EnvKeyFromProperty(env, argv[1], &key, &is_symbol) && !is_symbol && !key.empty()) {
      if (ProcessEnvUsesSharedStore(env)) {
        std::string value;
        has = GetProcessEnvValue(env, key, &value);
        if (!has) napi_has_property(env, argv[0], argv[1], &has);
      } else {
        napi_has_property(env, argv[0], argv[1], &has);
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, has, &out);
  return out;
}

napi_value ProcessEnvProxyDeletePropertyTrap(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc >= 2 && argv[0] != nullptr && argv[1] != nullptr) {
    std::string key;
    bool is_symbol = false;
    if (EnvKeyFromProperty(env, argv[1], &key, &is_symbol) && !is_symbol) {
      if (!ProcessEnvUsesSharedStore(env)) {
        bool deleted = false;
        napi_delete_property(env, argv[0], argv[1], &deleted);
      }
      if (!key.empty()) {
        if (ProcessEnvUsesSharedStore(env)) {
          ProcessEnvUnsetVariable(key);
        } else {
          EdgeWorkerEnvUnsetLocalEnvVar(env, key);
        }
        MaybeNotifyDateTimeConfigurationChange(env, key);
      }
    }
  }
  napi_value out = nullptr;
  napi_get_boolean(env, true, &out);
  return out;
}

napi_value ProcessEnvProxyDefinePropertyTrap(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 3 || argv[0] == nullptr || argv[1] == nullptr || argv[2] == nullptr) return nullptr;

  if (DescriptorHasAccessorValue(env, argv[2], "get") || DescriptorHasAccessorValue(env, argv[2], "set")) {
    napi_throw_type_error(env,
                          "ERR_INVALID_OBJECT_DEFINE_PROPERTY",
                          "'process.env' does not accept an accessor(getter/setter) descriptor");
    return nullptr;
  }

  bool configurable = false;
  bool writable = false;
  bool enumerable = false;
  if (!GetDescriptorBool(env, argv[2], "configurable", &configurable) || !configurable ||
      !GetDescriptorBool(env, argv[2], "writable", &writable) || !writable ||
      !GetDescriptorBool(env, argv[2], "enumerable", &enumerable) || !enumerable) {
    napi_throw_type_error(env,
                          "ERR_INVALID_OBJECT_DEFINE_PROPERTY",
                          "'process.env' only accepts a configurable, writable, and enumerable data descriptor");
    return nullptr;
  }

  std::string key;
  bool is_symbol = false;
  if (!EnvKeyFromProperty(env, argv[1], &key, &is_symbol)) return nullptr;
  if (is_symbol) {
    napi_value false_value = nullptr;
    napi_get_boolean(env, false, &false_value);
    return false_value;
  }
  if (key.empty()) {
    bool deleted = false;
    napi_delete_property(env, argv[0], argv[1], &deleted);
    napi_value true_value = nullptr;
    napi_get_boolean(env, true, &true_value);
    return true_value;
  }

  bool has_value = false;
  napi_has_named_property(env, argv[2], "value", &has_value);
  napi_value value = nullptr;
  if (has_value && napi_get_named_property(env, argv[2], "value", &value) == napi_ok && value != nullptr) {
    MaybeEmitProcessEnvDeprecationWarning(env, value);
    napi_value coerced = nullptr;
    if (napi_coerce_to_string(env, value, &coerced) != napi_ok || coerced == nullptr) return nullptr;
    const std::string text = NapiValueToUtf8(env, coerced);
    if (ProcessEnvUsesSharedStore(env)) {
      ProcessEnvSetVariable(key, text);
    } else {
      if (napi_set_property(env, argv[0], argv[1], coerced) != napi_ok) return nullptr;
      EdgeWorkerEnvSetLocalEnvVar(env, key, text);
    }
    MaybeNotifyDateTimeConfigurationChange(env, key);
  } else {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    if (ProcessEnvUsesSharedStore(env)) {
      ProcessEnvUnsetVariable(key);
    } else {
      if (napi_set_property(env, argv[0], argv[1], undefined) != napi_ok) return nullptr;
      EdgeWorkerEnvUnsetLocalEnvVar(env, key);
    }
    MaybeNotifyDateTimeConfigurationChange(env, key);
  }

  napi_value true_value = nullptr;
  napi_get_boolean(env, true, &true_value);
  return true_value;
}

napi_value ProcessEnvProxyOwnKeysTrap(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;

  if (!ProcessEnvUsesSharedStore(env)) {
    napi_value keys = nullptr;
    return napi_get_property_names(env, argv[0], &keys) == napi_ok ? keys : nullptr;
  }

  const std::vector<std::string> keys = GetProcessEnvKeys(env);
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, keys.size(), &out) != napi_ok || out == nullptr) return nullptr;
  for (uint32_t i = 0; i < keys.size(); ++i) {
    napi_value key = nullptr;
    if (napi_create_string_utf8(env, keys[i].c_str(), NAPI_AUTO_LENGTH, &key) != napi_ok || key == nullptr) {
      continue;
    }
    napi_set_element(env, out, i, key);
  }
  return out;
}

napi_value ProcessEnvProxyGetOwnPropertyDescriptorTrap(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) return nullptr;

  std::string key;
  bool is_symbol = false;
  if (!EnvKeyFromProperty(env, argv[1], &key, &is_symbol)) return nullptr;
  if (!is_symbol && !key.empty()) {
    std::string value;
    if (GetProcessEnvValue(env, key, &value)) {
      napi_value text = nullptr;
      napi_value descriptor = nullptr;
      if (!CreateProcessEnvValueString(env, value, &text) ||
          !CreateProcessEnvDescriptor(env, text, &descriptor)) {
        return nullptr;
      }
      return descriptor;
    }
  }

  napi_value descriptor = nullptr;
  if (GetObjectOwnPropertyDescriptor(env, argv[0], argv[1], &descriptor)) return descriptor;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value CreateProcessEnvObject(napi_env env) {
  napi_value target = nullptr;
  if (napi_create_object(env, &target) != napi_ok || target == nullptr) return nullptr;
  if (!ProcessEnvUsesSharedStore(env)) {
    CopyEnvSnapshotToObject(env, target, EdgeWorkerEnvSnapshotEnvVars(env));
  }

  napi_value handler = nullptr;
  if (napi_create_object(env, &handler) != napi_ok || handler == nullptr) return target;

  auto set_trap = [&](const char* name, napi_callback cb) {
    napi_value fn = nullptr;
    if (napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok && fn != nullptr) {
      napi_set_named_property(env, handler, name, fn);
    }
  };
  set_trap("get", ProcessEnvProxyGetTrap);
  set_trap("set", ProcessEnvProxySetTrap);
  set_trap("has", ProcessEnvProxyHasTrap);
  set_trap("deleteProperty", ProcessEnvProxyDeletePropertyTrap);
  set_trap("defineProperty", ProcessEnvProxyDefinePropertyTrap);
  set_trap("ownKeys", ProcessEnvProxyOwnKeysTrap);
  set_trap("getOwnPropertyDescriptor", ProcessEnvProxyGetOwnPropertyDescriptorTrap);

  napi_value global = nullptr;
  napi_value proxy_ctor = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "Proxy", &proxy_ctor) != napi_ok ||
      proxy_ctor == nullptr) {
    return target;
  }
  napi_value argv[2] = {target, handler};
  napi_value proxy = nullptr;
  if (napi_new_instance(env, proxy_ctor, 2, argv, &proxy) != napi_ok || proxy == nullptr) {
    return target;
  }
  return proxy;
}

napi_value ProcessCwdCallback(napi_env env, napi_callback_info info) {
  std::string cwd;
  int uv_error = 0;
  if (!TryGetCurrentWorkingDirectoryString(&cwd, &uv_error)) {
    ThrowUvCwdError(env, uv_error);
    return nullptr;
  }
  napi_value result = nullptr;
  if (napi_create_string_utf8(env, cwd.c_str(), cwd.size(), &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessChdirCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value args[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, args, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc < 1 || args[0] == nullptr) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "The \"directory\" argument must be of type string");
    return nullptr;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, args[0], &type) != napi_ok || type != napi_string) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "The \"directory\" argument must be of type string");
    return nullptr;
  }
  size_t len = 0;
  if (napi_get_value_string_utf8(env, args[0], nullptr, 0, &len) != napi_ok) return nullptr;
  std::vector<char> buf(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, args[0], buf.data(), buf.size(), &copied) != napi_ok) return nullptr;
  const std::string dest(buf.data(), copied);
  const std::string oldcwd = GetCurrentWorkingDirectoryForErrors();
  const int rc = uv_chdir(dest.c_str());
  if (rc != 0) {
    const char* code = uv_err_name(rc);
    if (code == nullptr) code = "UNKNOWN";
    const char* detail = uv_strerror(rc);
    if (detail == nullptr) detail = "unknown error";
    std::string msg = std::string(code) + ": " + detail + ", chdir " + oldcwd + " -> '" + dest + "'";
    napi_value code_value = nullptr;
    napi_value message_value = nullptr;
    napi_value error_value = nullptr;
    if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
        napi_create_string_utf8(env, msg.c_str(), NAPI_AUTO_LENGTH, &message_value) != napi_ok ||
        !CreateJsErrorObject(env, message_value, &error_value) ||
        error_value == nullptr) {
      return nullptr;
    }
    SetNamedInt32(env, error_value, "errno", rc);
    napi_set_named_property(env, error_value, "code", code_value);
    SetNamedString(env, error_value, "syscall", "chdir");
    SetNamedString(env, error_value, "path", oldcwd);
    SetNamedString(env, error_value, "dest", dest);
    napi_throw(env, error_value);
    return nullptr;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessCpuUsageCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value args[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
  uv_rusage_t rusage;
  const int rc = uv_getrusage(&rusage);
  if (rc != 0) {
    ThrowSystemError(env, rc, "uv_getrusage");
    return nullptr;
  }
  uint64_t user = static_cast<uint64_t>(kMicrosPerSec * rusage.ru_utime.tv_sec + rusage.ru_utime.tv_usec);
  uint64_t system = static_cast<uint64_t>(kMicrosPerSec * rusage.ru_stime.tv_sec + rusage.ru_stime.tv_usec);
  if (argc >= 1 && args[0] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, args[0], &t) != napi_ok || t != napi_object) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE",
                             "The \"prevValue\" argument must be of type object. Received type number (1)");
      return nullptr;
    }
    bool has_user = false;
    bool has_system = false;
    napi_has_named_property(env, args[0], "user", &has_user);
    napi_has_named_property(env, args[0], "system", &has_system);
    if (!has_user) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE",
                             "The \"prevValue.user\" property must be of type number. Received undefined");
      return nullptr;
    }
    napi_value user_v = nullptr;
    napi_value sys_v = nullptr;
    napi_get_named_property(env, args[0], "user", &user_v);
    double prev_user = 0;
    double prev_sys = 0;
    if (napi_get_value_double(env, user_v, &prev_user) != napi_ok) {
      const std::string msg = "The \"prevValue.user\" property must be of type number." + InvalidArgTypeSuffix(env, user_v);
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", msg.c_str());
      return nullptr;
    }
    if (!has_system) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE",
                             "The \"prevValue.system\" property must be of type number. Received undefined");
      return nullptr;
    }
    napi_get_named_property(env, args[0], "system", &sys_v);
    if (napi_get_value_double(env, sys_v, &prev_sys) != napi_ok) {
      const std::string msg = "The \"prevValue.system\" property must be of type number." + InvalidArgTypeSuffix(env, sys_v);
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", msg.c_str());
      return nullptr;
    }
    if (prev_user < 0 || std::isinf(prev_user)) {
      const std::string msg = "The property 'prevValue.user' is invalid. Received " + FormatNodeNumber(prev_user);
      ThrowRangeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", msg.c_str());
      return nullptr;
    }
    if (prev_sys < 0 || std::isinf(prev_sys)) {
      const std::string msg = "The property 'prevValue.system' is invalid. Received " + FormatNodeNumber(prev_sys);
      ThrowRangeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", msg.c_str());
      return nullptr;
    }
    const uint64_t pu = static_cast<uint64_t>(prev_user);
    const uint64_t ps = static_cast<uint64_t>(prev_sys);
    user = user > pu ? user - pu : 0;
    system = system > ps ? system - ps : 0;
  }
  napi_value obj = nullptr;
  napi_create_object(env, &obj);
  napi_value user_v = nullptr;
  napi_value sys_v = nullptr;
  napi_create_double(env, static_cast<double>(user), &user_v);
  napi_create_double(env, static_cast<double>(system), &sys_v);
  napi_set_named_property(env, obj, "user", user_v);
  napi_set_named_property(env, obj, "system", sys_v);
  return obj;
}

napi_value ProcessAvailableMemoryCallback(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_create_double(env, static_cast<double>(uv_get_available_memory()), &out);
  return out;
}

napi_value ProcessConstrainedMemoryCallback(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_create_double(env, static_cast<double>(uv_get_constrained_memory()), &out);
  return out;
}

napi_value ProcessUmaskCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value args[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, args, nullptr, nullptr) != napi_ok) return nullptr;
  uint32_t old_mask = 0;
  {
    std::lock_guard<std::mutex> lock(g_process_umask_mutex);
    old_mask = umask(0);
    umask(static_cast<mode_t>(old_mask));
  }
  if (argc >= 1 && args[0] != nullptr) {
    napi_valuetype arg_type = napi_undefined;
    if (napi_typeof(env, args[0], &arg_type) != napi_ok) return nullptr;
    if (arg_type == napi_undefined) {
      napi_value result = nullptr;
      if (napi_create_uint32(env, old_mask, &result) != napi_ok) return nullptr;
      return result;
    }
    uint32_t new_mask = 0;
    if (arg_type == napi_string) {
      size_t len = 0;
      if (napi_get_value_string_utf8(env, args[0], nullptr, 0, &len) != napi_ok) return nullptr;
      std::vector<char> buf(len + 1, '\0');
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, args[0], buf.data(), buf.size(), &copied) != napi_ok) return nullptr;
      const std::string value(buf.data(), copied);
      if (value.empty()) {
        ThrowErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "The argument 'mask' is invalid.");
        return nullptr;
      }
      for (char ch : value) {
        if (ch < '0' || ch > '7') {
          ThrowErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "The argument 'mask' is invalid.");
          return nullptr;
        }
      }
      try {
        new_mask = static_cast<uint32_t>(std::stoul(value, nullptr, 8)) & 0777u;
      } catch (...) {
        ThrowErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "The argument 'mask' is invalid.");
        return nullptr;
      }
    } else if (arg_type == napi_number) {
      double num = 0;
      if (napi_get_value_double(env, args[0], &num) != napi_ok) return nullptr;
      if (!(num >= 0) || num != num) {
        ThrowErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "The argument 'mask' is invalid.");
        return nullptr;
      }
      new_mask = static_cast<uint32_t>(num) & 0777u;
    } else {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "The \"mask\" argument must be of type number or string.");
      return nullptr;
    }
    std::lock_guard<std::mutex> lock(g_process_umask_mutex);
    old_mask = umask(static_cast<mode_t>(new_mask));
  }
  napi_value result = nullptr;
  if (napi_create_uint32(env, old_mask, &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessUptimeCallback(napi_env env, napi_callback_info info) {
  const double seconds = static_cast<double>(GetHrtimeNanoseconds() - g_process_start_time_ns) / kNanosPerSec;
  napi_value result = nullptr;
  if (napi_create_double(env, seconds, &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessHrtimeBigintCallback(napi_env env, napi_callback_info info) {
  napi_value result = nullptr;
  if (napi_create_bigint_uint64(env, GetHrtimeNanoseconds(), &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessHrtimeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value args[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, args, nullptr, nullptr) != napi_ok) return nullptr;
  const uint64_t now_ns = GetHrtimeNanoseconds();
  uint64_t out_ns = now_ns;
  if (argc == 1 && args[0] != nullptr) {
    bool is_array = false;
    if (napi_is_array(env, args[0], &is_array) != napi_ok || !is_array) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE",
                             "The \"time\" argument must be an instance of Array. Received type number (1)");
      return nullptr;
    }
    uint32_t len = 0;
    if (napi_get_array_length(env, args[0], &len) != napi_ok || len != 2) {
      napi_value code = nullptr;
      napi_value message = nullptr;
      napi_value err = nullptr;
      napi_create_string_utf8(env, "ERR_OUT_OF_RANGE", NAPI_AUTO_LENGTH, &code);
      std::string msg = "The value of \"time\" is out of range. It must be 2. Received " + std::to_string(len);
      napi_create_string_utf8(env, msg.c_str(), NAPI_AUTO_LENGTH, &message);
      napi_create_range_error(env, code, message, &err);
      napi_set_named_property(env, err, "code", code);
      napi_throw(env, err);
      return nullptr;
    }
    napi_value sec_val = nullptr;
    napi_value nsec_val = nullptr;
    double sec = 0;
    double nsec = 0;
    if (napi_get_element(env, args[0], 0, &sec_val) != napi_ok || napi_get_element(env, args[0], 1, &nsec_val) != napi_ok ||
        napi_get_value_double(env, sec_val, &sec) != napi_ok || napi_get_value_double(env, nsec_val, &nsec) != napi_ok) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "Invalid hrtime tuple values.");
      return nullptr;
    }
    const double base_ns = sec * 1e9 + nsec;
    out_ns = base_ns > static_cast<double>(now_ns) ? 0 : now_ns - static_cast<uint64_t>(base_ns);
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 2, &out) != napi_ok || out == nullptr) return nullptr;
  napi_value seconds = nullptr;
  napi_value nanoseconds = nullptr;
  if (napi_create_uint32(env, static_cast<uint32_t>(out_ns / 1000000000ull), &seconds) != napi_ok ||
      napi_create_uint32(env, static_cast<uint32_t>(out_ns % 1000000000ull), &nanoseconds) != napi_ok) {
    return nullptr;
  }
  napi_set_element(env, out, 0, seconds);
  napi_set_element(env, out, 1, nanoseconds);
  return out;
}

napi_value ProcessObjectTitleGetter(napi_env env, napi_callback_info info) {
  napi_value result = nullptr;
  std::string title;
  if (EdgeWorkerEnvOwnsProcessState(env)) {
    title = GetProcessTitleString();
  } else {
    title = EdgeWorkerEnvGetProcessTitle(env);
    if (title.empty()) title = GetProcessTitleString();
  }
  if (napi_create_string_utf8(env, title.c_str(), NAPI_AUTO_LENGTH, &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessObjectTitleSetter(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  std::string title = NapiValueToUtf8(env, argv[0]);
  if (title.empty()) title = "node";
  if (EdgeWorkerEnvOwnsProcessState(env)) {
    g_process_title = title;
    (void)uv_set_process_title(g_process_title.c_str());
  } else {
    EdgeWorkerEnvSetProcessTitle(env, title);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessObjectDebugPortGetter(napi_env env, napi_callback_info info) {
  napi_value result = nullptr;
  const uint32_t port = EdgeWorkerEnvOwnsProcessState(env) ? g_process_debug_port : EdgeWorkerEnvGetDebugPort(env);
  if (napi_create_uint32(env, port, &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessObjectDebugPortSetter(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  napi_value coerced = nullptr;
  double port = 0;
  if (napi_coerce_to_number(env, argv[0], &coerced) != napi_ok ||
      napi_get_value_double(env, coerced, &port) != napi_ok) {
    port = 0;
  }
  const int32_t port_i32 = static_cast<int32_t>(port);
  if ((port_i32 != 0 && port_i32 < 1024) || port_i32 > 65535) {
    ThrowRangeErrorWithCode(env, "ERR_OUT_OF_RANGE", "process.debugPort must be 0 or in range 1024 to 65535");
    return nullptr;
  }
  if (EdgeWorkerEnvOwnsProcessState(env)) {
    g_process_debug_port = static_cast<uint32_t>(port_i32);
  } else {
    EdgeWorkerEnvSetDebugPort(env, static_cast<uint32_t>(port_i32));
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessObjectPpidGetter(napi_env env, napi_callback_info info) {
  napi_value result = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(uv_os_getppid()), &result) != napi_ok) return nullptr;
  return result;
}

napi_value ProcessExitCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value args[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, args, nullptr, nullptr) != napi_ok) return nullptr;
  int32_t exit_code = 0;
  if (argc > 0 && args[0] != nullptr) {
    napi_valuetype arg_type = napi_undefined;
    if (napi_typeof(env, args[0], &arg_type) == napi_ok && arg_type != napi_undefined) napi_get_value_int32(env, args[0], &exit_code);
  }
  if (EdgeExecArgvHasFlag("--trace-exit")) {
    std::ostringstream warning;
    warning << "(node:" << uv_os_getpid();
    if (!EdgeWorkerEnvOwnsProcessState(env)) {
      warning << ", thread:" << EdgeWorkerEnvThreadId(env);
    }
    warning << ") WARNING: Exited the environment with code " << exit_code << "\n";

    napi_value global = nullptr;
    napi_value error_ctor = nullptr;
    napi_value error_obj = nullptr;
    napi_value stack_value = nullptr;
    std::string stack;
    if (napi_get_global(env, &global) == napi_ok &&
        global != nullptr &&
        napi_get_named_property(env, global, "Error", &error_ctor) == napi_ok &&
        error_ctor != nullptr &&
        napi_new_instance(env, error_ctor, 0, nullptr, &error_obj) == napi_ok &&
        error_obj != nullptr &&
        napi_get_named_property(env, error_obj, "stack", &stack_value) == napi_ok &&
        stack_value != nullptr) {
      stack = NapiValueToUtf8(env, stack_value);
      const size_t first_newline = stack.find('\n');
      if (first_newline != std::string::npos) {
        stack.erase(0, first_newline + 1);
      } else {
        stack.clear();
      }
    }

    std::cerr << warning.str();
    if (!stack.empty()) {
      std::cerr << stack;
      if (stack.back() != '\n') std::cerr << "\n";
    }
    std::cerr.flush();
  }
  // process.reallyExit() is reached from JS after process.exit() already marked
  // the process as exiting. When called directly, make that state explicit so
  // the embedder loop will not emit a second exit event while unwinding.
  EdgePrepareProcessExit(env, exit_code);
  EdgeEnvironmentRunAtExitCallbacks(env);
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->Exit(exit_code);
    return nullptr;
  }
  uv_loop_t* loop = EdgeGetExistingEnvLoop(env);
  if (loop != nullptr) uv_stop(loop);
  (void)unofficial_napi_terminate_execution(env);
  return nullptr;
}

napi_value ProcessAbortCallback(napi_env env, napi_callback_info info) {
  napi_value new_target = nullptr;
  if (napi_get_new_target(env, info, &new_target) == napi_ok && new_target != nullptr) {
    napi_throw_type_error(env, nullptr, "process.abort is not a constructor");
    return nullptr;
  }
  std::abort();
  return nullptr;
}

napi_value ProcessMethodsCauseSegfaultCallback(napi_env env, napi_callback_info info) {
#if defined(_WIN32)
  std::abort();
#else
  raise(SIGSEGV);
  std::abort();
#endif
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsKillCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;
  int32_t pid = 0;
  if (!ValueToInt32(env, argv[0], &pid)) return nullptr;
  int32_t signal = 0;
  if (argc >= 2 && argv[1] != nullptr) ValueToInt32(env, argv[1], &signal);
  int rc = uv_kill(pid, signal);
  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out;
}

napi_value ProcessMethodsSetEnvCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t rc = -1;
  if (argc >= 2 && argv[0] != nullptr && argv[1] != nullptr) {
    const std::string key = NapiValueToUtf8(env, argv[0]);
    const std::string value = NapiValueToUtf8(env, argv[1]);
    if (!key.empty()) {
      if (ProcessEnvUsesSharedStore(env)) {
#if defined(_WIN32)
        rc = (_putenv_s(key.c_str(), value.c_str()) == 0) ? 0 : -1;
#else
        rc = (setenv(key.c_str(), value.c_str(), 1) == 0) ? 0 : -1;
#endif
      } else {
        EdgeWorkerEnvSetLocalEnvVar(env, key, value);
        rc = 0;
      }
      if (rc == 0 && key == "TZ") {
#if defined(_WIN32)
        _tzset();
#else
        tzset();
#endif
        (void)unofficial_napi_notify_datetime_configuration_change(env);
      }
    }
  }

  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out;
}

napi_value ProcessMethodsUnsetEnvCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);

  int32_t rc = -1;
  if (argc >= 1 && argv[0] != nullptr) {
    const std::string key = NapiValueToUtf8(env, argv[0]);
    if (!key.empty()) {
      if (ProcessEnvUsesSharedStore(env)) {
#if defined(_WIN32)
        rc = (_putenv_s(key.c_str(), "") == 0) ? 0 : -1;
#else
        rc = (unsetenv(key.c_str()) == 0) ? 0 : -1;
#endif
      } else {
        EdgeWorkerEnvUnsetLocalEnvVar(env, key);
        rc = 0;
      }
      if (rc == 0 && key == "TZ") {
#if defined(_WIN32)
        _tzset();
#else
        tzset();
#endif
        (void)unofficial_napi_notify_datetime_configuration_change(env);
      }
    }
  }

  napi_value out = nullptr;
  napi_create_int32(env, rc, &out);
  return out;
}

napi_value ProcessMethodsRawDebugCallback(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  std::ostringstream oss;
  for (size_t i = 0; i < argc; ++i) {
    if (i > 0) oss << " ";
    oss << NapiValueToUtf8(env, argv[i]);
  }
  std::cerr << oss.str() << std::endl;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsDebugProcessCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    ThrowTypeErrorWithCode(env, "ERR_MISSING_ARGS", "Invalid number of arguments.");
    return nullptr;
  }
  int32_t pid = 0;
  if (!ValueToInt32(env, argv[0], &pid)) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "The \"pid\" argument must be of type number.");
    return nullptr;
  }

#if defined(_WIN32)
  ThrowErrorWithCode(
      env, "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM", "process._debugProcess is not supported on win32 in Edge runtime");
  return nullptr;
#else
  const int rc = uv_kill(pid, SIGUSR1);
  if (rc != 0) {
    ThrowSystemError(env, rc, "kill");
    return nullptr;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
#endif
}

napi_value ProcessMethodsExecveCallback(napi_env env, napi_callback_info info) {
#if defined(_WIN32) || defined(__PASE__)
  ThrowErrorWithCode(env,
                     "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
                     "process.execve is not available on this platform.");
  return nullptr;
#else
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 3 ||
      argv[0] == nullptr || argv[1] == nullptr || argv[2] == nullptr) {
    ThrowTypeErrorWithCode(env, "ERR_MISSING_ARGS", "Invalid number of arguments.");
    return nullptr;
  }

  napi_valuetype executable_type = napi_undefined;
  bool argv_is_array = false;
  bool envp_is_array = false;
  if (napi_typeof(env, argv[0], &executable_type) != napi_ok || executable_type != napi_string ||
      napi_is_array(env, argv[1], &argv_is_array) != napi_ok || !argv_is_array ||
      napi_is_array(env, argv[2], &envp_is_array) != napi_ok || !envp_is_array) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "process.execve expects (string, string[], string[]).");
    return nullptr;
  }

  const std::string executable = NapiValueToUtf8(env, argv[0]);
  if (executable.empty()) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "The \"execPath\" argument must be a non-empty string.");
    return nullptr;
  }

  uint32_t argv_len = 0;
  uint32_t envp_len = 0;
  if (napi_get_array_length(env, argv[1], &argv_len) != napi_ok ||
      napi_get_array_length(env, argv[2], &envp_len) != napi_ok) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Failed to read process.execve arguments.");
    return nullptr;
  }

  std::vector<std::string> argv_storage(argv_len);
  std::vector<char*> argv_exec(argv_len + 1, nullptr);
  for (uint32_t i = 0; i < argv_len; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, argv[1], i, &item) != napi_ok || item == nullptr) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Failed to deserialize argument.");
      return nullptr;
    }
    argv_storage[i] = NapiValueToUtf8(env, item);
    argv_exec[i] = argv_storage[i].data();
  }
  argv_exec[argv_len] = nullptr;

  std::vector<std::string> envp_storage(envp_len);
  std::vector<char*> envp_exec(envp_len + 1, nullptr);
  for (uint32_t i = 0; i < envp_len; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, argv[2], i, &item) != napi_ok || item == nullptr) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_VALUE", "Failed to deserialize environment variable.");
      return nullptr;
    }
    envp_storage[i] = NapiValueToUtf8(env, item);
    envp_exec[i] = envp_storage[i].data();
  }
  envp_exec[envp_len] = nullptr;

  auto persist_standard_stream = [](int fd) -> int {
    const int flags = fcntl(fd, F_GETFD, 0);
    if (flags < 0) return -1;
    return fcntl(fd, F_SETFD, flags & ~FD_CLOEXEC);
  };

  if (persist_standard_stream(0) < 0 || persist_standard_stream(1) < 0 || persist_standard_stream(2) < 0) {
    ThrowSystemError(env, uv_translate_sys_error(errno), "fcntl");
    return nullptr;
  }

  execve(executable.c_str(), argv_exec.data(), envp_exec.data());
  const int execve_errno = errno;
  int uv_execve_errno = uv_translate_sys_error(execve_errno);
  if (uv_execve_errno == 0) uv_execve_errno = UV_EIO;
  const char* execve_code = uv_err_name(uv_execve_errno);
  if (execve_code == nullptr) execve_code = "UNKNOWN";

  ThrowSystemError(env, uv_execve_errno, "execve", executable);
  std::string stack_message;
  bool has_pending_exception = false;
  if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
    napi_value exception = nullptr;
    if (napi_get_and_clear_last_exception(env, &exception) == napi_ok && exception != nullptr) {
      napi_value stack_value = nullptr;
      if (napi_get_named_property(env, exception, "stack", &stack_value) == napi_ok &&
          stack_value != nullptr) {
        stack_message = NapiValueToUtf8(env, stack_value);
      }
      if (stack_message.empty()) {
        stack_message = NapiValueToUtf8(env, exception);
      }
    }
  }

  std::cerr << "process.execve failed with error code " << execve_code << "\n";
  if (!stack_message.empty()) {
    std::cerr << stack_message << "\n";
  }
  std::abort();
#endif
}

napi_value ProcessMethodsPatchProcessObjectCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_valuetype arg_type = napi_undefined;
  if (napi_typeof(env, argv[0], &arg_type) != napi_ok || arg_type != napi_object) {
    ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "The \"process\" argument must be of type object.");
    return nullptr;
  }

  napi_value global = nullptr;
  napi_value process_obj = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value value = nullptr;
  if (napi_get_named_property(env, process_obj, "argv", &value) == napi_ok && value != nullptr) {
    SetOwnPropertyValue(env, argv[0], "argv", value);
  }
  if (napi_get_named_property(env, process_obj, "execArgv", &value) == napi_ok && value != nullptr) {
    SetOwnPropertyValue(env, argv[0], "execArgv", value);
  }
  if (napi_get_named_property(env, process_obj, "execPath", &value) == napi_ok && value != nullptr) {
    SetOwnPropertyValue(env, argv[0], "execPath", value);
  }
  if (napi_get_named_property(env, process_obj, "versions", &value) == napi_ok && value != nullptr) {
    SetOwnPropertyValue(env, argv[0], "versions", value);
  }
  napi_value pid_value = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(uv_os_getpid()), &pid_value) == napi_ok && pid_value != nullptr) {
    SetOwnPropertyValue(env, argv[0], "pid", pid_value);
  }

  napi_property_descriptor descriptors[3] = {};
  descriptors[0].utf8name = "title";
  descriptors[0].getter = ProcessObjectTitleGetter;
  descriptors[0].setter = ProcessObjectTitleSetter;
  descriptors[1].utf8name = "ppid";
  descriptors[1].getter = ProcessObjectPpidGetter;
  descriptors[2].utf8name = "debugPort";
  descriptors[2].getter = ProcessObjectDebugPortGetter;
  descriptors[2].setter = ProcessObjectDebugPortSetter;
  napi_define_properties(env, argv[0], 3, descriptors);

  // Node's process object has a custom constructor on its prototype where
  // constructor.prototype points back to that same process prototype.
  napi_value process_proto = nullptr;
  if (napi_get_prototype(env, argv[0], &process_proto) == napi_ok && process_proto != nullptr) {
    napi_value constructor_key = nullptr;
    bool has_own_constructor = false;
    if (napi_create_string_utf8(env, "constructor", NAPI_AUTO_LENGTH, &constructor_key) == napi_ok &&
        constructor_key != nullptr &&
        napi_has_own_property(env, process_proto, constructor_key, &has_own_constructor) == napi_ok &&
        has_own_constructor) {
      napi_value ctor = nullptr;
      napi_valuetype ctor_type = napi_undefined;
      if (napi_get_named_property(env, process_proto, "constructor", &ctor) == napi_ok && ctor != nullptr &&
          napi_typeof(env, ctor, &ctor_type) == napi_ok && ctor_type == napi_function) {
        napi_set_named_property(env, ctor, "prototype", process_proto);
      }
    }
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsLoadEnvFileCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  const bool has_path_arg = argc >= 1 && argv[0] != nullptr;
  const std::string path = has_path_arg ? NapiValueToUtf8(env, argv[0]) : ".env";

  std::ifstream input(path, std::ios::in | std::ios::binary);
  if (!input.is_open()) {
    int err = uv_translate_sys_error(errno);
    if (err == 0) err = UV_ENOENT;
    ThrowSystemError(env, err, "open", path);
    return nullptr;
  }
  std::ostringstream buffer;
  buffer << input.rdbuf();
  const std::string content = buffer.str();

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value util_binding = edge::binding_registry::Get(env, "util");

  if (util_binding == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_valuetype util_binding_type = napi_undefined;
  if (napi_typeof(env, util_binding, &util_binding_type) != napi_ok || util_binding_type != napi_object) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value parse_env = nullptr;
  if (napi_get_named_property(env, util_binding, "parseEnv", &parse_env) != napi_ok || parse_env == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_valuetype parse_type = napi_undefined;
  if (napi_typeof(env, parse_env, &parse_type) != napi_ok || parse_type != napi_function) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value content_value = nullptr;
  if (napi_create_string_utf8(env, content.c_str(), content.size(), &content_value) != napi_ok ||
      content_value == nullptr) {
    return nullptr;
  }

  napi_value parsed = nullptr;
  if (napi_call_function(env, util_binding, parse_env, 1, &content_value, &parsed) != napi_ok || parsed == nullptr) {
    return nullptr;
  }

  napi_valuetype parsed_type = napi_undefined;
  if (napi_typeof(env, parsed, &parsed_type) != napi_ok || parsed_type != napi_object) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value process_obj = nullptr;
  napi_value process_env = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr ||
      napi_get_named_property(env, process_obj, "env", &process_env) != napi_ok || process_env == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  napi_value keys = nullptr;
  if (napi_get_property_names(env, parsed, &keys) != napi_ok || keys == nullptr) return nullptr;
  uint32_t key_count = 0;
  if (napi_get_array_length(env, keys, &key_count) != napi_ok) return nullptr;

  for (uint32_t i = 0; i < key_count; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;

    bool has_existing = false;
    if (napi_has_property(env, process_env, key, &has_existing) != napi_ok || has_existing) continue;

    napi_value value = nullptr;
    if (napi_get_property(env, parsed, key, &value) != napi_ok || value == nullptr) continue;
    napi_set_property(env, process_env, key, value);
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsNoopUndefinedCallback(napi_env env, napi_callback_info info) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsGetActiveRequestsCallback(napi_env env, napi_callback_info info) {
  napi_value out = EdgeGetActiveRequestsArray(env);
  if (out != nullptr) return out;
  napi_create_array(env, &out);
  return out;
}

napi_value ProcessMethodsGetActiveHandlesCallback(napi_env env, napi_callback_info info) {
  napi_value out = EdgeGetActiveHandlesArray(env);
  if (out != nullptr) return out;
  napi_create_array(env, &out);
  return out;
}

napi_value ProcessMethodsGetActiveResourcesInfoCallback(napi_env env, napi_callback_info info) {
  napi_value out = EdgeGetActiveResourcesInfoArray(env);
  bool is_array = false;
  if (out == nullptr || napi_is_array(env, out, &is_array) != napi_ok || !is_array) {
    napi_create_array(env, &out);
  }
  if (out == nullptr) return nullptr;

  uint32_t length = 0;
  napi_get_array_length(env, out, &length);

  const int32_t timeout_count = EdgeGetActiveTimeoutCount(env);
  if (timeout_count > 0) {
    napi_value timeout_name = nullptr;
    if (napi_create_string_utf8(env, "Timeout", NAPI_AUTO_LENGTH, &timeout_name) == napi_ok &&
        timeout_name != nullptr) {
      for (int32_t i = 0; i < timeout_count; ++i) {
        napi_set_element(env, out, length++, timeout_name);
      }
    }
  }

  const uint32_t immediate_count = EdgeGetActiveImmediateRefCount(env);
  if (immediate_count > 0) {
    napi_value immediate_name = nullptr;
    if (napi_create_string_utf8(env, "Immediate", NAPI_AUTO_LENGTH, &immediate_name) == napi_ok &&
        immediate_name != nullptr) {
      for (uint32_t i = 0; i < immediate_count; ++i) {
        napi_set_element(env, out, length++, immediate_name);
      }
    }
  }

  return out;
}

napi_value ProcessMethodsDlopenCallback(napi_env env, napi_callback_info info) {
  size_t argc = 3;
  napi_value argv[3] = {nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) {
    ThrowErrorWithCode(env, "ERR_MISSING_ARGS", "process.dlopen needs at least 2 arguments");
    return nullptr;
  }

  std::string filename = "(unknown)";
  const std::string maybe_name = NapiValueToUtf8(env, argv[1]);
  if (!maybe_name.empty()) filename = maybe_name;

  int32_t flags = kDefaultDlopenFlags;
  if (argc > 2 && argv[2] != nullptr) {
    if (napi_get_value_int32(env, argv[2], &flags) != napi_ok) {
      ThrowTypeErrorWithCode(env, "ERR_INVALID_ARG_TYPE", "flag argument must be an integer.");
      return nullptr;
    }
  }
  const std::string cache_key = BuildDlopenCacheKey(filename, flags);

  napi_value module = nullptr;
  if (napi_coerce_to_object(env, argv[0], &module) != napi_ok || module == nullptr) {
    return nullptr;
  }

  napi_value exports_value = nullptr;
  if (napi_get_named_property(env, module, "exports", &exports_value) != napi_ok || exports_value == nullptr) {
    return nullptr;
  }

  napi_value exports = nullptr;
  if (napi_coerce_to_object(env, exports_value, &exports) != napi_ok || exports == nullptr) {
    return nullptr;
  }

  napi_addon_register_func init = nullptr;
  uv_lib_t* lib = nullptr;
  std::unique_ptr<uv_lib_t> newly_loaded;
  bool cache_loaded_library = false;
  {
    std::lock_guard<std::mutex> lock(g_process_dlopen_mutex);
    auto it = g_process_dlopen_handles.find(cache_key);
    if (it != g_process_dlopen_handles.end()) {
      lib = it->second.get();
    }
  }

  if (lib == nullptr) {
    newly_loaded = std::make_unique<uv_lib_t>();
    std::string message;
    if (OpenDynamicLibrary(filename, flags, newly_loaded.get(), &message) != 0) {
      ThrowErrorWithCode(env, "ERR_DLOPEN_FAILED", message.c_str());
      return nullptr;
    }
    lib = newly_loaded.get();
    cache_loaded_library = true;
  }

  init = GetNapiInitializerCallback(lib);

  if (init == nullptr) {
    const std::string message = "Module did not self-register: '" + filename + "'.";
    if (cache_loaded_library && newly_loaded != nullptr) {
      CloseDynamicLibrary(newly_loaded.get());
    }
    ThrowErrorWithCode(env, "ERR_DLOPEN_FAILED", message.c_str());
    return nullptr;
  }

  napi_value addon_exports = init(env, exports);

  bool has_pending = false;
  if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
    return nullptr;
  }

  bool same_exports = false;
  if (addon_exports != nullptr &&
      (napi_strict_equals(env, addon_exports, exports, &same_exports) != napi_ok || !same_exports)) {
    napi_set_named_property(env, module, "exports", addon_exports);
  }

  if (cache_loaded_library && newly_loaded != nullptr) {
    std::lock_guard<std::mutex> lock(g_process_dlopen_mutex);
    g_process_dlopen_handles.emplace(cache_key, std::move(newly_loaded));
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsEmptyArrayCallback(napi_env env, napi_callback_info info) {
  napi_value out = nullptr;
  napi_create_array(env, &out);
  return out;
}

static size_t get_rss() {
  size_t rss = 0;
  if (uv_resident_set_memory(&rss) != 0) return 0;
  return rss;
}

napi_value ProcessMethodsRssCallback(napi_env env, napi_callback_info info) {
  size_t rss = 0;
  const int rc = uv_resident_set_memory(&rss);
  if (rc != 0) {
    ThrowSystemError(env, rc, "uv_resident_set_memory");
    return nullptr;
  }
  napi_value out = nullptr;
  napi_create_double(env, static_cast<double>(rss), &out);
  return out;
}

napi_value ProcessMethodsCpuUsageBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;
  double* values = nullptr;
  if (!GetFloat64ArrayData(env, argv[0], 2, &values)) return nullptr;
  uv_rusage_t rusage;
  const int rc = uv_getrusage(&rusage);
  if (rc != 0) {
    ThrowSystemError(env, rc, "uv_getrusage");
    return nullptr;
  }
  values[0] = kMicrosPerSec * rusage.ru_utime.tv_sec + rusage.ru_utime.tv_usec;
  values[1] = kMicrosPerSec * rusage.ru_stime.tv_sec + rusage.ru_stime.tv_usec;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsThreadCpuUsageBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;
  double* values = nullptr;
  if (!GetFloat64ArrayData(env, argv[0], 2, &values)) return nullptr;
  uv_rusage_t rusage;
  const int rc = uv_getrusage_thread(&rusage);
  if (rc != 0) {
#if defined(__sun)
    ThrowErrorWithCode(env, "ERR_OPERATION_FAILED", "Operation failed: threadCpuUsage is not available on SunOS");
#else
    ThrowSystemError(env, rc, "uv_getrusage_thread");
#endif
    return nullptr;
  }
  values[0] = kMicrosPerSec * rusage.ru_utime.tv_sec + rusage.ru_utime.tv_usec;
  values[1] = kMicrosPerSec * rusage.ru_stime.tv_sec + rusage.ru_stime.tv_usec;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsMemoryUsageBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;
  double* values = nullptr;
  if (!GetFloat64ArrayData(env, argv[0], 5, &values)) return nullptr;
  size_t rss = 0;
  const int rss_rc = uv_resident_set_memory(&rss);
  if (rss_rc != 0) {
    ThrowSystemError(env, rss_rc, "uv_resident_set_memory");
    return nullptr;
  }
  double heap_total = 0;
  double heap_used = 0;
  double external = 0;
  double array_buffers = 0;
  const napi_status memory_status = unofficial_napi_get_process_memory_info(
      env, &heap_total, &heap_used, &external, &array_buffers);
  if (memory_status != napi_ok) return nullptr;
  values[0] = static_cast<double>(rss);
  values[1] = heap_total;
  values[2] = heap_used;
  values[3] = external;
  values[4] = array_buffers;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsResourceUsageBufferCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  if (argc < 1 || argv[0] == nullptr) return nullptr;
  double* values = nullptr;
  size_t length = 0;
  if (!GetFloat64ArrayData(env, argv[0], 16, &values, &length)) return nullptr;
  uv_rusage_t rusage;
  const int rc = uv_getrusage(&rusage);
  if (rc != 0) {
    ThrowSystemError(env, rc, "uv_getrusage");
    return nullptr;
  }
  for (size_t i = 0; i < length; ++i) values[i] = 0;
  values[0] = kMicrosPerSec * rusage.ru_utime.tv_sec + rusage.ru_utime.tv_usec;
  values[1] = kMicrosPerSec * rusage.ru_stime.tv_sec + rusage.ru_stime.tv_usec;
  values[2] = static_cast<double>(rusage.ru_maxrss);
  values[3] = static_cast<double>(rusage.ru_ixrss);
  values[4] = static_cast<double>(rusage.ru_idrss);
  values[5] = static_cast<double>(rusage.ru_isrss);
  values[6] = static_cast<double>(rusage.ru_minflt);
  values[7] = static_cast<double>(rusage.ru_majflt);
  values[8] = static_cast<double>(rusage.ru_nswap);
  values[9] = static_cast<double>(rusage.ru_inblock);
  values[10] = static_cast<double>(rusage.ru_oublock);
  values[11] = static_cast<double>(rusage.ru_msgsnd);
  values[12] = static_cast<double>(rusage.ru_msgrcv);
  values[13] = static_cast<double>(rusage.ru_nsignals);
  values[14] = static_cast<double>(rusage.ru_nvcsw);
  values[15] = static_cast<double>(rusage.ru_nivcsw);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

void UpdateHrtimeBuffer(napi_env env, bool write_bigint) {
  ProcessMethodsBindingState* state = GetProcessMethodsState(env);
  if (state == nullptr || state->hrtime_buffer_ref == nullptr) return;
  napi_value buffer = nullptr;
  if (napi_get_reference_value(env, state->hrtime_buffer_ref, &buffer) != napi_ok || buffer == nullptr) return;
  bool is_typedarray = false;
  if (napi_is_typedarray(env, buffer, &is_typedarray) != napi_ok || !is_typedarray) return;
  napi_typedarray_type ta_type;
  size_t length = 0;
  void* data = nullptr;
  napi_value arraybuffer = nullptr;
  size_t byte_offset = 0;
  if (napi_get_typedarray_info(env, buffer, &ta_type, &length, &data, &arraybuffer, &byte_offset) != napi_ok ||
      data == nullptr || ta_type != napi_uint32_array || length < 3) {
    return;
  }
  uint32_t* values = static_cast<uint32_t*>(data);
  const uint64_t now_ns = GetHrtimeNanoseconds();
  if (write_bigint) {
    values[0] = static_cast<uint32_t>(now_ns & 0xffffffffull);
    values[1] = static_cast<uint32_t>((now_ns >> 32) & 0xffffffffull);
    return;
  }
  const uint64_t sec = now_ns / 1000000000ull;
  const uint32_t nsec = static_cast<uint32_t>(now_ns % 1000000000ull);
  values[0] = static_cast<uint32_t>((sec >> 32) & 0xffffffffull);
  values[1] = static_cast<uint32_t>(sec & 0xffffffffull);
  values[2] = nsec;
}

napi_value ProcessMethodsHrtimeCallback(napi_env env, napi_callback_info info) {
  UpdateHrtimeBuffer(env, false);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsHrtimeBigIntCallback(napi_env env, napi_callback_info info) {
  UpdateHrtimeBuffer(env, true);
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsSetEmitWarningSyncCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) == napi_ok && argc >= 1 && argv[0] != nullptr) {
    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, argv[0], &type) == napi_ok && type == napi_function) {
      auto* state = GetProcessMethodsState(env);
      if (state != nullptr) {
        if (state->emit_warning_sync_ref != nullptr) {
          napi_delete_reference(env, state->emit_warning_sync_ref);
          state->emit_warning_sync_ref = nullptr;
        }
        napi_create_reference(env, argv[0], 1, &state->emit_warning_sync_ref);
      }
    }
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ProcessMethodsResetStdioForTestingCallback(napi_env env, napi_callback_info info) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

void ProcessMethodsBindingFinalize(napi_env env, void* data, void* hint) {
  (void)data;
  (void)hint;
  auto* state = GetProcessMethodsState(env);
  if (state == nullptr) return;
  DeleteRefIfPresent(env, &state->hrtime_buffer_ref);
  DeleteRefIfPresent(env, &state->emit_warning_sync_ref);
  DeleteRefIfPresent(env, &state->binding_ref);
}

bool WriteReportForUncaughtExceptionImpl(napi_env env, napi_value exception) {
  ReportBindingState* state = GetReportState(env);
  if (state == nullptr || !state->report_on_uncaught_exception) return false;

  const std::string filename = ResolveReportFilename(state, "");
  std::string event_message = "Exception";
  napi_valuetype error_type = napi_undefined;
  if (exception != nullptr && napi_typeof(env, exception, &error_type) == napi_ok &&
      (error_type == napi_object || error_type == napi_function)) {
    napi_value stack_value = nullptr;
    bool has_stack = GetNamedPropertyIfPresent(env, exception, "stack", &stack_value) &&
                     stack_value != nullptr;
    bool stack_is_defined = false;
    if (has_stack) {
      napi_valuetype stack_type = napi_undefined;
      stack_is_defined = napi_typeof(env, stack_value, &stack_type) == napi_ok && stack_type != napi_undefined;
    }

    if (!stack_is_defined) {
      napi_value message_value = nullptr;
      napi_value name_value = nullptr;
      const bool has_message = GetNamedPropertyIfPresent(env, exception, "message", &message_value) &&
                               message_value != nullptr &&
                               !IsNullOrUndefinedValue(env, message_value);
      const bool has_name = GetNamedPropertyIfPresent(env, exception, "name", &name_value) &&
                            name_value != nullptr &&
                            !IsNullOrUndefinedValue(env, name_value);
      if (has_message && has_name) {
        const std::string message_text = NapiValueToUtf8(env, message_value);
        if (!message_text.empty()) event_message = message_text;
      }
    }
  }

  napi_value report_obj = BuildReportObject(env, event_message, "Exception", filename, exception);
  if (report_obj == nullptr) return false;

  napi_value json_string = StringifyReportObject(env, report_obj, state->compact);
  if (json_string == nullptr) return false;

  std::string payload = NapiValueToUtf8(env, json_string);
  payload.push_back('\n');
  return WriteReportPayload(state, filename, payload);
}

napi_value ReportWriteReportCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  const std::string event_message = (argc >= 1 && argv[0] != nullptr) ? NapiValueToUtf8(env, argv[0]) : "JavaScript API";
  const std::string trigger = (argc >= 2 && argv[1] != nullptr) ? NapiValueToUtf8(env, argv[1]) : "API";
  std::string requested_file;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, argv[2], &t) == napi_ok && t == napi_string) {
      requested_file = NapiValueToUtf8(env, argv[2]);
    }
  }
  ReportBindingState* state = GetReportState(env);
  if (state == nullptr) return nullptr;

  const std::string filename = ResolveReportFilename(state, requested_file);

  napi_value report_obj = BuildReportObject(env, event_message, trigger, filename, argc >= 4 ? argv[3] : nullptr);
  if (report_obj == nullptr) return nullptr;
  napi_value json_string = StringifyReportObject(env, report_obj, state->compact);
  if (json_string == nullptr) return nullptr;
  std::string payload = NapiValueToUtf8(env, json_string);
  payload.push_back('\n');

  if (!WriteReportPayload(state, filename, payload)) {
    napi_value empty = nullptr;
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty);
    return empty;
  }

  napi_value path_value = nullptr;
  napi_create_string_utf8(env, filename.c_str(), NAPI_AUTO_LENGTH, &path_value);
  return path_value;
}

napi_value ReportGetReportCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  const std::string event_message = "JavaScript API";
  const std::string trigger = "GetReport";
  ReportBindingState* state = GetReportState(env);
  napi_value report_obj = BuildReportObject(env, event_message, trigger, "", argc >= 1 ? argv[0] : nullptr);
  if (report_obj == nullptr) return nullptr;
  return StringifyReportObject(env, report_obj, state != nullptr && state->compact);
}

napi_value ReportGetCompactCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->compact, &out);
  return out;
}

napi_value ReportSetCompactCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->compact = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportGetExcludeNetworkCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->exclude_network, &out);
  return out;
}

napi_value ReportSetExcludeNetworkCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->exclude_network = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportGetExcludeEnvCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->exclude_env, &out);
  return out;
}

napi_value ReportSetExcludeEnvCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->exclude_env = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportGetDirectoryCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  const std::string value = state != nullptr ? state->directory : "";
  napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &out);
  return out;
}

napi_value ReportSetDirectoryCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    state->directory = NapiValueToUtf8(env, argv[0]);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportGetFilenameCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  const std::string value = state != nullptr ? state->filename : "";
  napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &out);
  return out;
}

napi_value ReportSetFilenameCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    state->filename = NapiValueToUtf8(env, argv[0]);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportGetSignalCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  const std::string value = state != nullptr ? state->signal : "SIGUSR2";
  napi_create_string_utf8(env, value.c_str(), NAPI_AUTO_LENGTH, &out);
  return out;
}

napi_value ReportSetSignalCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    state->signal = NapiValueToUtf8(env, argv[0]);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportShouldReportOnFatalErrorCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->report_on_fatal_error, &out);
  return out;
}

napi_value ReportSetReportOnFatalErrorCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->report_on_fatal_error = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportShouldReportOnSignalCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->report_on_signal, &out);
  return out;
}

napi_value ReportSetReportOnSignalCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->report_on_signal = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReportShouldReportOnUncaughtExceptionCallback(napi_env env, napi_callback_info info) {
  ReportBindingState* state = GetReportState(env);
  napi_value out = nullptr;
  napi_get_boolean(env, state != nullptr && state->report_on_uncaught_exception, &out);
  return out;
}

napi_value ReportSetReportOnUncaughtExceptionCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr);
  ReportBindingState* state = GetReportState(env);
  if (state != nullptr && argc >= 1 && argv[0] != nullptr) {
    bool value = false;
    if (ValueToBool(env, argv[0], &value)) state->report_on_uncaught_exception = value;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

void ReportBindingFinalize(napi_env env, void* data, void* hint) {
  (void)data;
  (void)hint;
  auto* state = GetReportState(env);
  if (state == nullptr) return;
  DeleteRefIfPresent(env, &state->binding_ref);
}

}  // namespace

#if defined(ENABLE_TRACING)
uint64_t EdgeGetProcessStartTimeNanoseconds() {
  return g_process_start_time_ns;
}
#endif

void EdgeSetProcessArgv0(const std::string& argv0) {
  g_edge_argv0 = argv0;
}

std::string EdgeGetProcessExecPath() {
  if (g_edge_exec_path.empty()) g_edge_exec_path = DetectExecPath();
  return g_edge_exec_path;
}

napi_status EdgeInstallProcessObject(napi_env env,
                                      const std::string& current_script_path,
                                      const std::vector<std::string>& exec_argv,
                                      const std::vector<std::string>& script_argv,
                                      const std::string& process_title) {
  if (env == nullptr) return napi_invalid_arg;
  if (g_edge_exec_path.empty()) g_edge_exec_path = DetectExecPath();
  if (g_edge_argv0.empty()) g_edge_argv0 = g_edge_exec_path;
  napi_value global = nullptr;
  napi_status status = napi_get_global(env, &global);
  if (status != napi_ok || global == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value process_obj = nullptr;
  status = napi_create_object(env, &process_obj);
  if (status != napi_ok || process_obj == nullptr) return (status == napi_ok) ? napi_generic_failure : status;

  napi_value env_obj = CreateProcessEnvObject(env);
  if (env_obj == nullptr) return napi_generic_failure;
  status = napi_set_named_property(env, process_obj, "env", env_obj);
  if (status != napi_ok) return status;

  napi_value argv_arr = nullptr;
  const bool has_script_path = !current_script_path.empty();
  const size_t argv_len = has_script_path ? (2 + script_argv.size()) : (1 + script_argv.size());
  status = napi_create_array_with_length(env, argv_len, &argv_arr);
  if (status != napi_ok || argv_arr == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value exec_argv0 = nullptr;
  napi_create_string_utf8(env, g_edge_argv0.c_str(), NAPI_AUTO_LENGTH, &exec_argv0);
  if (exec_argv0 != nullptr) napi_set_element(env, argv_arr, 0, exec_argv0);
  if (has_script_path) {
    napi_value script_argv1 = nullptr;
    napi_create_string_utf8(env, current_script_path.c_str(), NAPI_AUTO_LENGTH, &script_argv1);
    if (script_argv1 != nullptr) napi_set_element(env, argv_arr, 1, script_argv1);
    for (size_t i = 0; i < script_argv.size(); ++i) {
      napi_value arg = nullptr;
      if (napi_create_string_utf8(env, script_argv[i].c_str(), NAPI_AUTO_LENGTH, &arg) == napi_ok &&
          arg != nullptr) {
        napi_set_element(env, argv_arr, static_cast<uint32_t>(i + 2), arg);
      }
    }
  } else {
    for (size_t i = 0; i < script_argv.size(); ++i) {
      napi_value arg = nullptr;
      if (napi_create_string_utf8(env, script_argv[i].c_str(), NAPI_AUTO_LENGTH, &arg) == napi_ok &&
          arg != nullptr) {
        napi_set_element(env, argv_arr, static_cast<uint32_t>(i + 1), arg);
      }
    }
  }
  status = napi_set_named_property(env, process_obj, "argv", argv_arr);
  if (status != napi_ok) return status;

  napi_value exec_argv_arr = nullptr;
  status = napi_create_array_with_length(env, exec_argv.size(), &exec_argv_arr);
  if (status != napi_ok || exec_argv_arr == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  for (size_t i = 0; i < exec_argv.size(); i++) {
    napi_value v = nullptr;
    if (napi_create_string_utf8(env, exec_argv[i].c_str(), NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr) {
      napi_set_element(env, exec_argv_arr, static_cast<uint32_t>(i), v);
    }
  }
  status = napi_set_named_property(env, process_obj, "execArgv", exec_argv_arr);
  if (status != napi_ok) return status;

  const bool owns_process_state = EdgeWorkerEnvOwnsProcessState(env);
  std::string title = process_title;
  if (title.empty()) {
    title = GetProcessTitleString();
  }
  if (title.empty()) {
    title = "node";
  }
  if (owns_process_state && !process_title.empty()) {
    g_process_title = title;
    (void)uv_set_process_title(g_process_title.c_str());
  } else if (!owns_process_state) {
    EdgeWorkerEnvSetProcessTitle(env, title);
  }
  napi_value title_value = nullptr;
  status = napi_create_string_utf8(env, title.c_str(), NAPI_AUTO_LENGTH, &title_value);
  if (status != napi_ok || title_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "title", title_value);
  if (status != napi_ok) return status;

  if (!owns_process_state) {
    EdgeWorkerEnvSetDebugPort(env, g_process_debug_port);
  }

  napi_value argv0_value = nullptr;
  status = napi_create_string_utf8(env, g_edge_argv0.c_str(), NAPI_AUTO_LENGTH, &argv0_value);
  if (status != napi_ok || argv0_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "argv0", argv0_value);
  if (status != napi_ok) return status;

  napi_value cwd_fn = nullptr;
  status = napi_create_function(env, "cwd", NAPI_AUTO_LENGTH, ProcessCwdCallback, nullptr, &cwd_fn);
  if (status != napi_ok || cwd_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "cwd", cwd_fn);
  if (status != napi_ok) return status;

  napi_value chdir_fn = nullptr;
  status = napi_create_function(env, "chdir", NAPI_AUTO_LENGTH, ProcessChdirCallback, nullptr, &chdir_fn);
  if (status != napi_ok || chdir_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "chdir", chdir_fn);
  if (status != napi_ok) return status;

  napi_value cpu_usage_fn = nullptr;
  status = napi_create_function(env, "cpuUsage", NAPI_AUTO_LENGTH, ProcessCpuUsageCallback, nullptr, &cpu_usage_fn);
  if (status != napi_ok || cpu_usage_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "cpuUsage", cpu_usage_fn);
  if (status != napi_ok) return status;

  napi_value available_memory_fn = nullptr;
  status = napi_create_function(env, "availableMemory", NAPI_AUTO_LENGTH, ProcessAvailableMemoryCallback, nullptr, &available_memory_fn);
  if (status != napi_ok || available_memory_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "availableMemory", available_memory_fn);
  if (status != napi_ok) return status;

  napi_value constrained_memory_fn = nullptr;
  status = napi_create_function(env, "constrainedMemory", NAPI_AUTO_LENGTH, ProcessConstrainedMemoryCallback, nullptr, &constrained_memory_fn);
  if (status != napi_ok || constrained_memory_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "constrainedMemory", constrained_memory_fn);
  if (status != napi_ok) return status;

  napi_value umask_fn = nullptr;
  status = napi_create_function(env, "umask", NAPI_AUTO_LENGTH, ProcessUmaskCallback, nullptr, &umask_fn);
  if (status != napi_ok || umask_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "umask", umask_fn);
  if (status != napi_ok) return status;

  napi_value uptime_fn = nullptr;
  status = napi_create_function(env, "uptime", NAPI_AUTO_LENGTH, ProcessUptimeCallback, nullptr, &uptime_fn);
  if (status != napi_ok || uptime_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "uptime", uptime_fn);
  if (status != napi_ok) return status;

  napi_value hrtime_fn = nullptr;
  status = napi_create_function(env, "hrtime", NAPI_AUTO_LENGTH, ProcessHrtimeCallback, nullptr, &hrtime_fn);
  if (status != napi_ok || hrtime_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value hrtime_bigint_fn = nullptr;
  status = napi_create_function(env, "bigint", NAPI_AUTO_LENGTH, ProcessHrtimeBigintCallback, nullptr, &hrtime_bigint_fn);
  if (status != napi_ok || hrtime_bigint_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, hrtime_fn, "bigint", hrtime_bigint_fn);
  if (status != napi_ok) return status;
  status = napi_set_named_property(env, process_obj, "hrtime", hrtime_fn);
  if (status != napi_ok) return status;

  napi_value raw_debug_fn = nullptr;
  status =
      napi_create_function(env, "_rawDebug", NAPI_AUTO_LENGTH, ProcessMethodsRawDebugCallback, nullptr, &raw_debug_fn);
  if (status != napi_ok || raw_debug_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "_rawDebug", raw_debug_fn);
  if (status != napi_ok) return status;

  napi_value abort_fn = nullptr;
  status = napi_create_function(env, "abort", NAPI_AUTO_LENGTH, ProcessAbortCallback, nullptr, &abort_fn);
  if (status != napi_ok || abort_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  if (!SetFunctionPrototypeUndefined(env, abort_fn)) return napi_generic_failure;
  status = napi_set_named_property(env, process_obj, "abort", abort_fn);
  if (status != napi_ok) return status;

  napi_value exit_fn = nullptr;
  status = napi_create_function(env, "exit", NAPI_AUTO_LENGTH, ProcessExitCallback, nullptr, &exit_fn);
  if (status != napi_ok || exit_fn == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "exit", exit_fn);
  if (status != napi_ok) return status;

  napi_value arch_str = nullptr;
  status = napi_create_string_utf8(env, DetectArch(), NAPI_AUTO_LENGTH, &arch_str);
  if (status != napi_ok || arch_str == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "arch", arch_str);
  if (status != napi_ok) return status;

  napi_value platform_str = nullptr;
  status = napi_create_string_utf8(env, DetectPlatform(), NAPI_AUTO_LENGTH, &platform_str);
  if (status != napi_ok || platform_str == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "platform", platform_str);
  if (status != napi_ok) return status;

  napi_value exec_path = nullptr;
  status = napi_create_string_utf8(env, g_edge_exec_path.c_str(), NAPI_AUTO_LENGTH, &exec_path);
  if (status != napi_ok || exec_path == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "execPath", exec_path);
  if (status != napi_ok) return status;

  napi_value version_str = nullptr;
  status = napi_create_string_utf8(env, NODE_COMPAT_VERSION, NAPI_AUTO_LENGTH, &version_str);
  if (status != napi_ok || version_str == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "version", version_str);
  if (status != napi_ok) return status;

  napi_value pid_value = nullptr;
  status = napi_create_int32(env, static_cast<int32_t>(uv_os_getpid()), &pid_value);
  if (status != napi_ok || pid_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "pid", pid_value);
  if (status != napi_ok) return status;

  napi_value ppid_value = nullptr;
  status = napi_create_int32(env, static_cast<int32_t>(uv_os_getppid()), &ppid_value);
  if (status != napi_ok || ppid_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, process_obj, "ppid", ppid_value);
  if (status != napi_ok) return status;

  napi_value versions_obj = nullptr;
  status = napi_create_object(env, &versions_obj);
  if (status != napi_ok || versions_obj == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  const bool has_intl = RuntimeHasIntl(env);
  std::vector<ProcessVersionEntry> version_entries = BuildProcessVersionEntries(has_intl);

  napi_value node_version_value = nullptr;
  status = napi_create_string_utf8(env, NODE_COMPAT_VERSION_STRING, NAPI_AUTO_LENGTH, &node_version_value);
  if (status != napi_ok || node_version_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_property_descriptor node_prop = {};
  node_prop.utf8name = "node";
  node_prop.value = node_version_value;
  node_prop.attributes = napi_enumerable;
  status = napi_define_properties(env, versions_obj, 1, &node_prop);
  if (status != napi_ok) return status;

  for (const auto& entry : version_entries) {
    if (entry.value.empty()) continue;
    napi_value value = nullptr;
    status = napi_create_string_utf8(env, entry.value.c_str(), NAPI_AUTO_LENGTH, &value);
    if (status != napi_ok || value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
    napi_property_descriptor prop = {};
    prop.utf8name = entry.key;
    prop.value = value;
    prop.attributes = napi_enumerable;
    status = napi_define_properties(env, versions_obj, 1, &prop);
    if (status != napi_ok) return status;
  }
  status = napi_set_named_property(env, process_obj, "versions", versions_obj);
  if (status != napi_ok) return status;

  napi_value release_obj = nullptr;
  status = napi_create_object(env, &release_obj);
  if (status != napi_ok || release_obj == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value release_name = nullptr;
  status = napi_create_string_utf8(env, "node", NAPI_AUTO_LENGTH, &release_name);
  if (status != napi_ok || release_name == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, release_obj, "name", release_name);
  if (status != napi_ok) return status;
#if NODE_VERSION_IS_LTS
  napi_value release_lts = nullptr;
  status = napi_create_string_utf8(env, NODE_VERSION_LTS_CODENAME, NAPI_AUTO_LENGTH, &release_lts);
  if (status != napi_ok || release_lts == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, release_obj, "lts", release_lts);
  if (status != napi_ok) return status;
#endif
  const std::string release_url_prefix =
      std::string("https://nodejs.org/download/release/v") + NODE_COMPAT_VERSION_STRING + "/";
  const std::string release_file_prefix =
      release_url_prefix + "node-v" + NODE_COMPAT_VERSION_STRING;
  napi_value source_url = nullptr;
  status = napi_create_string_utf8(
      env,
      (release_file_prefix + ".tar.gz").c_str(),
      NAPI_AUTO_LENGTH,
      &source_url);
  if (status != napi_ok || source_url == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, release_obj, "sourceUrl", source_url);
  if (status != napi_ok) return status;
  napi_value headers_url = nullptr;
  status = napi_create_string_utf8(
      env,
      (release_file_prefix + "-headers.tar.gz").c_str(),
      NAPI_AUTO_LENGTH,
      &headers_url);
  if (status != napi_ok || headers_url == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  status = napi_set_named_property(env, release_obj, "headersUrl", headers_url);
  if (status != napi_ok) return status;
  status = napi_set_named_property(env, process_obj, "release", release_obj);
  if (status != napi_ok) return status;

  napi_value features_obj = nullptr;
  status = napi_create_object(env, &features_obj);
  if (status != napi_ok || features_obj == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value true_value = nullptr;
  status = napi_get_boolean(env, true, &true_value);
  if (status != napi_ok || true_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  napi_value false_value = nullptr;
  status = napi_get_boolean(env, false, &false_value);
  if (status != napi_ok || false_value == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
  const bool has_openssl = !GetOpenSslVersion().empty() && GetOpenSslVersion() != "0.0.0";
#ifdef OPENSSL_IS_BORINGSSL
  const bool openssl_is_boringssl = true;
#else
  const bool openssl_is_boringssl = false;
#endif
  if (napi_set_named_property(env, features_obj, "inspector", false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "debug", false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "uv", true_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "ipv6", true_value) != napi_ok ||
      napi_set_named_property(env,
                              features_obj,
                              "openssl_is_boringssl",
                              openssl_is_boringssl ? true_value : false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "tls_alpn", has_openssl ? true_value : false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "tls_sni", has_openssl ? true_value : false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "tls_ocsp", has_openssl ? true_value : false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "tls", has_openssl ? true_value : false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "cached_builtins", false_value) != napi_ok ||
      napi_set_named_property(env, features_obj, "require_module", false_value) != napi_ok) {
    return napi_generic_failure;
  }
  if (napi_set_named_property(env, features_obj, "typescript", false_value) != napi_ok) return napi_generic_failure;
  status = napi_set_named_property(env, process_obj, "features", features_obj);
  if (status != napi_ok) return status;

  napi_value config_obj = BuildProcessConfigObject(env);
  if (config_obj == nullptr) return napi_generic_failure;
  napi_property_descriptor config_desc = {};
  config_desc.utf8name = "config";
  config_desc.value = config_obj;
  // Node bootstrap redefines process.config with specific descriptors.
  config_desc.attributes = static_cast<napi_property_attributes>(
      napi_writable | napi_enumerable | napi_configurable);
  status = napi_define_properties(env, process_obj, 1, &config_desc);
  if (status != napi_ok) return status;

  status = napi_set_named_property(env, global, "process", process_obj);
  if (status != napi_ok) return status;

  // Native internalBinding('process_methods')
  {
    auto& state = EnsureProcessMethodsState(env);
    DeleteRefIfPresent(env, &state.binding_ref);
    DeleteRefIfPresent(env, &state.hrtime_buffer_ref);
    DeleteRefIfPresent(env, &state.emit_warning_sync_ref);
    napi_value binding = nullptr;
    status = napi_create_object(env, &binding);
    if (status != napi_ok || binding == nullptr) return (status == napi_ok) ? napi_generic_failure : status;
    napi_wrap(env, binding, nullptr, ProcessMethodsBindingFinalize, nullptr, nullptr);

    napi_value hrtime_buffer_ab = nullptr;
    if (napi_create_arraybuffer(env, sizeof(uint32_t) * 3, nullptr, &hrtime_buffer_ab) != napi_ok ||
        hrtime_buffer_ab == nullptr) {
      return napi_generic_failure;
    }
    napi_value hrtime_buffer = nullptr;
    if (napi_create_typedarray(
            env, napi_uint32_array, 3, hrtime_buffer_ab, 0, &hrtime_buffer) != napi_ok ||
        hrtime_buffer == nullptr) {
      return napi_generic_failure;
    }
    if (napi_create_reference(env, hrtime_buffer, 1, &state.hrtime_buffer_ref) != napi_ok ||
        state.hrtime_buffer_ref == nullptr) {
      return napi_generic_failure;
    }
    if (napi_set_named_property(env, binding, "hrtimeBuffer", hrtime_buffer) != napi_ok) return napi_generic_failure;

    struct BindingMethod {
      const char* name;
      napi_callback cb;
    };
    const BindingMethod methods[] = {
        {"_debugProcess", ProcessMethodsDebugProcessCallback},
        {"abort", ProcessAbortCallback},
        {"causeSegfault", ProcessMethodsCauseSegfaultCallback},
        {"chdir", ProcessChdirCallback},
        {"umask", ProcessUmaskCallback},
        {"memoryUsage", ProcessMethodsMemoryUsageBufferCallback},
        {"constrainedMemory", ProcessConstrainedMemoryCallback},
        {"availableMemory", ProcessAvailableMemoryCallback},
        {"rss", ProcessMethodsRssCallback},
        {"cpuUsage", ProcessMethodsCpuUsageBufferCallback},
        {"threadCpuUsage", ProcessMethodsThreadCpuUsageBufferCallback},
        {"resourceUsage", ProcessMethodsResourceUsageBufferCallback},
        {"_debugEnd", ProcessMethodsNoopUndefinedCallback},
        {"_getActiveRequests", ProcessMethodsGetActiveRequestsCallback},
        {"_getActiveHandles", ProcessMethodsGetActiveHandlesCallback},
        {"getActiveResourcesInfo", ProcessMethodsGetActiveResourcesInfoCallback},
        {"_kill", ProcessMethodsKillCallback},
        {"_rawDebug", ProcessMethodsRawDebugCallback},
        {"cwd", ProcessCwdCallback},
        {"dlopen", ProcessMethodsDlopenCallback},
        {"reallyExit", ProcessExitCallback},
        {"execve", ProcessMethodsExecveCallback},
        {"uptime", ProcessUptimeCallback},
        {"patchProcessObject", ProcessMethodsPatchProcessObjectCallback},
        {"loadEnvFile", ProcessMethodsLoadEnvFileCallback},
        {"setEmitWarningSync", ProcessMethodsSetEmitWarningSyncCallback},
        {"hrtime", ProcessMethodsHrtimeCallback},
        {"hrtimeBigInt", ProcessMethodsHrtimeBigIntCallback},
    };
    for (const auto& method : methods) {
      napi_value fn = nullptr;
      if (napi_create_function(env, method.name, NAPI_AUTO_LENGTH, method.cb, nullptr, &fn) != napi_ok ||
          fn == nullptr ||
          napi_set_named_property(env, binding, method.name, fn) != napi_ok) {
        return napi_generic_failure;
      }
      if (std::strcmp(method.name, "abort") == 0 && !SetFunctionPrototypeUndefined(env, fn)) {
        return napi_generic_failure;
      }
    }
    UpdateHrtimeBuffer(env, false);
    if (napi_create_reference(env, binding, 1, &state.binding_ref) != napi_ok || state.binding_ref == nullptr) {
      return napi_generic_failure;
    }
  }

  // Native internalBinding('report'): keep state ready for fatal-report behavior,
  // but defer creating the JS binding object until it is actually requested.
  {
    auto& state = EnsureReportState(env);
    DeleteRefIfPresent(env, &state.binding_ref);
    state.compact = HasExecArgvFlag(exec_argv, "--report-compact");
    state.exclude_network = HasExecArgvFlag(exec_argv, "--report-exclude-network");
    state.exclude_env = HasExecArgvFlag(exec_argv, "--report-exclude-env");
    state.report_on_fatal_error = HasExecArgvFlag(exec_argv, "--report-on-fatalerror");
    state.report_on_signal = HasExecArgvFlag(exec_argv, "--report-on-signal");
    state.report_on_uncaught_exception = HasExecArgvFlag(exec_argv, "--report-uncaught-exception");
    state.has_intl = has_intl;
    state.directory = GetExecArgvStringOption(exec_argv, "--report-directory=");
    state.filename = GetExecArgvStringOption(exec_argv, "--report-filename=");
    const std::string report_signal = GetExecArgvStringOption(exec_argv, "--report-signal=");
    state.signal = report_signal.empty() ? "SIGUSR2" : report_signal;
    state.command_line = BuildCommandLineSnapshot(exec_argv, script_argv, current_script_path);
    state.max_heap_size_bytes = ReadReportHeapLimitFromExecArgv(exec_argv);
    state.sequence = 0;
  }

  if (unofficial_napi_set_fatal_error_callbacks(env, FatalErrorReportCallback, OomErrorReportCallback) != napi_ok) {
    return napi_generic_failure;
  }

  return napi_ok;
}

napi_value EdgeGetProcessMethodsBinding(napi_env env) {
  ProcessMethodsBindingState* state = GetProcessMethodsState(env);
  if (state == nullptr || state->binding_ref == nullptr) return nullptr;
  napi_value binding = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &binding) != napi_ok || binding == nullptr) return nullptr;
  return binding;
}

napi_value EdgeGetReportBinding(napi_env env) {
  ReportBindingState* state = GetReportState(env);
  if (state == nullptr) return nullptr;
  if (state->binding_ref == nullptr) {
    napi_value binding = nullptr;
    if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;
    napi_wrap(env, binding, nullptr, ReportBindingFinalize, nullptr, nullptr);

    struct BindingMethod {
      const char* name;
      napi_callback cb;
    };
    const BindingMethod methods[] = {
        {"writeReport", ReportWriteReportCallback},
        {"getReport", ReportGetReportCallback},
        {"getCompact", ReportGetCompactCallback},
        {"setCompact", ReportSetCompactCallback},
        {"getExcludeNetwork", ReportGetExcludeNetworkCallback},
        {"setExcludeNetwork", ReportSetExcludeNetworkCallback},
        {"getExcludeEnv", ReportGetExcludeEnvCallback},
        {"setExcludeEnv", ReportSetExcludeEnvCallback},
        {"getDirectory", ReportGetDirectoryCallback},
        {"setDirectory", ReportSetDirectoryCallback},
        {"getFilename", ReportGetFilenameCallback},
        {"setFilename", ReportSetFilenameCallback},
        {"getSignal", ReportGetSignalCallback},
        {"setSignal", ReportSetSignalCallback},
        {"shouldReportOnFatalError", ReportShouldReportOnFatalErrorCallback},
        {"setReportOnFatalError", ReportSetReportOnFatalErrorCallback},
        {"shouldReportOnSignal", ReportShouldReportOnSignalCallback},
        {"setReportOnSignal", ReportSetReportOnSignalCallback},
        {"shouldReportOnUncaughtException", ReportShouldReportOnUncaughtExceptionCallback},
        {"setReportOnUncaughtException", ReportSetReportOnUncaughtExceptionCallback},
    };
    for (const auto& method : methods) {
      napi_value fn = nullptr;
      if (napi_create_function(env, method.name, NAPI_AUTO_LENGTH, method.cb, nullptr, &fn) != napi_ok ||
          fn == nullptr ||
          napi_set_named_property(env, binding, method.name, fn) != napi_ok) {
        return nullptr;
      }
    }
    if (napi_create_reference(env, binding, 1, &state->binding_ref) != napi_ok || state->binding_ref == nullptr) {
      return nullptr;
    }
    return binding;
  }
  napi_value binding = nullptr;
  if (napi_get_reference_value(env, state->binding_ref, &binding) != napi_ok || binding == nullptr) return nullptr;
  return binding;
}

bool EdgeWriteReportForUncaughtException(napi_env env, napi_value exception) {
  return WriteReportForUncaughtExceptionImpl(env, exception);
}
