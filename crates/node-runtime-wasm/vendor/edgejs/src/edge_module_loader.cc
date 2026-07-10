#include "edge_module_loader.h"
#include "edge_buffer.h"
#include "edge_builtin_bytecode.h"
#include "edge_bytecode_cache.h"
#include "edge_cares_wrap.h"
#include "edge_crypto.h"
#include "edge_env_loop.h"
#include "edge_errors_binding.h"
#include "edge_encoding.h"
#include "edge_fs.h"
#include "edge_http_parser.h"
#include "edge_js_udp_wrap.h"
#include "edge_js_stream.h"
#include "edge_os.h"
#include "edge_path.h"
#include "edge_pipe_wrap.h"
#include "edge_process.h"
#include "edge_process_wrap.h"
#include "edge_signal_wrap.h"
#include "edge_spawn_sync.h"
#include "edge_stream_wrap.h"
#include "edge_string_decoder.h"
#include "edge_task_queue.h"
#include "edge_tcp_wrap.h"
#include "edge_tls_wrap.h"
#include "edge_timers_host.h"
#include "edge_trace.h"
#include "edge_tty_wrap.h"
#include "edge_udp_wrap.h"
#include "edge_url.h"
#include "edge_environment.h"
#include "edge_loader_bindings.h"
#include "edge_util.h"
#include "edge_worker_env.h"
#include "edge_option_helpers.h"
#include "binding_registry/binding_registry.h"
#include "internal_binding/watchdog.h"
#include "builtin_catalog.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cstring>
#include <cstdlib>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <optional>
#include <sstream>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#include "simdjson/simdjson.h"
#include "uv.h"
#include "unofficial_napi.h"

namespace {

#if defined(NODE_OPENSSL_DEFAULT_CIPHER_LIST)
#define EDGE_DEFAULT_CIPHER_LIST_CORE NODE_OPENSSL_DEFAULT_CIPHER_LIST
#else
#define EDGE_DEFAULT_CIPHER_LIST_CORE                                           \
  "TLS_AES_256_GCM_SHA384:"                                                    \
  "TLS_CHACHA20_POLY1305_SHA256:"                                              \
  "TLS_AES_128_GCM_SHA256:"                                                    \
  "ECDHE-RSA-AES128-GCM-SHA256:"                                               \
  "ECDHE-ECDSA-AES128-GCM-SHA256:"                                             \
  "ECDHE-RSA-AES256-GCM-SHA384:"                                               \
  "ECDHE-ECDSA-AES256-GCM-SHA384:"                                             \
  "DHE-RSA-AES128-GCM-SHA256:"                                                 \
  "ECDHE-RSA-AES128-SHA256:"                                                   \
  "DHE-RSA-AES128-SHA256:"                                                     \
  "ECDHE-RSA-AES256-SHA384:"                                                   \
  "DHE-RSA-AES256-SHA384:"                                                     \
  "ECDHE-RSA-AES256-SHA256:"                                                   \
  "DHE-RSA-AES256-SHA256:"                                                     \
  "HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA"
#endif

namespace fs = std::filesystem;

struct RequireContext;
struct ModuleLoaderState;
struct ContextifyScriptInstance;

static bool IsUndefinedValue(napi_env env, napi_value value);
static napi_value GetNamedPropertyOrUndefined(napi_env env, napi_value object, const char* key);
static ModuleLoaderState* GetModuleLoaderState(napi_env env);
static void ResetModuleLoaderState(napi_env env, ModuleLoaderState* state);
static void DeleteModuleLoaderState(void* data);

struct ModuleLoaderState {
  explicit ModuleLoaderState(napi_env env_in) : env(env_in) {}

  napi_env env = nullptr;
  std::unordered_map<std::string, napi_ref> module_cache;
  napi_ref cache_object_ref = nullptr;
  napi_ref per_context_exports_ref = nullptr;
  napi_ref primordials_ref = nullptr;
  napi_ref internal_binding_ref = nullptr;
  napi_ref private_symbols_ref = nullptr;
  napi_ref per_isolate_symbols_ref = nullptr;
  napi_ref require_ref = nullptr;
  napi_ref native_builtins_binding_ref = nullptr;
  napi_ref internal_binding_loader_ref = nullptr;
  napi_ref require_builtin_loader_ref = nullptr;
  napi_ref contextify_binding_ref = nullptr;
  struct TraceEventsBindingState {
    napi_ref binding_ref = nullptr;
    napi_ref state_update_handler_ref = nullptr;
    struct CategoryBufferState {
      napi_ref typed_array_ref = nullptr;
      uint8_t* data = nullptr;
    };
    std::unordered_map<std::string, int32_t> category_refcounts;
    std::unordered_map<std::string, CategoryBufferState> category_buffers;
  } trace_events_state;
  std::vector<RequireContext*> require_contexts;
  std::vector<ContextifyScriptInstance*> contextify_scripts;
  std::string entry_dir;
  bool finalized = false;
};

struct RequireContext {
  ModuleLoaderState* state;
  std::string base_dir;
};

struct ContextifyScriptInstance {
  ModuleLoaderState* state = nullptr;
  napi_ref wrapper_ref = nullptr;
  napi_ref host_defined_option_ref = nullptr;
};
using TraceEventsBindingState = ModuleLoaderState::TraceEventsBindingState;

void DeleteRefIfPresent(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

static void ClearContextifyHostDefinedOptionProperties(napi_env env, napi_value target);

static void DestroyContextifyScriptInstance(napi_env env,
                                            ContextifyScriptInstance* instance,
                                            bool detach_wrapper) {
  if (instance == nullptr) return;
  if (env == nullptr && instance->state != nullptr) env = instance->state->env;

  if (env != nullptr && instance->wrapper_ref != nullptr) {
    napi_value wrapper = nullptr;
    if (napi_get_reference_value(env, instance->wrapper_ref, &wrapper) == napi_ok &&
        wrapper != nullptr) {
      ClearContextifyHostDefinedOptionProperties(env, wrapper);
      if (detach_wrapper) {
        void* removed = nullptr;
        (void)napi_remove_wrap(env, wrapper, &removed);
      }
    }
  }

  DeleteRefIfPresent(env, &instance->wrapper_ref);
  DeleteRefIfPresent(env, &instance->host_defined_option_ref);
  delete instance;
}

static void ContextifyScriptFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* instance = static_cast<ContextifyScriptInstance*>(data);
  if (instance == nullptr) return;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state != nullptr && !state->finalized) {
    auto& scripts = state->contextify_scripts;
    scripts.erase(std::remove(scripts.begin(), scripts.end(), instance), scripts.end());
  }
  DestroyContextifyScriptInstance(env, instance, false);
}

static ModuleLoaderState* GetModuleLoaderState(napi_env env) {
  return EdgeEnvironmentGetSlotData<ModuleLoaderState>(
      env, kEdgeEnvironmentSlotModuleLoaderState);
}

static bool SetModuleLoaderState(napi_env env, ModuleLoaderState* state) {
  if (env == nullptr || state == nullptr) return false;
  EdgeEnvironmentSetOpaqueSlot(
      env, kEdgeEnvironmentSlotModuleLoaderState, state, DeleteModuleLoaderState);
  return true;
}

static RequireContext* AddRequireContext(ModuleLoaderState* state, std::string base_dir) {
  if (state == nullptr) return nullptr;
  auto* context = new RequireContext();
  context->state = state;
  context->base_dir = std::move(base_dir);
  state->require_contexts.push_back(context);
  return context;
}

static void FinalizeTraceEventsState(napi_env env, ModuleLoaderState* loader_state) {
  if (loader_state == nullptr) return;
  TraceEventsBindingState& state = loader_state->trace_events_state;
  DeleteRefIfPresent(env, &state.binding_ref);
  DeleteRefIfPresent(env, &state.state_update_handler_ref);
  for (auto& kv : state.category_buffers) {
    DeleteRefIfPresent(env, &kv.second.typed_array_ref);
    kv.second.data = nullptr;
  }
  state.category_buffers.clear();
  state.category_refcounts.clear();
}

static void FinalizeContextifyBindingRef(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return;
  DeleteRefIfPresent(env, &state->contextify_binding_ref);
}

static void ResetModuleLoaderState(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return;

  for (RequireContext* context : state->require_contexts) {
    delete context;
  }
  state->require_contexts.clear();

  for (ContextifyScriptInstance* script : state->contextify_scripts) {
    DestroyContextifyScriptInstance(env, script, true);
  }
  state->contextify_scripts.clear();

  for (auto& kv : state->module_cache) {
    DeleteRefIfPresent(env, &kv.second);
  }
  state->module_cache.clear();

  DeleteRefIfPresent(env, &state->cache_object_ref);
  DeleteRefIfPresent(env, &state->per_context_exports_ref);
  DeleteRefIfPresent(env, &state->primordials_ref);
  DeleteRefIfPresent(env, &state->internal_binding_ref);
  DeleteRefIfPresent(env, &state->private_symbols_ref);
  DeleteRefIfPresent(env, &state->per_isolate_symbols_ref);
  DeleteRefIfPresent(env, &state->require_ref);
  DeleteRefIfPresent(env, &state->native_builtins_binding_ref);
  DeleteRefIfPresent(env, &state->internal_binding_loader_ref);
  DeleteRefIfPresent(env, &state->require_builtin_loader_ref);
  FinalizeTraceEventsState(env, state);
  FinalizeContextifyBindingRef(env, state);
  state->entry_dir.clear();
}

static void DeleteModuleLoaderState(void* data) {
  auto* state = static_cast<ModuleLoaderState*>(data);
  if (state == nullptr) return;
  state->finalized = true;
  ResetModuleLoaderState(state->env, state);
  delete state;
}

static void FinalizeModuleLoaderState(napi_env env) {
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return;
  state->finalized = true;
  edge::binding_registry::FinalizeEnv(env);
  EdgeEnvironmentClearOpaqueSlot(env, kEdgeEnvironmentSlotModuleLoaderState);
}

fs::path ResolveSymlinkComponents(const fs::path& path) {
  fs::path current;
  for (const fs::path& part : path.lexically_normal()) {
    if (part == "." || part.empty()) continue;
    if (part == "..") {
      current /= part;
      continue;
    }
    if (part == path.root_name() || part == path.root_directory()) {
      current /= part;
      continue;
    }

    fs::path candidate = current.empty() ? part : current / part;
    std::error_code ec;
    if (fs::is_symlink(candidate, ec) && !ec) {
      fs::path target = fs::read_symlink(candidate, ec);
      if (!ec) {
        current = target.is_absolute() ? target : candidate.parent_path() / target;
        current = current.lexically_normal();
        continue;
      }
    }
    current = candidate;
  }
  return current.lexically_normal();
}

fs::path NormalizeResolvedPath(const fs::path& path) {
  std::error_code ec;
  fs::path absolute = path;
  if (!absolute.is_absolute()) {
    absolute = fs::absolute(path, ec);
    if (ec) {
      absolute = path;
      ec.clear();
    }
  }

  fs::path canonical = fs::weakly_canonical(absolute, ec);
  if (!ec) return canonical.lexically_normal();

  fs::path resolved = ResolveSymlinkComponents(absolute);
  if (resolved != absolute.lexically_normal()) return resolved.lexically_normal();
  return absolute.lexically_normal();
}

std::string ReadTextFile(const fs::path& path) {
  std::ifstream in(path);
  if (!in.is_open()) {
    const fs::path resolved = ResolveSymlinkComponents(path);
    if (resolved != path.lexically_normal()) {
      in.clear();
      in.open(resolved);
    }
  }
  if (!in.is_open()) {
    return "";
  }
  std::ostringstream ss;
  ss << in.rdbuf();
  return ss.str();
}

bool ReadTextFileWithBuiltinCache(const fs::path& path, std::string* out) {
  if (out == nullptr) return false;
  if (builtin_catalog::TryReadBuiltinSource(path, out)) {
    return true;
  }

  *out = ReadTextFile(path);
  if (!out->empty()) return true;

  std::ifstream probe(path, std::ios::binary);
  return probe.is_open();
}

void ReplaceAll(std::string* text, const std::string& from, const std::string& to) {
  if (text == nullptr || from.empty()) return;
  size_t pos = 0;
  while ((pos = text->find(from, pos)) != std::string::npos) {
    text->replace(pos, from.size(), to);
    pos += to.size();
  }
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

void ReplaceJsonBooleanOrNumber(std::string* text, const char* key, bool value) {
  if (text == nullptr || key == nullptr) return;

  const std::string needle = std::string("\"") + key + "\"";
  size_t key_pos = text->find(needle);
  if (key_pos == std::string::npos) return;

  size_t colon = text->find(':', key_pos + needle.size());
  if (colon == std::string::npos) return;

  size_t value_start = colon + 1;
  while (value_start < text->size() &&
         ((*text)[value_start] == ' ' || (*text)[value_start] == '\t' || (*text)[value_start] == '\n' ||
          (*text)[value_start] == '\r')) {
    ++value_start;
  }

  size_t value_end = value_start;
  while (value_end < text->size() && ((*text)[value_end] == '-' || std::isalnum(static_cast<unsigned char>((*text)[value_end])))) {
    ++value_end;
  }

  text->replace(value_start, value_end - value_start, value ? "1" : "0");
}

void EnsureVariablesField(std::string* text, const char* key, const char* value) {
  if (text == nullptr || key == nullptr || value == nullptr) return;
  const std::string needle = std::string("\"") + key + "\"";
  if (text->find(needle) != std::string::npos) return;

  const size_t variables_pos = text->find("\"variables\"");
  if (variables_pos == std::string::npos) return;

  const size_t object_open = text->find('{', variables_pos);
  if (object_open == std::string::npos) return;

  text->insert(object_open + 1, needle + ":" + value + ",");
}

std::string LoadBuiltinsConfigJson(bool has_intl) {
  static std::array<std::string, 2> cached;
  std::string& cached_value = cached[has_intl ? 1 : 0];
  if (!cached_value.empty()) return cached_value;
#if defined(EDGE_NODE_SHARED_OPENSSL)
  constexpr bool kEdgeNodeSharedOpenSsl = EDGE_NODE_SHARED_OPENSSL != 0;
#else
  constexpr bool kEdgeNodeSharedOpenSsl = false;
#endif

  auto append_candidate = [](std::vector<fs::path>* out, const fs::path& p) {
    if (out == nullptr) return;
    if (p.empty()) return;
    out->push_back(p);
  };

  const fs::path source_root = fs::absolute(fs::path(__FILE__).parent_path() / "..").lexically_normal();
  std::vector<fs::path> candidates;
  append_candidate(&candidates, source_root / "node" / "config.gypi");

  std::error_code ec;
  fs::path cwd = fs::current_path(ec);
  if (!ec && !cwd.empty()) {
    append_candidate(&candidates, cwd / "node" / "config.gypi");
    fs::path probe = cwd;
    for (int depth = 0; depth < 8; depth++) {
      append_candidate(&candidates, probe / "node" / "config.gypi");
      fs::path parent = probe.parent_path();
      if (parent.empty() || parent == probe) break;
      probe = parent;
    }
  }

  for (const fs::path& candidate : candidates) {
    const std::string raw = ReadTextFile(candidate);
    if (raw.empty()) continue;

    // Drop the generated comment header line and normalize bool-like strings
    // to booleans so JSON.parse() in bootstrap/node matches process.config.
    const size_t newline = raw.find('\n');
    std::string body = (newline == std::string::npos) ? raw : raw.substr(newline + 1);
    ReplaceAll(&body, "\"true\"", "true");
    ReplaceAll(&body, "\"false\"", "false");
    // Edge does not yet implement Amaro/type-stripping, so process.config
    // must not advertise it or Node's own tests will take unsupported paths.
    ReplaceJsonBooleanOrNumber(&body, "node_use_amaro", false);
    ReplaceJsonBooleanOrNumber(&body, "node_use_node_code_cache", false);
    // Edge ships its own ICU-backed encoding support and should advertise that
    // in the serialized config consumed by bootstrap/node.
    ReplaceJsonBooleanOrNumber(&body, "v8_enable_i18n_support", has_intl);
    EnsureVariablesField(&body, "node_is_edge", "true");
    ReplaceJsonBooleanOrNumber(&body, "node_shared_openssl", kEdgeNodeSharedOpenSsl);
    EnsureVariablesField(&body, "node_shared_openssl", kEdgeNodeSharedOpenSsl ? "1" : "0");
    ReplaceJsonBooleanOrNumber(&body,
                               "icu_small",
#if defined(EDGE_HAS_SMALL_ICU)
                               true);
#else
                               false);
#endif
    cached_value = body;
    if (!cached_value.empty()) return cached_value;
  }

  // Keep this fallback aligned with what Node's bootstrap expects from
  // config.gypi on modern releases.
  cached_value = std::string("{") +
                 "\"variables\":{" +
                 "\"node_is_edge\":true," +
                 "\"node_shared_openssl\":" + (kEdgeNodeSharedOpenSsl ? "1" : "0") + "," +
                 "\"v8_enable_i18n_support\":" + (has_intl ? "1" : "0") + "," +
                 "\"icu_small\":" +
#if defined(EDGE_HAS_SMALL_ICU)
                 "true,"
#else
                 "false,"
#endif
                 +
                 "\"node_use_amaro\":false," +
                 "\"node_use_node_code_cache\":false," +
                 "\"node_builtin_shareable_builtins\":[" +
                 "\"deps/cjs-module-lexer/lexer.js\"," +
                 "\"deps/cjs-module-lexer/dist/lexer.js\"," +
                 "\"deps/undici/undici.js\"," +
                 "\"deps/amaro/dist/index.js\"" +
                 "]," +
                 "\"napi_build_version\":\"10\"" +
                 "}" +
                 "}";
  return cached_value;
}

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

bool PathExistsRegularFile(const fs::path& path) {
  std::error_code ec;
  if (fs::exists(path, ec) && fs::is_regular_file(path, ec)) return true;
  const fs::path resolved = ResolveSymlinkComponents(path);
  ec.clear();
  return resolved != path.lexically_normal() && fs::exists(resolved, ec) && fs::is_regular_file(resolved, ec);
}

bool PathExistsDirectory(const fs::path& path) {
  std::error_code ec;
  if (fs::exists(path, ec) && fs::is_directory(path, ec)) return true;
  const fs::path resolved = ResolveSymlinkComponents(path);
  ec.clear();
  return resolved != path.lexically_normal() && fs::exists(resolved, ec) && fs::is_directory(resolved, ec);
}

bool ResolveAsFile(const fs::path& candidate, fs::path* out) {
  if (PathExistsRegularFile(candidate)) {
    *out = candidate;
    return true;
  }
  if (candidate.has_extension()) {
    return false;
  }

  const fs::path js_path = candidate.string() + ".js";
  if (PathExistsRegularFile(js_path)) {
    *out = js_path;
    return true;
  }
  const fs::path json_path = candidate.string() + ".json";
  if (PathExistsRegularFile(json_path)) {
    *out = json_path;
    return true;
  }
  return false;
}

bool ParsePackageMain(const fs::path& package_json_path, std::string* main_out) {
  const std::string source = ReadTextFile(package_json_path);
  if (source.empty()) {
    return false;
  }

  const std::string needle = "\"main\"";
  const size_t key_pos = source.find(needle);
  if (key_pos == std::string::npos) {
    return false;
  }
  const size_t colon_pos = source.find(':', key_pos + needle.size());
  if (colon_pos == std::string::npos) {
    return false;
  }
  const size_t first_quote = source.find('"', colon_pos + 1);
  if (first_quote == std::string::npos) {
    return false;
  }
  const size_t second_quote = source.find('"', first_quote + 1);
  if (second_quote == std::string::npos || second_quote <= first_quote + 1) {
    return false;
  }
  *main_out = source.substr(first_quote + 1, second_quote - first_quote - 1);
  return true;
}

struct SerializedPackageConfigData {
  std::optional<std::string> name;
  std::optional<std::string> main;
  std::optional<std::string> imports;
  std::optional<std::string> exports_field;
  std::string type = "none";
  std::string file_path;
};

enum class ParsePackageStatus {
  kMissing,
  kParsed,
  kInvalid,
};

int HexDigitValue(char ch) {
  if (ch >= '0' && ch <= '9') return ch - '0';
  if (ch >= 'a' && ch <= 'f') return 10 + (ch - 'a');
  if (ch >= 'A' && ch <= 'F') return 10 + (ch - 'A');
  return -1;
}

std::string PercentDecode(std::string_view input) {
  std::string out;
  out.reserve(input.size());
  for (size_t i = 0; i < input.size(); ++i) {
    const char ch = input[i];
    if (ch == '%' && i + 2 < input.size()) {
      const int hi = HexDigitValue(input[i + 1]);
      const int lo = HexDigitValue(input[i + 2]);
      if (hi >= 0 && lo >= 0) {
        out.push_back(static_cast<char>((hi << 4) | lo));
        i += 2;
        continue;
      }
    }
    out.push_back(ch);
  }
  return out;
}

std::optional<std::string> FileUrlToPathString(std::string_view specifier) {
  constexpr std::string_view kFileScheme = "file://";
  if (!(specifier.size() >= kFileScheme.size() &&
        specifier.substr(0, kFileScheme.size()) == kFileScheme)) {
    return std::nullopt;
  }

  std::string rest(specifier.substr(kFileScheme.size()));
  if (rest.rfind("localhost/", 0) == 0) {
    rest.erase(0, std::string("localhost").size());
  } else if (!rest.empty() && rest[0] != '/') {
    // Drop non-empty host section.
    const size_t slash = rest.find('/');
    if (slash == std::string::npos) return std::nullopt;
    rest.erase(0, slash);
  }

  std::string decoded = PercentDecode(rest);
#if defined(_WIN32)
  if (decoded.size() >= 3 && decoded[0] == '/' && std::isalpha(decoded[1]) &&
      decoded[2] == ':') {
    decoded.erase(decoded.begin());
  }
  std::replace(decoded.begin(), decoded.end(), '/', '\\');
#endif
  return decoded;
}

bool ThrowInvalidPackageConfig(napi_env env,
                               const std::string& path,
                               const std::string* base = nullptr,
                               const std::string* specifier = nullptr) {
  std::string message = "Invalid package config " + path;
  if (base != nullptr && specifier != nullptr && !specifier->empty()) {
    message += " while importing \"";
    message += *specifier;
    message += "\" from ";
    message += *base;
  }
  message += ".";
  napi_throw_error(env, "ERR_INVALID_PACKAGE_CONFIG", message.c_str());
  return false;
}

ParsePackageStatus ParseSerializedPackageConfig(const fs::path& package_json_path,
                                                SerializedPackageConfigData* out) {
  if (out == nullptr) return ParsePackageStatus::kInvalid;

  fs::path resolved_package_json_path = package_json_path;
  std::error_code ec;
  if (!fs::is_regular_file(resolved_package_json_path, ec) || ec) {
    const fs::path resolved = ResolveSymlinkComponents(package_json_path);
    if (resolved == package_json_path.lexically_normal()) {
      return ParsePackageStatus::kMissing;
    }
    resolved_package_json_path = resolved;
    ec.clear();
    if (!fs::is_regular_file(resolved_package_json_path, ec) || ec) {
      return ParsePackageStatus::kMissing;
    }
  }

  std::ifstream in(resolved_package_json_path, std::ios::binary);
  if (!in.is_open()) return ParsePackageStatus::kMissing;
  std::ostringstream ss;
  ss << in.rdbuf();
  std::string source = ss.str();

  simdjson::ondemand::parser parser;
  simdjson::padded_string padded(source);
  simdjson::ondemand::document document;
  simdjson::ondemand::object main_object;
  if (parser.iterate(padded).get(document) != simdjson::SUCCESS ||
      document.get_object().get(main_object) != simdjson::SUCCESS) {
    return ParsePackageStatus::kInvalid;
  }

  SerializedPackageConfigData parsed;
  parsed.file_path = resolved_package_json_path.lexically_normal().string();

  for (auto field : main_object) {
    simdjson::ondemand::raw_json_string key;
    simdjson::ondemand::value value;
    if (field.key().get(key) != simdjson::SUCCESS ||
        field.value().get(value) != simdjson::SUCCESS) {
      return ParsePackageStatus::kInvalid;
    }
    if (key.raw() == nullptr) continue;

    if (key == "name") {
      std::string_view out_name;
      if (value.get_string().get(out_name) != simdjson::SUCCESS) {
        return ParsePackageStatus::kInvalid;
      }
      parsed.name = std::string(out_name);
      continue;
    }

    if (key == "main") {
      std::string_view out_main;
      if (value.get_string().get(out_main) == simdjson::SUCCESS) {
        parsed.main = std::string(out_main);
      }
      continue;
    }

    if (key == "type") {
      std::string_view out_type;
      if (value.get_string().get(out_type) != simdjson::SUCCESS) {
        return ParsePackageStatus::kInvalid;
      }
      if (out_type == "commonjs" || out_type == "module") {
        parsed.type = std::string(out_type);
      }
      continue;
    }

    if (key == "imports" || key == "exports") {
      simdjson::ondemand::json_type field_type;
      if (value.type().get(field_type) != simdjson::SUCCESS) {
        return ParsePackageStatus::kInvalid;
      }

      std::optional<std::string>* target = (key == "imports") ? &parsed.imports : &parsed.exports_field;
      switch (field_type) {
        case simdjson::ondemand::json_type::object:
        case simdjson::ondemand::json_type::array: {
          std::string_view raw_value;
          if (value.raw_json().get(raw_value) != simdjson::SUCCESS) {
            return ParsePackageStatus::kInvalid;
          }
          *target = std::string(raw_value);
          break;
        }
        case simdjson::ondemand::json_type::string: {
          std::string_view str_value;
          if (value.get_string().get(str_value) != simdjson::SUCCESS) {
            return ParsePackageStatus::kInvalid;
          }
          *target = std::string(str_value);
          break;
        }
        default:
          break;
      }
      continue;
    }
  }

  *out = std::move(parsed);
  return ParsePackageStatus::kParsed;
}

napi_value SerializePackageConfigToArray(napi_env env, const SerializedPackageConfigData& data) {
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 6, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }

  auto set_optional_string = [&](uint32_t index, const std::optional<std::string>& value) {
    if (!value.has_value()) return;
    napi_value str = nullptr;
    if (napi_create_string_utf8(env, value->c_str(), value->size(), &str) == napi_ok && str != nullptr) {
      napi_set_element(env, out, index, str);
    }
  };

  set_optional_string(0, data.name);
  set_optional_string(1, data.main);
  set_optional_string(3, data.imports);
  set_optional_string(4, data.exports_field);

  napi_value type_value = nullptr;
  if (napi_create_string_utf8(env, data.type.c_str(), data.type.size(), &type_value) == napi_ok &&
      type_value != nullptr) {
    napi_set_element(env, out, 2, type_value);
  }

  napi_value file_path_value = nullptr;
  if (napi_create_string_utf8(env, data.file_path.c_str(), data.file_path.size(), &file_path_value) == napi_ok &&
      file_path_value != nullptr) {
    napi_set_element(env, out, 5, file_path_value);
  }
  return out;
}

std::optional<fs::path> ParsePathOrFileUrl(const std::string& input) {
  if (input.empty()) return std::nullopt;
  if (std::optional<std::string> file_path = FileUrlToPathString(input)) {
    return fs::path(*file_path);
  }
  return fs::path(input);
}

bool TraverseNearestParentPackageConfig(const fs::path& check_path,
                                        SerializedPackageConfigData* out,
                                        bool* invalid_out = nullptr) {
  if (invalid_out != nullptr) *invalid_out = false;
  fs::path current_path = check_path;
  while (true) {
    current_path = current_path.parent_path();
    if (current_path.empty() || current_path.parent_path() == current_path) {
      return false;
    }
    if (current_path.filename() == "node_modules") {
      return false;
    }

    const fs::path package_json = current_path / "package.json";
    ParsePackageStatus status = ParseSerializedPackageConfig(package_json, out);
    if (status == ParsePackageStatus::kParsed) {
      return true;
    }
    if (status == ParsePackageStatus::kInvalid) {
      if (invalid_out != nullptr) *invalid_out = true;
      return false;
    }
  }
}

bool ResolvePackageScopeFromPath(const fs::path& resolved_path,
                                 SerializedPackageConfigData* out,
                                 fs::path* unresolved_package_json_out,
                                 bool* invalid_out = nullptr) {
  if (invalid_out != nullptr) *invalid_out = false;

  fs::path start_dir = resolved_path;
  std::error_code ec;
  if (fs::is_regular_file(start_dir, ec)) {
    start_dir = start_dir.parent_path();
  } else if (start_dir.has_filename() && !start_dir.extension().empty()) {
    start_dir = start_dir.parent_path();
  }

  fs::path package_json = start_dir / "package.json";
  while (true) {
    if (package_json.parent_path().filename() == "node_modules") {
      break;
    }

    ParsePackageStatus status = ParseSerializedPackageConfig(package_json, out);
    if (status == ParsePackageStatus::kParsed) {
      return true;
    }
    if (status == ParsePackageStatus::kInvalid) {
      if (invalid_out != nullptr) *invalid_out = true;
      return false;
    }

    fs::path parent = package_json.parent_path().parent_path();
    if (parent.empty() || parent == package_json.parent_path()) {
      break;
    }
    package_json = parent / "package.json";
  }

  if (unresolved_package_json_out != nullptr) {
    *unresolved_package_json_out = start_dir / "package.json";
  }
  return false;
}

bool ResolveAsDirectory(const fs::path& candidate, fs::path* out) {
  if (!PathExistsDirectory(candidate)) {
    return false;
  }

  const fs::path package_json = candidate / "package.json";
  if (PathExistsRegularFile(package_json)) {
    std::string main_entry;
    if (ParsePackageMain(package_json, &main_entry) && !main_entry.empty()) {
      fs::path resolved_main;
      const fs::path main_candidate = candidate / main_entry;
      if (ResolveAsFile(main_candidate, &resolved_main) || ResolveAsDirectory(main_candidate, &resolved_main)) {
        *out = resolved_main;
        return true;
      }
    }
  }

  const fs::path index_js = candidate / "index.js";
  if (PathExistsRegularFile(index_js)) {
    *out = index_js;
    return true;
  }
  const fs::path index_json = candidate / "index.json";
  if (PathExistsRegularFile(index_json)) {
    *out = index_json;
    return true;
  }
  return false;
}

std::string CanonicalPathKey(const fs::path& path) {
  return edge_path::FromNamespacedPath(NormalizeResolvedPath(path).string());
}

// True if request is relative (./, ../, ., ..) or absolute (starts with /).
// Used to decide whether to try node_modules resolution (only for bare specifiers).
static bool IsRelativeOrAbsoluteRequest(const std::string& specifier) {
  if (specifier.empty()) return true;
  if (specifier[0] == '/') return true;
  if (specifier[0] != '.') return false;
  if (specifier.size() == 1) return true;  // "."
  if (specifier[1] == '/' || specifier[1] == '.') return true;  // "./" or ".."
  return false;
}

// Build list of node_modules directories to search, from from_dir upward (Node's _nodeModulePaths).
// from_dir must be absolute. Returns e.g. [from_dir/node_modules, parent/node_modules, ..., /node_modules].
static std::vector<fs::path> NodeModulePaths(const fs::path& from_dir) {
  std::vector<fs::path> out;
  fs::path from = fs::path(edge_path::PathResolve({from_dir.string()}));
  std::string from_str = from.string();
  if (from_str.empty() || (from_str.size() == 1 && from_str[0] == '/')) {
    out.push_back(fs::path("/node_modules"));
    return out;
  }
  const std::string node_modules_name = "node_modules";
  const size_t nm_len = node_modules_name.size();
  size_t last = from_str.size();
  for (size_t i = from_str.size(); i > 0; --i) {
    size_t idx = i - 1;
    if (from_str[idx] == '/' || from_str[idx] == '\\') {
      bool segment_is_node_modules = (last >= nm_len &&
          from_str.compare(last - nm_len, nm_len, node_modules_name) == 0 &&
          (last == nm_len || from_str[last - nm_len - 1] == '/' || from_str[last - nm_len - 1] == '\\'));
      if (!segment_is_node_modules) {
        out.push_back(fs::path(from_str.substr(0, last) + "/node_modules"));
      }
      last = idx;
    }
  }
  out.push_back(fs::path("/node_modules"));
  return out;
}

// For each path in node_modules_dirs, try path/request as file or directory (package main / index).
// Returns true and sets *out to the resolved absolute path when found.
static bool FindPathInNodeModules(const std::string& request,
                                  const std::vector<fs::path>& node_modules_dirs,
                                  fs::path* out) {
  for (const fs::path& nm_dir : node_modules_dirs) {
    fs::path candidate = nm_dir / request;
    fs::path resolved;
    if (ResolveAsFile(candidate, &resolved) || ResolveAsDirectory(candidate, &resolved)) {
      *out = fs::path(
          edge_path::FromNamespacedPath(edge_path::PathResolve({resolved.string()})));
      return true;
    }
  }
  return false;
}

// Resolve a bare specifier (e.g. "lodash") via node_modules walk from base_dir.
// Returns false if not found or if specifier is relative/absolute (use ResolveModulePath for those).
static bool ResolveNodeModules(const std::string& specifier, const std::string& base_dir,
                               fs::path* out) {
  if (IsRelativeOrAbsoluteRequest(specifier)) {
    return false;
  }
  std::vector<fs::path> paths = NodeModulePaths(fs::path(base_dir));
  return FindPathInNodeModules(specifier, paths, out);
}

// Resolve bare specifier (e.g. "assert", "path", "node:worker_threads")
// against lib and selected deps/internal/deps entries.
bool ResolveBuiltinPath(const std::string& specifier, const std::string& base_dir, fs::path* out) {
  (void)base_dir;
  return builtin_catalog::ResolveBuiltinId(specifier, out);
}

bool TryGetBuiltinIdFromResolvedPath(const fs::path& resolved_path, std::string* out_id) {
  return builtin_catalog::TryGetBuiltinIdForPath(resolved_path, out_id);
}

std::string ModuleSourceUrlForResolvedPath(const fs::path& resolved_path) {
  std::string builtin_id;
  if (TryGetBuiltinIdFromResolvedPath(resolved_path, &builtin_id)) {
    return "node:" + builtin_id;
  }
  return resolved_path.string();
}

static bool IsPerContextBuiltinId(const std::string& id);
static napi_value GetStatePrimordials(napi_env env, ModuleLoaderState* state);
static napi_value GetStatePrivateSymbols(napi_env env, ModuleLoaderState* state);
static napi_value GetStatePerIsolateSymbols(napi_env env, ModuleLoaderState* state);
static napi_value GetStateInternalBinding(napi_env env, ModuleLoaderState* state);
static napi_value GetGlobalInternalBindingFunction(napi_env env, napi_value global);

enum class NativeBuiltinExecutionKind {
  kUnsupported,
  kPerContext,
  kBootstrapRealm,
  kBootstrapOrMain,
};

static NativeBuiltinExecutionKind GetNativeBuiltinExecutionKind(const std::string& id) {
  if (IsPerContextBuiltinId(id)) {
    return NativeBuiltinExecutionKind::kPerContext;
  }
  if (id == "internal/bootstrap/realm") {
    return NativeBuiltinExecutionKind::kBootstrapRealm;
  }
  if (id.rfind("internal/bootstrap/", 0) == 0 || id.rfind("internal/main/", 0) == 0) {
    return NativeBuiltinExecutionKind::kBootstrapOrMain;
  }
  return NativeBuiltinExecutionKind::kUnsupported;
}

static bool GetTypedArrayBytes(napi_env env, napi_value value, const uint8_t** data_out, size_t* len_out);

// Builtins consolidated bytecode cache: returns a bytecode handle for the
// builtin either deserialized from the per-binary cache file or compiled
// eagerly and recorded for the teardown flush. nullptr on any failure with no
// pending exception, so callers fall back to the plain-text compile path and
// its exact error shaping.
static void* AcquireBuiltinBytecode(napi_env env,
                                    edge_builtin_bytecode::Kind kind,
                                    const std::string& id,
                                    const std::string& source,
                                    napi_value code,
                                    napi_value filename,
                                    int32_t shape,
                                    napi_value params_or_undefined,
                                    napi_value host_defined_option_id) {
  if (!edge_bytecode_cache::BuiltinsCacheEnabled()) return nullptr;

  edge_builtin_bytecode::PayloadView view;
  if (edge_builtin_bytecode::TryGet(kind, id, source, &view)) {
    void* handle = nullptr;
    bool rejected = false;
    if (unofficial_napi_bytecode_deserialize(env,
                                             view.data,
                                             view.size,
                                             code,
                                             filename,
                                             shape,
                                             params_or_undefined,
                                             host_defined_option_id,
                                             &handle,
                                             &rejected) == napi_ok &&
        !rejected && handle != nullptr) {
      return handle;
    }
    if (handle != nullptr) (void)unofficial_napi_bytecode_release(env, handle);
    // Engine refused bytes our header considered valid (e.g. engine flag
    // drift); recompile below so the flush rewrites the entry.
  }

  void* handle = nullptr;
  if (unofficial_napi_bytecode_compile(env,
                                       code,
                                       filename,
                                       shape,
                                       params_or_undefined,
                                       host_defined_option_id,
                                       0,
                                       0,
                                       &handle,
                                       nullptr) != napi_ok ||
      handle == nullptr) {
    bool has_pending = false;
    napi_value ignored = nullptr;
    if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }
    return nullptr;
  }
  napi_value buffer = nullptr;
  if (unofficial_napi_bytecode_serialize(env, handle, &buffer) == napi_ok && buffer != nullptr) {
    const uint8_t* bytes = nullptr;
    size_t length = 0;
    if (GetTypedArrayBytes(env, buffer, &bytes, &length) && length > 0) {
      edge_builtin_bytecode::Record(kind, id, source, bytes, length);
    }
  }
  return handle;
}

static const std::vector<std::string>& CollectRuntimeBuiltinIds() {
  static const std::vector<std::string> ids = []() {
    std::vector<std::string> out = builtin_catalog::AllBuiltinIds();
    return out;
  }();
  return ids;
}

static napi_value NoopCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value ReturnFalseCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, false, &out);
  return out;
}

static napi_value ReturnFirstArgCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  if (argc >= 1 && argv[0] != nullptr) return argv[0];
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value InspectorBuiltinFallbackCompileFunction(napi_env env, napi_callback_info info) {
  size_t argc = 6;
  napi_value argv[6] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok ||
      argc < 5 ||
      argv[2] == nullptr ||
      argv[4] == nullptr) {
    napi_throw_error(env, nullptr, "Failed to initialize inspector builtin fallback");
    return nullptr;
  }
  napi_valuetype internal_binding_type = napi_undefined;
  if (napi_typeof(env, argv[4], &internal_binding_type) != napi_ok ||
      internal_binding_type != napi_function) {
    napi_throw_error(env, nullptr, "Failed to initialize inspector builtin fallback");
    return nullptr;
  }

  napi_value name = nullptr;
  if (napi_create_string_utf8(env, "inspector", NAPI_AUTO_LENGTH, &name) != napi_ok ||
      name == nullptr) {
    return nullptr;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value inspector = nullptr;
  if (napi_call_function(env, global, argv[4], 1, &name, &inspector) != napi_ok ||
      inspector == nullptr ||
      IsUndefinedValue(env, inspector)) {
    return nullptr;
  }

  if (napi_set_named_property(env, argv[2], "exports", inspector) != napi_ok) {
    return nullptr;
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value CreateInspectorBuiltinFallbackCompileFunction(napi_env env) {
  napi_value compiled = nullptr;
  if (napi_create_function(env,
                           "inspector",
                           NAPI_AUTO_LENGTH,
                           InspectorBuiltinFallbackCompileFunction,
                           nullptr,
                           &compiled) != napi_ok ||
      compiled == nullptr) {
    return nullptr;
  }
  return compiled;
}

static napi_value BuiltinsCompileFunctionCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return nullptr;
  }
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) {
    return nullptr;
  }

  const std::string id = ValueToUtf8(env, argv[0]);
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_BUILTINS")) {
    std::fprintf(stderr, "EDGE_TRACE_BUILTINS compile %s\n", id.c_str());
  }
  if (id.empty()) {
    napi_throw_error(env, nullptr, "builtins.compileFunction requires a builtin id");
    return nullptr;
  }

  if (id == "inspector") {
    return CreateInspectorBuiltinFallbackCompileFunction(env);
  }

  std::string source;
  if (!builtin_catalog::TryReadBuiltinSource(id, &source)) {
    const std::string msg = "No such built-in module: " + id;
    napi_throw_error(env, nullptr, msg.c_str());
    return nullptr;
  }
  napi_value code = nullptr;
  if (napi_create_string_utf8(env, source.c_str(), source.size(), &code) != napi_ok || code == nullptr) {
    return nullptr;
  }
  const std::string filename_string = "node:" + id;
  napi_value filename = nullptr;
  if (napi_create_string_utf8(env, filename_string.c_str(), NAPI_AUTO_LENGTH, &filename) != napi_ok ||
      filename == nullptr) {
    return nullptr;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  napi_value params = nullptr;
  if (napi_create_array(env, &params) != napi_ok || params == nullptr) {
    return nullptr;
  }
  auto set_param = [&](uint32_t index, const char* value) -> bool {
    napi_value param = nullptr;
    return napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &param) == napi_ok &&
           param != nullptr &&
           napi_set_element(env, params, index, param) == napi_ok;
  };
  edge_builtin_bytecode::Kind bytecode_kind;
  if (id == "internal/bootstrap/realm") {
    bytecode_kind = edge_builtin_bytecode::Kind::kFnBootstrapRealm;
    if (!set_param(0, "process") ||
        !set_param(1, "getLinkedBinding") ||
        !set_param(2, "getInternalBinding") ||
        !set_param(3, "primordials")) {
      return nullptr;
    }
  } else if (IsPerContextBuiltinId(id)) {
    bytecode_kind = edge_builtin_bytecode::Kind::kFnPerContext;
    if (!set_param(0, "exports") ||
        !set_param(1, "primordials") ||
        !set_param(2, "privateSymbols") ||
        !set_param(3, "perIsolateSymbols")) {
      return nullptr;
    }
  } else if (id.rfind("internal/main/", 0) == 0 || id.rfind("internal/bootstrap/", 0) == 0) {
    bytecode_kind = edge_builtin_bytecode::Kind::kFnBootstrapMain;
    if (!set_param(0, "process") ||
        !set_param(1, "require") ||
        !set_param(2, "internalBinding") ||
        !set_param(3, "primordials")) {
      return nullptr;
    }
  } else {
    bytecode_kind = edge_builtin_bytecode::Kind::kFnLazyBuiltin;
    if (!set_param(0, "exports") ||
        !set_param(1, "require") ||
        !set_param(2, "module") ||
        !set_param(3, "process") ||
        !set_param(4, "internalBinding") ||
        !set_param(5, "primordials")) {
      return nullptr;
    }
  }
  void* builtin_bytecode =
      AcquireBuiltinBytecode(env, bytecode_kind, id, source, code, filename,
                             unofficial_napi_bytecode_shape_cjs_function, params, undefined);
  napi_value compile_result = nullptr;
  const unofficial_napi_js_source compile_source{builtin_bytecode != nullptr ? nullptr : code,
                                                 builtin_bytecode};
  const napi_status compile_status = unofficial_napi_contextify_compile_function(env,
                                                                                 &compile_source,
                                                                                 filename,
                                                                                 0,
                                                                                 0,
                                                                                 undefined,
                                                                                 undefined,
                                                                                 params,
                                                                                 undefined,
                                                                                 &compile_result);
  if (builtin_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, builtin_bytecode);
  if (compile_status != napi_ok || compile_result == nullptr) {
    return nullptr;
  }
  napi_value compiled = nullptr;
  if (napi_get_named_property(env, compile_result, "function", &compiled) != napi_ok ||
      compiled == nullptr) {
    return nullptr;
  }
  return compiled;
}

static napi_value BuiltinsNativesSourceGetter(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  void* data = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, &data) != napi_ok || data == nullptr) {
    return nullptr;
  }

  const std::string* id = static_cast<const std::string*>(data);
  std::string source;
  if (!builtin_catalog::TryReadBuiltinSource(*id, &source)) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value source_value = nullptr;
  if (napi_create_string_utf8(env, source.c_str(), source.size(), &source_value) != napi_ok ||
      source_value == nullptr) {
    return nullptr;
  }

  // Cache string value on first access to avoid repeated file reads.
  if (this_arg != nullptr) {
    napi_set_named_property(env, this_arg, id->c_str(), source_value);
  }
  return source_value;
}

static napi_value CreateBuiltinsNativesObject(napi_env env, const std::vector<std::string>& builtin_ids) {
  napi_value natives = nullptr;
  if (napi_create_object(env, &natives) != napi_ok || natives == nullptr) return nullptr;

  for (const std::string& id : builtin_ids) {
    napi_property_descriptor descriptor{
        id.c_str(),
        nullptr,
        nullptr,
        BuiltinsNativesSourceGetter,
        nullptr,
        nullptr,
        static_cast<napi_property_attributes>(napi_enumerable | napi_configurable),
        const_cast<std::string*>(&id),
    };
    if (napi_define_properties(env, natives, 1, &descriptor) != napi_ok) {
      return nullptr;
    }
  }
  return natives;
}

static napi_value BuiltinsSetInternalLoadersCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state != nullptr) {
    auto set_loader_ref = [&](napi_ref* slot, napi_value fn) {
      if (slot == nullptr) return;
      if (*slot != nullptr) {
        napi_delete_reference(env, *slot);
        *slot = nullptr;
      }
      if (fn == nullptr) return;
      napi_valuetype t = napi_undefined;
      if (napi_typeof(env, fn, &t) != napi_ok || t != napi_function) return;
      napi_create_reference(env, fn, 1, slot);
    };
    if (argc >= 1) set_loader_ref(&state->internal_binding_loader_ref, argv[0]);
    if (argc >= 2) set_loader_ref(&state->require_builtin_loader_ref, argv[1]);
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static bool AddUvErrorMapEntry(napi_env env, napi_value map, int32_t code, const char* name, const char* message) {
  napi_value code_key = nullptr;
  napi_value name_val = nullptr;
  napi_value message_val = nullptr;
  napi_value tuple = nullptr;
  if (napi_create_int32(env, code, &code_key) != napi_ok || code_key == nullptr) return false;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &name_val) != napi_ok || name_val == nullptr) {
    return false;
  }
  if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &message_val) != napi_ok ||
      message_val == nullptr) {
    return false;
  }
  if (napi_create_array_with_length(env, 2, &tuple) != napi_ok || tuple == nullptr) return false;
  if (napi_set_element(env, tuple, 0, name_val) != napi_ok) return false;
  if (napi_set_element(env, tuple, 1, message_val) != napi_ok) return false;

  napi_value set_fn = nullptr;
  if (napi_get_named_property(env, map, "set", &set_fn) != napi_ok || set_fn == nullptr) return false;
  napi_value argv[2] = {code_key, tuple};
  napi_value ignored = nullptr;
  if (napi_call_function(env, map, set_fn, 2, argv, &ignored) != napi_ok) return false;
  return true;
}

static napi_value CreateMapObject(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;
  napi_value map_ctor = nullptr;
  if (napi_get_named_property(env, global, "Map", &map_ctor) != napi_ok || map_ctor == nullptr) return nullptr;
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, map_ctor, &t) != napi_ok || t != napi_function) return nullptr;
  napi_value out = nullptr;
  if (napi_new_instance(env, map_ctor, 0, nullptr, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

static napi_value UndefinedValue(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value GetGlobalNamedProperty(napi_env env, const char* key) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;
  bool has_prop = false;
  if (napi_has_named_property(env, global, key, &has_prop) != napi_ok || !has_prop) return nullptr;
  napi_value out = nullptr;
  if (napi_get_named_property(env, global, key, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

static bool IsUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype t = napi_undefined;
  return napi_typeof(env, value, &t) == napi_ok && t == napi_undefined;
}

static napi_value GetRefValue(napi_env env, napi_ref ref) {
  if (ref == nullptr) return nullptr;
  napi_value out = nullptr;
  if (napi_get_reference_value(env, ref, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

static void ResetStateRef(napi_env env, napi_ref* slot, napi_value value) {
  if (slot == nullptr) return;
  if (*slot != nullptr) {
    napi_delete_reference(env, *slot);
    *slot = nullptr;
  }
  if (value == nullptr || IsUndefinedValue(env, value)) return;
  if (napi_create_reference(env, value, 1, slot) != napi_ok) {
    *slot = nullptr;
  }
}

static bool IsPerContextBuiltinId(const std::string& id) {
  return id.rfind("internal/per_context/", 0) == 0;
}

static napi_value GetStatePrimordials(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->primordials_ref);
}

static napi_value GetStatePerContextExports(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->per_context_exports_ref);
}

static napi_value EnsureStatePerContextExports(napi_env env, ModuleLoaderState* state) {
  napi_value exports_obj = GetStatePerContextExports(env, state);
  if (exports_obj != nullptr) return exports_obj;

  if (napi_create_object(env, &exports_obj) != napi_ok || exports_obj == nullptr) {
    return nullptr;
  }
  ResetStateRef(env, &state->per_context_exports_ref, exports_obj);
  return GetStatePerContextExports(env, state);
}

static napi_value GetStatePrivateSymbols(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->private_symbols_ref);
}

static napi_value GetStatePerIsolateSymbols(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->per_isolate_symbols_ref);
}

static bool IsFunctionValue(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

static napi_value GetStateInternalBindingLoader(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->internal_binding_loader_ref);
}

static napi_value GetStateInternalBinding(napi_env env, ModuleLoaderState* state) {
  napi_value internal_binding = GetStateInternalBindingLoader(env, state);
  if (IsFunctionValue(env, internal_binding)) {
    return internal_binding;
  }
  if (state == nullptr) return nullptr;
  return GetRefValue(env, state->internal_binding_ref);
}

static bool CreateStringArray(napi_env env,
                              std::initializer_list<const char*> values,
                              napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;

  napi_value array = nullptr;
  if (napi_create_array_with_length(env, values.size(), &array) != napi_ok || array == nullptr) {
    return false;
  }

  size_t index = 0;
  for (const char* value : values) {
    napi_value entry = nullptr;
    if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &entry) != napi_ok ||
        entry == nullptr ||
        napi_set_element(env, array, index++, entry) != napi_ok) {
      return false;
    }
  }

  *out = array;
  return true;
}

static bool ThrowNativeBuiltinExecutionError(napi_env env,
                                             const std::string& id,
                                             const std::string& message) {
  const std::string full_message = "Failed to execute builtin '" + id + "': " + message;
  napi_throw_error(env, nullptr, full_message.c_str());
  return false;
}

static bool PreservePendingNativeBuiltinExceptionWithContext(napi_env env, const std::string& id) {
  bool has_exception = false;
  if (napi_is_exception_pending(env, &has_exception) != napi_ok || !has_exception) {
    return ThrowNativeBuiltinExecutionError(env, id, "builtin threw");
  }

  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr && environment->exiting()) {
    return false;
  }

  std::fprintf(stderr, "Failed to execute builtin '%s':\n", id.c_str());
  return false;
}

static std::string LastNapiErrorMessage(napi_env env) {
  const napi_extended_error_info* error_info = nullptr;
  if (napi_get_last_error_info(env, &error_info) != napi_ok ||
      error_info == nullptr ||
      error_info->error_message == nullptr ||
      error_info->error_message[0] == '\0') {
    return {};
  }
  return error_info->error_message;
}

static bool ResolveNativeBuiltinCompileInput(napi_env env,
                                             ModuleLoaderState* state,
                                             const std::string& id,
                                             napi_value* params_out,
                                             std::vector<napi_value>* argv_out) {
  if (params_out == nullptr || argv_out == nullptr) return false;
  *params_out = nullptr;
  argv_out->clear();

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return ThrowNativeBuiltinExecutionError(env, id, "failed to fetch global object");
  }

  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return ThrowNativeBuiltinExecutionError(env, id, "process object is unavailable");
  }

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  const NativeBuiltinExecutionKind kind = GetNativeBuiltinExecutionKind(id);
  switch (kind) {
    case NativeBuiltinExecutionKind::kPerContext: {
      napi_value exports_obj = EnsureStatePerContextExports(env, state);
      if (exports_obj == nullptr) {
        return ThrowNativeBuiltinExecutionError(env, id, "failed to create per-context exports object");
      }

      napi_value primordials = GetStatePrimordials(env, state);
      if (primordials == nullptr) primordials = undefined;
      if (napi_set_named_property(env, exports_obj, "primordials", primordials) != napi_ok) {
        return ThrowNativeBuiltinExecutionError(env, id, "failed to initialize per-context exports");
      }

      napi_value private_symbols = GetStatePrivateSymbols(env, state);
      if (private_symbols == nullptr) private_symbols = undefined;
      napi_value per_isolate_symbols = GetStatePerIsolateSymbols(env, state);
      if (per_isolate_symbols == nullptr) per_isolate_symbols = undefined;

      if (!CreateStringArray(env,
                             {"exports", "primordials", "privateSymbols", "perIsolateSymbols"},
                             params_out)) {
        return ThrowNativeBuiltinExecutionError(env, id, "failed to build parameter list");
      }
      argv_out->assign({exports_obj, primordials, private_symbols, per_isolate_symbols});
      return true;
    }
    case NativeBuiltinExecutionKind::kBootstrapRealm: {
      napi_value get_linked_binding = nullptr;
      if (napi_get_named_property(env, global, "getLinkedBinding", &get_linked_binding) != napi_ok ||
          !IsFunctionValue(env, get_linked_binding)) {
        return ThrowNativeBuiltinExecutionError(env, id, "getLinkedBinding is unavailable");
      }

      napi_value get_internal_binding = nullptr;
      if (napi_get_named_property(env, global, "getInternalBinding", &get_internal_binding) != napi_ok ||
          !IsFunctionValue(env, get_internal_binding)) {
        get_internal_binding = GetGlobalInternalBindingFunction(env, global);
      }
      if (!IsFunctionValue(env, get_internal_binding)) {
        return ThrowNativeBuiltinExecutionError(env, id, "getInternalBinding is unavailable");
      }

      napi_value primordials = GetStatePrimordials(env, state);
      if (primordials == nullptr) primordials = undefined;

      if (!CreateStringArray(env,
                             {"process", "getLinkedBinding", "getInternalBinding", "primordials"},
                             params_out)) {
        return ThrowNativeBuiltinExecutionError(env, id, "failed to build parameter list");
      }
      argv_out->assign({process_obj, get_linked_binding, get_internal_binding, primordials});
      return true;
    }
    case NativeBuiltinExecutionKind::kBootstrapOrMain: {
      napi_value require_builtin = nullptr;
      if (state != nullptr) {
        require_builtin = GetRefValue(env, state->require_builtin_loader_ref);
      }
      if (!IsFunctionValue(env, require_builtin)) {
        return ThrowNativeBuiltinExecutionError(env, id, "requireBuiltin loader is unavailable");
      }

      napi_value internal_binding = GetStateInternalBinding(env, state);
      if (!IsFunctionValue(env, internal_binding)) {
        internal_binding = GetGlobalInternalBindingFunction(env, global);
      }
      if (!IsFunctionValue(env, internal_binding)) {
        return ThrowNativeBuiltinExecutionError(env, id, "internalBinding loader is unavailable");
      }

      napi_value primordials = GetStatePrimordials(env, state);
      if (primordials == nullptr) primordials = undefined;

      if (!CreateStringArray(env,
                             {"process", "require", "internalBinding", "primordials"},
                             params_out)) {
        return ThrowNativeBuiltinExecutionError(env, id, "failed to build parameter list");
      }
      argv_out->assign({process_obj, require_builtin, internal_binding, primordials});
      return true;
    }
    case NativeBuiltinExecutionKind::kUnsupported:
      return ThrowNativeBuiltinExecutionError(env, id, "unsupported native execution kind");
  }

  return false;
}

static bool ExecuteBuiltinFromNative(napi_env env, ModuleLoaderState* state, const std::string& id, napi_value* out) {
  if (out != nullptr) *out = nullptr;
  if (env == nullptr || state == nullptr || id.empty()) return false;

  std::string source;
  if (!builtin_catalog::TryReadBuiltinSource(id, &source)) {
    return ThrowNativeBuiltinExecutionError(env, id, "builtin source could not be read");
  }

  napi_value params = nullptr;
  std::vector<napi_value> argv;
  if (!ResolveNativeBuiltinCompileInput(env, state, id, &params, &argv)) {
    return false;
  }

  const std::string source_url = "node:" + id;
  napi_value code = nullptr;
  napi_value filename = nullptr;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  if (napi_create_string_utf8(env, source.c_str(), source.size(), &code) != napi_ok ||
      code == nullptr ||
      napi_create_string_utf8(env, source_url.c_str(), NAPI_AUTO_LENGTH, &filename) != napi_ok ||
      filename == nullptr) {
    return ThrowNativeBuiltinExecutionError(env, id, "failed to create compile inputs");
  }

  edge_builtin_bytecode::Kind bytecode_kind = edge_builtin_bytecode::Kind::kFnBootstrapMain;
  switch (GetNativeBuiltinExecutionKind(id)) {
    case NativeBuiltinExecutionKind::kPerContext:
      bytecode_kind = edge_builtin_bytecode::Kind::kFnPerContext;
      break;
    case NativeBuiltinExecutionKind::kBootstrapRealm:
      bytecode_kind = edge_builtin_bytecode::Kind::kFnBootstrapRealm;
      break;
    case NativeBuiltinExecutionKind::kBootstrapOrMain:
    case NativeBuiltinExecutionKind::kUnsupported:
      bytecode_kind = edge_builtin_bytecode::Kind::kFnBootstrapMain;
      break;
  }
  void* builtin_bytecode =
      AcquireBuiltinBytecode(env, bytecode_kind, id, source, code, filename,
                             unofficial_napi_bytecode_shape_cjs_function, params, undefined);
  napi_value compile_result = nullptr;
  const unofficial_napi_js_source compile_source{builtin_bytecode != nullptr ? nullptr : code,
                                                 builtin_bytecode};
  const napi_status compile_status =
      unofficial_napi_contextify_compile_function(env,
                                                  &compile_source,
                                                  filename,
                                                  0,
                                                  0,
                                                  undefined,
                                                  undefined,
                                                  params,
                                                  undefined,
                                                  &compile_result);
  if (builtin_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, builtin_bytecode);
  if (compile_status != napi_ok ||
      compile_result == nullptr) {
    std::string message = "contextify compile failed";
    const std::string last_error = LastNapiErrorMessage(env);
    if (!last_error.empty()) {
      message += ": ";
      message += last_error;
    }
    return ThrowNativeBuiltinExecutionError(env, id, message);
  }

  napi_value compiled_fn = nullptr;
  if (napi_get_named_property(env, compile_result, "function", &compiled_fn) != napi_ok ||
      !IsFunctionValue(env, compiled_fn)) {
    return ThrowNativeBuiltinExecutionError(env, id, "compiled function is unavailable");
  }

  napi_value call_result = nullptr;
  if (napi_call_function(env, undefined, compiled_fn, argv.size(), argv.data(), &call_result) != napi_ok) {
    return PreservePendingNativeBuiltinExceptionWithContext(env, id);
  }

  if (out != nullptr) {
    *out = call_result;
  }
  return true;
}

static napi_value CreateNullProtoObject(napi_env env) {
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
  napi_valuetype t = napi_undefined;
  if (napi_typeof(env, create_fn, &t) != napi_ok || t != napi_function) return nullptr;
  napi_value null_value = nullptr;
  napi_get_null(env, &null_value);
  napi_value argv[1] = {null_value};
  napi_value out = nullptr;
  if (napi_call_function(env, object_ctor, create_fn, 1, argv, &out) != napi_ok || out == nullptr) return nullptr;
  return out;
}

static bool MapSet(napi_env env, napi_value map, napi_value key, napi_value value) {
  napi_value set_fn = nullptr;
  if (napi_get_named_property(env, map, "set", &set_fn) != napi_ok || set_fn == nullptr) return false;
  napi_value argv[2] = {key, value};
  napi_value ignored = nullptr;
  return napi_call_function(env, map, set_fn, 2, argv, &ignored) == napi_ok;
}

static bool MapSetStringKeyValue(napi_env env, napi_value map, const char* key, napi_value value) {
  napi_value key_v = nullptr;
  if (napi_create_string_utf8(env, key, NAPI_AUTO_LENGTH, &key_v) != napi_ok || key_v == nullptr) return false;
  return MapSet(env, map, key_v, value);
}

static bool SetBoolProperty(napi_env env, napi_value target, const char* key, bool value) {
  napi_value v = nullptr;
  return napi_get_boolean(env, value, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, target, key, v) == napi_ok;
}

static bool SetNumberProperty(napi_env env, napi_value target, const char* key, double value) {
  napi_value v = nullptr;
  return napi_create_double(env, value, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, target, key, v) == napi_ok;
}

static bool SetStringProperty(napi_env env, napi_value target, const char* key, const char* value) {
  napi_value v = nullptr;
  return napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) == napi_ok &&
         v != nullptr &&
         napi_set_named_property(env, target, key, v) == napi_ok;
}

static bool SetArrayProperty(napi_env env, napi_value target, const char* key) {
  napi_value arr = nullptr;
  return napi_create_array(env, &arr) == napi_ok &&
         arr != nullptr &&
         napi_set_named_property(env, target, key, arr) == napi_ok;
}

static bool PushArrayString(napi_env env, napi_value arr, const std::string& value) {
  uint32_t len = 0;
  if (napi_get_array_length(env, arr, &len) != napi_ok) return false;
  napi_value str = nullptr;
  if (napi_create_string_utf8(env, value.c_str(), value.size(), &str) != napi_ok || str == nullptr) return false;
  return napi_set_element(env, arr, len, str) == napi_ok;
}

static std::vector<std::string> GetStringArrayProperty(napi_env env, napi_value obj, const char* key) {
  std::vector<std::string> out;
  if (obj == nullptr) return out;
  bool has_prop = false;
  if (napi_has_named_property(env, obj, key, &has_prop) != napi_ok || !has_prop) return out;
  napi_value arr = nullptr;
  if (napi_get_named_property(env, obj, key, &arr) != napi_ok || arr == nullptr) return out;
  bool is_array = false;
  if (napi_is_array(env, arr, &is_array) != napi_ok || !is_array) return out;
  uint32_t len = 0;
  if (napi_get_array_length(env, arr, &len) != napi_ok) return out;
  out.reserve(len);
  for (uint32_t i = 0; i < len; i++) {
    napi_value item = nullptr;
    if (napi_get_element(env, arr, i, &item) != napi_ok || item == nullptr) continue;
    napi_value str = nullptr;
    if (napi_coerce_to_string(env, item, &str) != napi_ok || str == nullptr) continue;
    size_t n = 0;
    if (napi_get_value_string_utf8(env, str, nullptr, 0, &n) != napi_ok) continue;
    std::string s(n + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, str, s.data(), s.size(), &copied) != napi_ok) continue;
    s.resize(copied);
    out.push_back(std::move(s));
  }
  return out;
}

static bool TryParseDouble(const std::string& text, double* out) {
  if (out == nullptr || text.empty()) return false;
  char* end = nullptr;
  const double v = std::strtod(text.c_str(), &end);
  if (end == nullptr || *end != '\0') return false;
  *out = v;
  return true;
}

static bool IsTruthyNestedProperty(napi_env env, napi_value obj, const std::vector<const char*>& path) {
  napi_value cur = obj;
  for (const char* key : path) {
    if (cur == nullptr) return false;
    bool has_prop = false;
    if (napi_has_named_property(env, cur, key, &has_prop) != napi_ok || !has_prop) return false;
    if (napi_get_named_property(env, cur, key, &cur) != napi_ok || cur == nullptr) return false;
  }
  bool out = false;
  napi_get_value_bool(env, cur, &out);
  return out;
}

static std::string ReadCliMarkdown() {
  std::error_code ec;
  const fs::path cwd = fs::current_path(ec);
  if (ec) return "";
  const std::vector<fs::path> candidates = {
      cwd / "test" / "doc" / "api" / "cli.md",
      cwd / "doc" / "api" / "cli.md",
      cwd / "node" / "doc" / "api" / "cli.md",
      cwd / ".." / "test" / "doc" / "api" / "cli.md",
      cwd / ".." / "doc" / "api" / "cli.md",
      cwd / ".." / "node" / "doc" / "api" / "cli.md",
      cwd / ".." / ".." / "test" / "doc" / "api" / "cli.md",
      cwd / ".." / ".." / "doc" / "api" / "cli.md",
      cwd / ".." / ".." / "node" / "doc" / "api" / "cli.md",
  };
  for (const auto& path : candidates) {
    const std::string text = ReadTextFile(path);
    if (!text.empty()) return text;
  }
  return "";
}

static std::string NormalizeOptionNameFromDoc(const std::string& raw) {
  std::string name = raw;
  const size_t space = name.find(' ');
  if (space != std::string::npos) name.resize(space);
  const size_t comma = name.find(',');
  if (comma != std::string::npos) name.resize(comma);
  if (name.rfind("--no-", 0) == 0) {
    name = "--" + name.substr(5);
  }
  return name;
}

static std::vector<std::string> ParseAllowedFlagsFromDocs() {
  const std::string text = ReadCliMarkdown();
  if (text.empty()) return {};

  const std::vector<std::pair<std::string, std::string>> sections = {
      {"<!-- node-options-node start -->", "<!-- node-options-node end -->"},
      {"<!-- node-options-v8 start -->", "<!-- node-options-v8 end -->"},
  };
  std::unordered_map<std::string, bool> dedup;
  std::vector<std::string> out;

  for (const auto& section : sections) {
    const size_t start = text.find(section.first);
    if (start == std::string::npos) continue;
    const size_t body_start = text.find('\n', start);
    if (body_start == std::string::npos) continue;
    const size_t end = text.find(section.second, body_start + 1);
    if (end == std::string::npos || end <= body_start + 1) continue;
    const std::string_view body(text.data() + body_start + 1, end - body_start - 1);

    size_t pos = 0;
    while (pos < body.size()) {
      const size_t open = body.find('`', pos);
      if (open == std::string_view::npos) break;
      const size_t close = body.find('`', open + 1);
      if (close == std::string_view::npos) break;
      std::string candidate(body.substr(open + 1, close - open - 1));
      if (!candidate.empty() && candidate[0] == '-') {
        candidate = NormalizeOptionNameFromDoc(candidate);
        if (!candidate.empty() && dedup.emplace(candidate, true).second) out.push_back(std::move(candidate));
      }
      pos = close + 1;
    }
  }

  return out;
}

static napi_value UvGetErrorMapCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value err_map = CreateMapObject(env);
  if (err_map == nullptr) return nullptr;
#define EDGE_SET_UV_ERRMAP_ENTRY(name, message)                                                   \
  if (!AddUvErrorMapEntry(env, err_map, static_cast<int32_t>(UV_##name), #name, message)) {     \
    return nullptr;                                                                               \
  }
  UV_ERRNO_MAP(EDGE_SET_UV_ERRMAP_ENTRY);
#undef EDGE_SET_UV_ERRMAP_ENTRY
#ifdef UV_EAI_MEMORY
  const int32_t eai_memory_code =
      static_cast<int32_t>(UV_EAI_MEMORY) != static_cast<int32_t>(UV_EAI_AGAIN)
          ? static_cast<int32_t>(UV_EAI_MEMORY)
          : -3006;
  if (!AddUvErrorMapEntry(env, err_map, eai_memory_code, "EAI_MEMORY", "memory allocation failure")) {
    return nullptr;
  }
#endif
  return err_map;
}

static napi_value UvGetErrorMessageCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  int32_t err = 0;
  if (napi_get_value_int32(env, argv[0], &err) != napi_ok) return nullptr;
  const char* message = uv_strerror(static_cast<int>(err));
  if (message == nullptr) message = "unknown error";
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, message, NAPI_AUTO_LENGTH, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

static napi_value UvErrNameCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1) return nullptr;
  int32_t err = 0;
  if (napi_get_value_int32(env, argv[0], &err) != napi_ok) return nullptr;
  const char* name = uv_err_name(static_cast<int>(err));
  if (name == nullptr) name = "UNKNOWN";
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

std::string GetEnvironmentOptionValue(napi_env env, const char* name) {
  if (name == nullptr || name[0] == '\0') return {};
  if (env != nullptr && !EdgeWorkerEnvOwnsProcessState(env) && !EdgeWorkerEnvSharesEnvironment(env)) {
    const std::map<std::string, std::string> entries = EdgeWorkerEnvSnapshotEnvVars(env);
    auto it = entries.find(name);
    if (it != entries.end()) return it->second;
    return {};
  }
  const char* value = std::getenv(name);
  return value != nullptr ? value : std::string();
}

bool EnvironmentOptionEnabled(napi_env env, const char* name) {
  return GetEnvironmentOptionValue(env, name) == "1";
}

static std::vector<std::string> GetEffectiveCliTokens(napi_env env) {
  napi_value process = GetGlobalNamedProperty(env, "process");
  std::vector<std::string> raw_exec_argv = GetStringArrayProperty(env, process, "execArgv");
  const edge_options::EffectiveCliState state = edge_options::BuildEffectiveCliState(raw_exec_argv);
  return state.ok ? state.effective_tokens : raw_exec_argv;
}

static bool LooksLikeCliOptionToken(const std::string& token) {
  if (token == "--" || token == "-") return true;
  if (token == "-c" || token == "--check" ||
      token == "-e" || token == "--eval" ||
      token == "-i" || token == "--interactive" ||
      token == "-p" || token == "--print" ||
      token == "-pe" || token == "-ep" ||
      token == "-r" || token == "--require" ||
      token == "--run") {
    return true;
  }
  if (token.rfind("--no-", 0) == 0) return true;
  if (token.rfind("--env-file=", 0) == 0 ||
      token.rfind("--env-file-if-exists=", 0) == 0 ||
      token.rfind("--experimental-config-file=", 0) == 0 ||
      token.rfind("--input-type=", 0) == 0) {
    return true;
  }
  const size_t eq = token.find('=');
  const std::string key = eq == std::string::npos ? token : token.substr(0, eq);
  static const std::unordered_set<std::string> kKnownOptions = {
      "--allow-addons",
      "--allow-child-process",
      "--allow-fs-read",
      "--allow-fs-write",
      "--allow-inspector",
      "--allow-wasi",
      "--allow-worker",
      "--check",
      "--debug-port",
      "--diagnostic-dir",
      "--disable-warning",
      "--dns-result-order",
      "--entry-url",
      "--env-file",
      "--env-file-if-exists",
      "--es-module-specifier-resolution",
      "--eval",
      "--experimental-config-file",
      "--experimental-fetch",
      "--experimental-global-customevent",
      "--experimental-global-webcrypto",
      "--experimental-import-meta-resolve",
      "--experimental-loader",
      "--experimental-repl-await",
      "--experimental-report",
      "--experimental-strip-types",
      "--experimental-transform-types",
      "--experimental-wasm-modules",
      "--experimental-worker",
      "--expose-internals",
      "--heapsnapshot-signal",
      "--icu-data-dir",
      "--import",
      "--input-type",
      "--inspect-brk",
      "--inspect-port",
      "--interactive",
      "--loader",
      "--max-http-header-size",
      "--network-family-autoselection",
      "--no-deprecation",
      "--no-node-snapshot",
      "--node-snapshot",
      "--openssl-config",
      "--openssl-legacy-provider",
      "--openssl-shared-config",
      "--pending-deprecation",
      "--permission",
      "--preserve-symlinks",
      "--preserve-symlinks-main",
      "--print",
      "--redirect-warnings",
      "--report-on-signal",
      "--require",
      "--run",
      "--secure-heap",
      "--secure-heap-min",
      "--stack-trace-limit",
      "--test",
      "--test-force-exit",
      "--test-global-setup",
      "--test-isolation",
      "--test-only",
      "--test-rerun-failures",
      "--test-shard",
      "--test-update-snapshots",
      "--throw-deprecation",
      "--tls-cipher-list",
      "--tls-keylog",
      "--trace-deprecation",
      "--trace-exit",
      "--trace-require-module",
      "--trace-sigint",
      "--trace-tls",
      "--trace-warnings",
      "--unhandled-rejections",
      "--use-bundled-ca",
      "--use-env-proxy",
      "--use-openssl-ca",
      "--use-system-ca",
      "--verify-base-objects",
      "--warnings",
      "--watch",
      "--watch-kill-signal",
      "--watch-path",
      "--watch-preserve-output",
  };
  return kKnownOptions.find(key) != kKnownOptions.end();
}

static napi_value OptionsGetCLIOptionsValuesCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = CreateNullProtoObject(env);
  if (out == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }

  const std::vector<const char*> bool_false = {
      "--abort-on-uncaught-exception",
      "--allow-addons",
      "--allow-child-process",
      "--allow-inspector",
      "--allow-wasi",
      "--allow-worker",
      "--check",
      "--enable-source-maps",
      "--entry-url",
      "--experimental-addon-modules",
      "--experimental-default-config-file",
      "--experimental-eventsource",
      "--experimental-fetch",
      "--experimental-global-customevent",
      "--experimental-global-webcrypto",
      "--experimental-import-meta-resolve",
      "--experimental-inspector-network-resource",
      "--experimental-network-inspection",
      "--experimental-print-required-tla",
      "--experimental-quic",
      "--no-experimental-quic",
      "--no-experimental-repl-await",
      "--experimental-report",
      "--experimental-strip-types",
      "--experimental-sqlite",
      "--no-experimental-sqlite",
      "--experimental-test-coverage",
      "--experimental-test-module-mocks",
      "--experimental-transform-types",
      "--experimental-vm-modules",
      "--experimental-wasm-modules",
      "--experimental-webstorage",
      "--experimental-worker",
      "--expose-internals",
      "--force-fips",
      "--frozen-intrinsics",
      "--insecure-http-parser",
      "--inspect-brk",
      "--interactive",
      "--no-addons",
      "--no-deprecation",
      "--no-experimental-global-navigator",
      "--no-experimental-websocket",
      "--node-snapshot",
      "--no-node-snapshot",
      "--openssl-legacy-provider",
      "--openssl-shared-config",
      "--pending-deprecation",
      "--permission",
      "--preserve-symlinks",
      "--preserve-symlinks-main",
      "--print",
      "--report-on-signal",
      "--strip-types",
      "--test",
      "--test-force-exit",
      "--test-only",
      "--test-update-snapshots",
      "--throw-deprecation",
      "--tls-max-v1.2",
      "--tls-max-v1.3",
      "--tls-min-v1.0",
      "--tls-min-v1.1",
      "--tls-min-v1.2",
      "--tls-min-v1.3",
      "--trace-deprecation",
      "--trace-sigint",
      "--trace-tls",
      "--trace-warnings",
      "--use-bundled-ca",
      "--use-env-proxy",
      "--use-openssl-ca",
      "--use-system-ca",
      "--verify-base-objects",
      "--no-verify-base-objects",
      "--watch",
      "--watch-preserve-output",
      "[has_eval_string]",
  };
  const std::vector<const char*> bool_true = {
      "--async-context-frame",
      "--experimental-detect-module",
      "--experimental-require-module",
      "--experimental-repl-await",
      "--network-family-autoselection",
      "--warnings",
  };
  const std::vector<std::pair<const char*, const char*>> string_defaults = {
      {"--diagnostic-dir", ""},
      {"--dns-result-order", "verbatim"},
      {"--es-module-specifier-resolution", ""},
      {"--eval", ""},
      {"--experimental-config-file", ""},
      {"--heapsnapshot-signal", ""},
      {"--icu-data-dir", ""},
      {"--input-type", ""},
      {"--localstorage-file", ""},
      {"--openssl-config", ""},
      {"--redirect-warnings", ""},
      {"--run", ""},
      {"--test-global-setup", ""},
      {"--test-isolation", "process"},
      {"--test-rerun-failures", ""},
      {"--test-shard", ""},
      {"--trace-require-module", ""},
      {"--trace-event-categories", ""},
      {"--trace-event-file-pattern", ""},
      {"--tls-cipher-list", EDGE_DEFAULT_CIPHER_LIST_CORE},
      {"--tls-keylog", ""},
      {"--unhandled-rejections", "throw"},
      {"--watch-kill-signal", "SIGTERM"},
  };
  const std::vector<std::pair<const char*, double>> number_defaults = {
      {"--heapsnapshot-near-heap-limit", 0},
      {"--max-http-header-size", 16 * 1024},
      {"--network-family-autoselection-attempt-timeout", 250},
      {"--secure-heap", 0},
      {"--secure-heap-min", 0},
      {"--stack-trace-limit", 10},
      {"--test-concurrency", 0},
      {"--test-coverage-branches", 0},
      {"--test-coverage-functions", 0},
      {"--test-coverage-lines", 0},
      {"--test-timeout", 0},
  };
  const std::vector<const char*> array_defaults = {
      "--allow-fs-read",
      "--allow-fs-write",
      "--conditions",
      "--disable-warning",
      "--env-file",
      "--env-file-if-exists",
      "--experimental-loader",
      "--import",
      "--loader",
      "--require",
      "--test-coverage-exclude",
      "--test-coverage-include",
      "--test-name-pattern",
      "--test-reporter",
      "--test-reporter-destination",
      "--test-skip-pattern",
      "--watch-path",
  };

  for (const char* key : bool_false) SetBoolProperty(env, out, key, false);
  for (const char* key : bool_true) SetBoolProperty(env, out, key, true);
  for (const auto& [key, value] : string_defaults) SetStringProperty(env, out, key, value);
  for (const auto& [key, value] : number_defaults) SetNumberProperty(env, out, key, value);
  for (const char* key : array_defaults) SetArrayProperty(env, out, key);

  const std::unordered_map<std::string, bool> array_option_set = {
      {"--allow-fs-read", true},
      {"--allow-fs-write", true},
      {"--conditions", true},
      {"--disable-warning", true},
      {"--env-file", true},
      {"--env-file-if-exists", true},
      {"--experimental-loader", true},
      {"--import", true},
      {"--require", true},
      {"--test-coverage-exclude", true},
      {"--test-coverage-include", true},
      {"--test-name-pattern", true},
      {"--test-reporter", true},
      {"--test-reporter-destination", true},
      {"--test-skip-pattern", true},
      {"--watch-path", true},
  };
  std::unordered_set<std::string> bool_option_set;
  bool_option_set.reserve(bool_false.size() + bool_true.size());
  for (const char* key : bool_false) bool_option_set.emplace(key);
  for (const char* key : bool_true) bool_option_set.emplace(key);
  std::unordered_set<std::string> string_option_set;
  string_option_set.reserve(string_defaults.size());
  for (const auto& [key, _] : string_defaults) string_option_set.emplace(key);
  std::unordered_set<std::string> number_option_set;
  number_option_set.reserve(number_defaults.size());
  for (const auto& [key, _] : number_defaults) number_option_set.emplace(key);
  bool saw_pending_deprecation_cli = false;
  bool saw_preserve_symlinks_cli = false;
  bool saw_preserve_symlinks_main_cli = false;
  bool saw_redirect_warnings_cli = false;
  bool saw_use_env_proxy_cli = false;

  const std::vector<std::string> tokens = GetEffectiveCliTokens(env);
  for (size_t i = 0; i < tokens.size(); i++) {
    const std::string& token = tokens[i];
    if (token.empty() || token[0] != '-') continue;

    if (token == "-pe" || token == "-ep") {
      SetBoolProperty(env, out, "--print", true);
      if (i + 1 < tokens.size()) {
        const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[i + 1]);
        SetStringProperty(env, out, "--eval", value.c_str());
        SetBoolProperty(env, out, "[has_eval_string]", true);
        i++;
      }
      continue;
    }
    if (token == "-p" || token == "--print") {
      SetBoolProperty(env, out, "--print", true);
      if (i + 1 < tokens.size() && !LooksLikeCliOptionToken(tokens[i + 1])) {
        const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[i + 1]);
        SetStringProperty(env, out, "--eval", value.c_str());
        SetBoolProperty(env, out, "[has_eval_string]", true);
        i++;
      }
      continue;
    }
    if (token.rfind("--print=", 0) == 0) {
      SetBoolProperty(env, out, "--print", true);
      const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(token.substr(8));
      SetStringProperty(env, out, "--eval", value.c_str());
      SetBoolProperty(env, out, "[has_eval_string]", true);
      continue;
    }
    if (token == "-e" || token == "--eval") {
      if (i + 1 < tokens.size()) {
        const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[i + 1]);
        SetStringProperty(env, out, "--eval", value.c_str());
        SetBoolProperty(env, out, "[has_eval_string]", true);
        i++;
      }
      continue;
    }
    if (token.rfind("--eval=", 0) == 0) {
      const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(token.substr(7));
      SetStringProperty(env, out, "--eval", value.c_str());
      SetBoolProperty(env, out, "[has_eval_string]", true);
      continue;
    }
    if (token == "-c") {
      SetBoolProperty(env, out, "--check", true);
      continue;
    }
    if (token == "-i") {
      SetBoolProperty(env, out, "--interactive", true);
      continue;
    }

    const size_t eq = token.find('=');
    std::string key = (eq == std::string::npos) ? token : token.substr(0, eq);
    const std::string raw = (eq == std::string::npos) ? "" : token.substr(eq + 1);

    if (key == "-r") key = "--require";
    if (key == "--loader") key = "--experimental-loader";
    if (key == "--experimental-strip-types") key = "--strip-types";
    if (key == "--no-experimental-strip-types") key = "--no-strip-types";

    if (eq == std::string::npos && key.rfind("--no-", 0) == 0) {
      if (bool_option_set.find(key) != bool_option_set.end()) {
        SetBoolProperty(env, out, key.c_str(), true);
        continue;
      }
      const std::string normalized = "--" + key.substr(5);
      if (bool_option_set.find(normalized) != bool_option_set.end()) {
        SetBoolProperty(env, out, normalized.c_str(), false);
        saw_pending_deprecation_cli = saw_pending_deprecation_cli || normalized == "--pending-deprecation";
        saw_preserve_symlinks_cli = saw_preserve_symlinks_cli || normalized == "--preserve-symlinks";
        saw_preserve_symlinks_main_cli =
            saw_preserve_symlinks_main_cli || normalized == "--preserve-symlinks-main";
        if (normalized == "--use-env-proxy") saw_use_env_proxy_cli = true;
      }
      continue;
    }

    if (array_option_set.find(key) != array_option_set.end()) {
      napi_value arr = nullptr;
      if (napi_get_named_property(env, out, key.c_str(), &arr) != napi_ok || arr == nullptr) continue;
      if (eq != std::string::npos) {
        PushArrayString(env, arr, raw);
      } else if (i + 1 < tokens.size()) {
        PushArrayString(env, arr, edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[++i]));
      }
      continue;
    }

    if (eq == std::string::npos) {
      if (bool_option_set.find(key) != bool_option_set.end()) {
        SetBoolProperty(env, out, key.c_str(), true);
        saw_pending_deprecation_cli = saw_pending_deprecation_cli || key == "--pending-deprecation";
        saw_preserve_symlinks_cli = saw_preserve_symlinks_cli || key == "--preserve-symlinks";
        saw_preserve_symlinks_main_cli =
            saw_preserve_symlinks_main_cli || key == "--preserve-symlinks-main";
        if (key == "--use-env-proxy") saw_use_env_proxy_cli = true;
        continue;
      }
      if (string_option_set.find(key) != string_option_set.end()) {
        if (i + 1 < tokens.size()) {
          const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[++i]);
          SetStringProperty(env, out, key.c_str(), value.c_str());
          saw_redirect_warnings_cli = saw_redirect_warnings_cli || key == "--redirect-warnings";
        }
        continue;
      }
      if (number_option_set.find(key) != number_option_set.end()) {
        if (i + 1 < tokens.size()) {
          double numeric = 0;
          const std::string value = edge_options::MaybeUnescapeLeadingDashOptionValue(tokens[++i]);
          if (TryParseDouble(value, &numeric)) {
            SetNumberProperty(env, out, key.c_str(), numeric);
          }
        }
        continue;
      }
      continue;
    }

    if (bool_option_set.find(key) != bool_option_set.end()) {
      SetBoolProperty(env, out, key.c_str(), raw != "false" && raw != "0");
      saw_pending_deprecation_cli = saw_pending_deprecation_cli || key == "--pending-deprecation";
      saw_preserve_symlinks_cli = saw_preserve_symlinks_cli || key == "--preserve-symlinks";
      saw_preserve_symlinks_main_cli =
          saw_preserve_symlinks_main_cli || key == "--preserve-symlinks-main";
      if (key == "--use-env-proxy") saw_use_env_proxy_cli = true;
      continue;
    }
    if (number_option_set.find(key) != number_option_set.end()) {
      double numeric = 0;
      if (TryParseDouble(raw, &numeric)) {
        SetNumberProperty(env, out, key.c_str(), numeric);
      }
      continue;
    }
    if (string_option_set.find(key) != string_option_set.end()) {
      SetStringProperty(env, out, key.c_str(), raw.c_str());
      saw_redirect_warnings_cli = saw_redirect_warnings_cli || key == "--redirect-warnings";
    }
  }

  if (!saw_pending_deprecation_cli && EnvironmentOptionEnabled(env, "NODE_PENDING_DEPRECATION")) {
    SetBoolProperty(env, out, "--pending-deprecation", true);
  }
  if (!saw_preserve_symlinks_cli && EnvironmentOptionEnabled(env, "NODE_PRESERVE_SYMLINKS")) {
    SetBoolProperty(env, out, "--preserve-symlinks", true);
  }
  if (!saw_preserve_symlinks_main_cli &&
      EnvironmentOptionEnabled(env, "NODE_PRESERVE_SYMLINKS_MAIN")) {
    SetBoolProperty(env, out, "--preserve-symlinks-main", true);
  }
  if (!saw_redirect_warnings_cli) {
    const std::string redirect_warnings = GetEnvironmentOptionValue(env, "NODE_REDIRECT_WARNINGS");
    if (!redirect_warnings.empty()) {
      SetStringProperty(env, out, "--redirect-warnings", redirect_warnings.c_str());
    }
  }
  if (EnvironmentOptionEnabled(env, "NODE_USE_ENV_PROXY") && !saw_use_env_proxy_cli) {
    SetBoolProperty(env, out, "--use-env-proxy", true);
  }

  return out;
}

static napi_value OptionsGetCLIOptionsInfoCallback(napi_env env, napi_callback_info /*info*/) {
  constexpr int32_t kAllowedInEnvvar = 0;
  constexpr int32_t kString = 5;

  napi_value options_map = CreateMapObject(env);
  napi_value aliases_map = CreateMapObject(env);
  if (options_map == nullptr || aliases_map == nullptr) return UndefinedValue(env);

  auto make_info = [&](napi_value* out) -> bool {
    if (out == nullptr) return false;
    napi_value info = nullptr;
    if (napi_create_object(env, &info) != napi_ok || info == nullptr) return false;
    if (!SetNumberProperty(env, info, "envVarSettings", kAllowedInEnvvar) ||
        !SetNumberProperty(env, info, "type", kString)) {
      return false;
    }
    *out = info;
    return true;
  };

  std::unordered_map<std::string, bool> seen;
  auto add_option = [&](const std::string& name) {
    if (name.empty() || seen.find(name) != seen.end()) return;
    napi_value info = nullptr;
    if (!make_info(&info)) return;
    if (MapSetStringKeyValue(env, options_map, name.c_str(), info)) {
      seen.emplace(name, true);
    }
  };

  const std::vector<std::string> documented = ParseAllowedFlagsFromDocs();
  for (const auto& opt : documented) add_option(opt);

  const std::vector<std::string> extras = {
      // Keep alias targets present even when CLI docs are unavailable from cwd.
      "--require",
      "--debug-arraybuffer-allocations",
      "--no-debug-arraybuffer-allocations",
      "--es-module-specifier-resolution",
      "--experimental-detect-module",
      "--no-experimental-detect-module",
      "--experimental-fetch",
      "--experimental-require-module",
      "--no-experimental-require-module",
      "--experimental-wasm-modules",
      "--experimental-global-customevent",
      "--experimental-global-webcrypto",
      "--experimental-report",
      "--experimental-repl-await",
      "--experimental-strip-types",
      "--no-experimental-strip-types",
      "--experimental-worker",
      "--node-snapshot",
      "--no-node-snapshot",
      "--loader",
      "--verify-base-objects",
      "--no-verify-base-objects",
      "--trace-promises",
      "--no-trace-promises",
      "--experimental-quic",
  };
  for (const auto& opt : extras) add_option(opt);

  napi_value process = GetGlobalNamedProperty(env, "process");
  if (IsTruthyNestedProperty(env, process, {"config", "variables", "node_quic"})) {
    add_option("--no-experimental-quic");
  }

  if (!IsTruthyNestedProperty(env, process, {"features", "inspector"})) {
    const std::vector<std::string> inspector_only = {
        "--cpu-prof-dir",
        "--cpu-prof-interval",
        "--cpu-prof-name",
        "--cpu-prof",
        "--heap-prof-dir",
        "--heap-prof-interval",
        "--heap-prof-name",
        "--heap-prof",
        "--inspect-brk",
        "--inspect-port",
        "--debug-port",
        "--inspect-publish-uid",
        "--inspect-wait",
        "--inspect",
    };
    for (const auto& opt : inspector_only) {
      seen.erase(opt);
    }
    // Rebuild map without deleted options to keep parity with previous behavior.
    napi_value rebuilt = CreateMapObject(env);
    if (rebuilt == nullptr) return UndefinedValue(env);
    for (const auto& name : documented) {
      if (seen.find(name) == seen.end()) continue;
      napi_value info = nullptr;
      if (!make_info(&info)) continue;
      MapSetStringKeyValue(env, rebuilt, name.c_str(), info);
    }
    for (const auto& name : extras) {
      if (seen.find(name) == seen.end()) continue;
      napi_value info = nullptr;
      if (!make_info(&info)) continue;
      MapSetStringKeyValue(env, rebuilt, name.c_str(), info);
    }
    if (seen.find("--no-experimental-quic") != seen.end()) {
      napi_value info = nullptr;
      if (make_info(&info)) MapSetStringKeyValue(env, rebuilt, "--no-experimental-quic", info);
    }
    options_map = rebuilt;
  }

  add_option("--perf-basic-prof");
  add_option("--stack-trace-limit");
  add_option("-r");

  napi_value require_alias = nullptr;
  if (napi_create_array_with_length(env, 1, &require_alias) == napi_ok && require_alias != nullptr) {
    napi_value require_string = nullptr;
    if (napi_create_string_utf8(env, "--require", NAPI_AUTO_LENGTH, &require_string) == napi_ok &&
        require_string != nullptr) {
      napi_set_element(env, require_alias, 0, require_string);
      MapSetStringKeyValue(env, aliases_map, "-r", require_alias);
    }
  }

  napi_value out = CreateNullProtoObject(env);
  if (out == nullptr) return UndefinedValue(env);
  napi_set_named_property(env, out, "options", options_map);
  napi_set_named_property(env, out, "aliases", aliases_map);
  return out;
}

static napi_value OptionsGetOptionsAsFlagsCallback(napi_env env, napi_callback_info /*info*/) {
  std::vector<std::string> exec_argv = GetEffectiveCliTokens(env);
  if (EnvironmentOptionEnabled(env, "NODE_USE_ENV_PROXY")) {
    const bool has_use_env_proxy =
        std::find(exec_argv.begin(), exec_argv.end(), "--use-env-proxy") != exec_argv.end();
    const bool has_no_use_env_proxy =
        std::find(exec_argv.begin(), exec_argv.end(), "--no-use-env-proxy") != exec_argv.end();
    if (!has_use_env_proxy && !has_no_use_env_proxy) {
      exec_argv.push_back("--use-env-proxy");
    }
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, exec_argv.size(), &out) != napi_ok || out == nullptr) {
    return UndefinedValue(env);
  }
  for (size_t i = 0; i < exec_argv.size(); i++) {
    napi_value item = nullptr;
    if (napi_create_string_utf8(env, exec_argv[i].c_str(), exec_argv[i].size(), &item) != napi_ok ||
        item == nullptr) {
      continue;
    }
    napi_set_element(env, out, i, item);
  }
  return out;
}

static napi_value OptionsGetEmbedderOptionsCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = CreateNullProtoObject(env);
  if (out == nullptr) return UndefinedValue(env);
  SetBoolProperty(env, out, "shouldNotRegisterESMLoader", false);
  SetBoolProperty(env, out, "noGlobalSearchPaths", false);
  SetBoolProperty(env, out, "noBrowserGlobals", false);
  SetBoolProperty(env, out, "hasEmbedderPreload", false);
  return out;
}

static napi_value OptionsGetEnvOptionsInputTypeCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value map = CreateMapObject(env);
  if (map == nullptr) return UndefinedValue(env);

  const std::vector<std::string> string_opts = {
      "--conditions",
      "--disable-warning",
      "--diagnostic-dir",
      "--dns-result-order",
      "--env-file",
      "--env-file-if-exists",
      "--experimental-loader",
      "--import",
      "--input-type",
      "--max-http-header-size",
      "--network-family-autoselection-attempt-timeout",
      "--redirect-warnings",
      "--require",
      "--secure-heap",
      "--secure-heap-min",
      "--stack-trace-limit",
      "--test-concurrency",
      "--test-coverage-branches",
      "--test-coverage-exclude",
      "--test-coverage-functions",
      "--test-coverage-include",
      "--test-coverage-lines",
      "--test-global-setup",
      "--test-name-pattern",
      "--test-reporter",
      "--test-reporter-destination",
      "--test-rerun-failures",
      "--test-shard",
      "--test-skip-pattern",
      "--test-timeout",
      "--trace-require-module",
      "--trace-event-categories",
      "--trace-event-file-pattern",
      "--tls-cipher-list",
      "--tls-keylog",
      "--unhandled-rejections",
      "--watch-kill-signal",
      "--watch-path",
  };

  for (const auto& opt : string_opts) {
    napi_value type_v = nullptr;
    if (napi_create_string_utf8(env, "string", NAPI_AUTO_LENGTH, &type_v) != napi_ok || type_v == nullptr) {
      continue;
    }
    MapSetStringKeyValue(env, map, opt.c_str(), type_v);
  }
  return map;
}

static napi_value OptionsGetNamespaceOptionsInputTypeCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value map = CreateMapObject(env);
  if (map == nullptr) return UndefinedValue(env);
  return map;
}

static napi_value ModulesReadPackageJSONCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string path_input = ValueToUtf8(env, argv[0]);
  if (path_input.empty()) return UndefinedValue(env);
  std::optional<fs::path> maybe_path = ParsePathOrFileUrl(path_input);
  if (!maybe_path.has_value()) return UndefinedValue(env);

  SerializedPackageConfigData package_config;
  const ParsePackageStatus status = ParseSerializedPackageConfig(*maybe_path, &package_config);
  if (status == ParsePackageStatus::kMissing) {
    return UndefinedValue(env);
  }
  if (status == ParsePackageStatus::kInvalid) {
    napi_valuetype is_esm_type = napi_undefined;
    bool is_esm = false;
    if (argc >= 2 && argv[1] != nullptr && napi_typeof(env, argv[1], &is_esm_type) == napi_ok &&
        is_esm_type == napi_boolean) {
      (void)napi_get_value_bool(env, argv[1], &is_esm);
    }
    if (is_esm && argc >= 4 && argv[3] != nullptr) {
      const std::string specifier = ValueToUtf8(env, argv[3]);
      std::string base = ValueToUtf8(env, argv[2]);
      if (std::optional<fs::path> base_path = ParsePathOrFileUrl(base); base_path.has_value()) {
        base = base_path->lexically_normal().string();
      }
      ThrowInvalidPackageConfig(env, maybe_path->lexically_normal().string(), &base, &specifier);
    } else {
      ThrowInvalidPackageConfig(env, maybe_path->lexically_normal().string());
    }
    return nullptr;
  }
  napi_value out = SerializePackageConfigToArray(env, package_config);
  return out != nullptr ? out : UndefinedValue(env);
}

static napi_value ModulesGetNearestParentPackageJSONTypeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string check_path_input = ValueToUtf8(env, argv[0]);
  std::optional<fs::path> check_path = ParsePathOrFileUrl(check_path_input);
  if (!check_path.has_value()) return UndefinedValue(env);

  SerializedPackageConfigData package_config;
  bool invalid = false;
  if (!TraverseNearestParentPackageConfig(*check_path, &package_config, &invalid)) {
    if (invalid) {
      ThrowInvalidPackageConfig(env, (check_path->parent_path() / "package.json").lexically_normal().string());
      return nullptr;
    }
    return UndefinedValue(env);
  }

  napi_value out = nullptr;
  if (napi_create_string_utf8(env, package_config.type.c_str(), package_config.type.size(), &out) != napi_ok ||
      out == nullptr) {
    return UndefinedValue(env);
  }
  return out;
}

static napi_value ModulesGetNearestParentPackageJSONCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string check_path_input = ValueToUtf8(env, argv[0]);
  std::optional<fs::path> check_path = ParsePathOrFileUrl(check_path_input);
  if (!check_path.has_value()) return UndefinedValue(env);

  SerializedPackageConfigData package_config;
  bool invalid = false;
  if (!TraverseNearestParentPackageConfig(*check_path, &package_config, &invalid)) {
    if (invalid) {
      ThrowInvalidPackageConfig(env, (check_path->parent_path() / "package.json").lexically_normal().string());
      return nullptr;
    }
    return UndefinedValue(env);
  }

  napi_value out = SerializePackageConfigToArray(env, package_config);
  return out != nullptr ? out : UndefinedValue(env);
}

static napi_value ModulesGetPackageScopeConfigCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string resolved_input = ValueToUtf8(env, argv[0]);
  std::optional<fs::path> resolved_path = ParsePathOrFileUrl(resolved_input);
  if (!resolved_path.has_value()) return UndefinedValue(env);

  SerializedPackageConfigData package_config;
  fs::path unresolved_package_json;
  bool invalid = false;
  const bool found = ResolvePackageScopeFromPath(*resolved_path, &package_config, &unresolved_package_json, &invalid);
  if (invalid) {
    ThrowInvalidPackageConfig(
        env,
        unresolved_package_json.empty()
            ? (*resolved_path / "package.json").lexically_normal().string()
            : unresolved_package_json.lexically_normal().string());
    return nullptr;
  }

  if (found) {
    napi_value out = SerializePackageConfigToArray(env, package_config);
    return out != nullptr ? out : UndefinedValue(env);
  }

  if (unresolved_package_json.empty()) {
    unresolved_package_json = resolved_path->parent_path() / "package.json";
  }
  const std::string unresolved = unresolved_package_json.lexically_normal().string();
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, unresolved.c_str(), unresolved.size(), &out) != napi_ok || out == nullptr) {
    return UndefinedValue(env);
  }
  return out;
}

static napi_value ModulesGetPackageTypeCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok || argc < 1 || argv[0] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string resolved_input = ValueToUtf8(env, argv[0]);
  std::optional<fs::path> resolved_path = ParsePathOrFileUrl(resolved_input);
  if (!resolved_path.has_value()) return UndefinedValue(env);

  SerializedPackageConfigData package_config;
  fs::path unresolved_package_json;
  bool invalid = false;
  const bool found = ResolvePackageScopeFromPath(*resolved_path, &package_config, &unresolved_package_json, &invalid);
  if (invalid) {
    ThrowInvalidPackageConfig(
        env,
        unresolved_package_json.empty()
            ? (*resolved_path / "package.json").lexically_normal().string()
            : unresolved_package_json.lexically_normal().string());
    return nullptr;
  }
  if (!found) return UndefinedValue(env);

  napi_value out = nullptr;
  if (napi_create_string_utf8(env, package_config.type.c_str(), package_config.type.size(), &out) != napi_ok ||
      out == nullptr) {
    return UndefinedValue(env);
  }
  return out;
}

static napi_value ModulesEnableCompileCacheCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, 3, &out) != napi_ok || out == nullptr) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    return undefined;
  }
  napi_value status = nullptr;
  // CompileCacheEnableStatus::DISABLED
  napi_create_int32(env, 3, &status);
  napi_set_element(env, out, 0, status);
  napi_value empty = nullptr;
  napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty);
  napi_set_element(env, out, 1, empty);
  if (argc >= 1 && argv[0] != nullptr) {
    napi_set_element(env, out, 2, argv[0]);
  } else {
    napi_set_element(env, out, 2, empty);
  }
  return out;
}

static napi_value ModulesGetCompileCacheDirCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &out) != napi_ok || out == nullptr) {
    napi_get_undefined(env, &out);
  }
  return out;
}

static napi_value ModulesFlushCompileCacheCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value ModulesGetCompileCacheEntryCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value ModulesSaveCompileCacheEntryCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value ModulesSetLazyPathHelpersCallback(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2] = {nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return UndefinedValue(env);
  }
  if (argc < 2 || argv[0] == nullptr || argv[1] == nullptr) {
    return UndefinedValue(env);
  }

  const std::string url = ValueToUtf8(env, argv[1]);
  if (url.empty()) return UndefinedValue(env);

  const std::string filename = edge_path::NormalizeFileURLOrPath(url);
  if (filename.empty()) return UndefinedValue(env);
  const std::string dirname = fs::path(filename).parent_path().string();

  napi_value filename_value = nullptr;
  napi_value dirname_value = nullptr;
  if (napi_create_string_utf8(env, filename.c_str(), filename.size(), &filename_value) != napi_ok ||
      filename_value == nullptr ||
      napi_create_string_utf8(env, dirname.c_str(), dirname.size(), &dirname_value) != napi_ok ||
      dirname_value == nullptr) {
    return UndefinedValue(env);
  }

  napi_set_named_property(env, argv[0], "dirname", dirname_value);
  napi_set_named_property(env, argv[0], "filename", filename_value);

  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value GetGlobalInternalBindingFunction(napi_env env, napi_value global) {
  if (env == nullptr || global == nullptr) return nullptr;

  auto lookup = [&](const char* key) -> napi_value {
    napi_value fn = nullptr;
    napi_valuetype type = napi_undefined;
    if (napi_get_named_property(env, global, key, &fn) != napi_ok ||
        fn == nullptr ||
        napi_typeof(env, fn, &type) != napi_ok ||
        type != napi_function) {
      return nullptr;
    }
    return fn;
  };

  if (napi_value fn = lookup("internalBinding"); fn != nullptr) return fn;
  return lookup("getInternalBinding");
}

static napi_value GetUtilPrivateSymbolByName(napi_env env, const char* key_name) {
  if (env == nullptr || key_name == nullptr) return nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value internal_binding = GetGlobalInternalBindingFunction(env, global);
  if (internal_binding == nullptr) return nullptr;

  napi_value util_name = nullptr;
  if (napi_create_string_utf8(env, "util", NAPI_AUTO_LENGTH, &util_name) != napi_ok || util_name == nullptr) {
    return nullptr;
  }
  napi_value util_binding = nullptr;
  if (napi_call_function(env, global, internal_binding, 1, &util_name, &util_binding) != napi_ok ||
      util_binding == nullptr ||
      IsUndefinedValue(env, util_binding)) {
    return nullptr;
  }

  napi_value private_symbols = nullptr;
  if (napi_get_named_property(env, util_binding, "privateSymbols", &private_symbols) != napi_ok ||
      private_symbols == nullptr ||
      IsUndefinedValue(env, private_symbols)) {
    return nullptr;
  }

  napi_value symbol = nullptr;
  if (napi_get_named_property(env, private_symbols, key_name, &symbol) != napi_ok || symbol == nullptr ||
      IsUndefinedValue(env, symbol)) {
    return nullptr;
  }
  return symbol;
}

static napi_value GetPerIsolateSymbolByName(napi_env env, const char* key_name) {
  if (env == nullptr || key_name == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;

  napi_value per_isolate_symbols = GetStatePerIsolateSymbols(env, state);
  if (per_isolate_symbols == nullptr || IsUndefinedValue(env, per_isolate_symbols)) {
    return nullptr;
  }

  napi_value symbol = nullptr;
  if (napi_get_named_property(env, per_isolate_symbols, key_name, &symbol) != napi_ok || symbol == nullptr ||
      IsUndefinedValue(env, symbol)) {
    return nullptr;
  }
  return symbol;
}

static void SetHostDefinedOptionSymbol(napi_env env, napi_value target, napi_value id_value) {
  if (env == nullptr || target == nullptr) return;
  napi_value symbol = GetUtilPrivateSymbolByName(env, "host_defined_option_symbol");
  if (symbol == nullptr || IsUndefinedValue(env, symbol)) return;
  napi_value value = id_value;
  if (value == nullptr) napi_get_undefined(env, &value);
  napi_set_property(env, target, symbol, value);
}

static void SetContextifyHostDefinedOption(napi_env env, napi_value target, napi_value id_value) {
  if (env == nullptr || target == nullptr) return;
  napi_value value = id_value;
  if (value == nullptr) napi_get_undefined(env, &value);
  SetHostDefinedOptionSymbol(env, target, value);
  napi_set_named_property(env, target, "contextifyHostDefinedOptionId", value);
}

static void ClearContextifyHostDefinedOptionProperties(napi_env env, napi_value target) {
  if (env == nullptr || target == nullptr) return;
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  SetContextifyHostDefinedOption(env, target, undefined);
}

static ContextifyScriptInstance* TrackContextifyScript(napi_env env,
                                                       napi_value wrapper,
                                                       napi_value host_defined_option_id) {
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || wrapper == nullptr || host_defined_option_id == nullptr ||
      IsUndefinedValue(env, host_defined_option_id)) {
    return nullptr;
  }

  auto* instance = new (std::nothrow) ContextifyScriptInstance();
  if (instance == nullptr) return nullptr;
  instance->state = state;

  if (napi_create_reference(env, wrapper, 0, &instance->wrapper_ref) != napi_ok ||
      napi_create_reference(env, host_defined_option_id, 1, &instance->host_defined_option_ref) != napi_ok ||
      napi_wrap(env, wrapper, instance, ContextifyScriptFinalize, nullptr, nullptr) != napi_ok) {
    DestroyContextifyScriptInstance(env, instance, true);
    return nullptr;
  }

  state->contextify_scripts.push_back(instance);
  return instance;
}

static napi_value GetContextifyHostDefinedOption(napi_env env, napi_value wrapper) {
  if (env == nullptr || wrapper == nullptr) return nullptr;
  void* data = nullptr;
  if (napi_unwrap(env, wrapper, &data) == napi_ok && data != nullptr) {
    auto* instance = static_cast<ContextifyScriptInstance*>(data);
    if (instance->host_defined_option_ref != nullptr) {
      napi_value out = nullptr;
      if (napi_get_reference_value(env, instance->host_defined_option_ref, &out) == napi_ok &&
          out != nullptr) {
        return out;
      }
    }
  }
  return GetNamedPropertyOrUndefined(env, wrapper, "contextifyHostDefinedOptionId");
}

static napi_value GetNamedPropertyOrUndefined(napi_env env, napi_value object, const char* key) {
  napi_value value = nullptr;
  if (object != nullptr && napi_get_named_property(env, object, key, &value) == napi_ok && value != nullptr) {
    return value;
  }
  napi_get_undefined(env, &value);
  return value;
}

static napi_value CreateContextifyCjsLoaderResult(napi_env env,
                                                  napi_value compile_result_or_null,
                                                  bool can_parse_as_esm) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return nullptr;

  napi_value function_value = GetNamedPropertyOrUndefined(env, compile_result_or_null, "function");
  napi_value source_url = GetNamedPropertyOrUndefined(env, compile_result_or_null, "sourceURL");
  napi_value source_map_url = GetNamedPropertyOrUndefined(env, compile_result_or_null, "sourceMapURL");
  napi_value cached_data_rejected = nullptr;
  napi_value can_parse_value = nullptr;
  if (napi_get_boolean(env, false, &cached_data_rejected) != napi_ok || cached_data_rejected == nullptr ||
      napi_get_boolean(env, can_parse_as_esm, &can_parse_value) != napi_ok || can_parse_value == nullptr) {
    return nullptr;
  }

  if (napi_set_named_property(env, out, "function", function_value) != napi_ok ||
      napi_set_named_property(env, out, "sourceURL", source_url) != napi_ok ||
      napi_set_named_property(env, out, "sourceMapURL", source_map_url) != napi_ok ||
      napi_set_named_property(env, out, "cachedDataRejected", cached_data_rejected) != napi_ok ||
      napi_set_named_property(env, out, "canParseAsESM", can_parse_value) != napi_ok) {
    return nullptr;
  }

  return out;
}

// Extracts the bytes of any ArrayBufferView (typed array or DataView) —
// Node's vm cachedData accepts every view flavor over the same buffer.
static bool GetTypedArrayBytes(napi_env env, napi_value value, const uint8_t** data_out, size_t* len_out) {
  *data_out = nullptr;
  *len_out = 0;
  if (value == nullptr) return false;

  bool is_typedarray = false;
  if (napi_is_typedarray(env, value, &is_typedarray) == napi_ok && is_typedarray) {
    napi_typedarray_type type = napi_uint8_array;
    size_t length = 0;
    void* data = nullptr;
    if (napi_get_typedarray_info(env, value, &type, &length, &data, nullptr, nullptr) != napi_ok ||
        data == nullptr) {
      return false;
    }
    size_t element_size = 1;
    switch (type) {
      case napi_int8_array:
      case napi_uint8_array:
      case napi_uint8_clamped_array:
        element_size = 1;
        break;
      case napi_int16_array:
      case napi_uint16_array:
      case napi_float16_array:
        element_size = 2;
        break;
      case napi_int32_array:
      case napi_uint32_array:
      case napi_float32_array:
        element_size = 4;
        break;
      case napi_float64_array:
      case napi_bigint64_array:
      case napi_biguint64_array:
        element_size = 8;
        break;
      default:
        return false;
    }
    *data_out = static_cast<const uint8_t*>(data);
    *len_out = length * element_size;
    return true;
  }

  bool is_dataview = false;
  if (napi_is_dataview(env, value, &is_dataview) == napi_ok && is_dataview) {
    size_t byte_length = 0;
    void* data = nullptr;
    if (napi_get_dataview_info(env, value, &byte_length, &data, nullptr, nullptr) != napi_ok ||
        data == nullptr) {
      return false;
    }
    *data_out = static_cast<const uint8_t*>(data);
    *len_out = byte_length;
    return true;
  }
  return false;
}

// vm APIs promise node Buffers for cachedData; bytecode_serialize guarantees
// only a Uint8Array (engine-dependent). Wrap via Buffer.from when needed.
static napi_value EnsureNodeBuffer(napi_env env, napi_value view) {
  if (view == nullptr) return nullptr;
  napi_value global = nullptr;
  napi_value buffer_ctor = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "Buffer", &buffer_ctor) != napi_ok ||
      buffer_ctor == nullptr) {
    return view;
  }
  bool is_buffer = false;
  if (napi_instanceof(env, view, buffer_ctor, &is_buffer) == napi_ok && is_buffer) {
    return view;
  }
  napi_value from_fn = nullptr;
  napi_value wrapped = nullptr;
  if (napi_get_named_property(env, buffer_ctor, "from", &from_fn) != napi_ok || from_fn == nullptr ||
      napi_call_function(env, buffer_ctor, from_fn, 1, &view, &wrapped) != napi_ok ||
      wrapped == nullptr) {
    return view;
  }
  return wrapped;
}

// Compiles script text and returns its serialized code cache (replaces the
// removed unofficial_napi_contextify_create_cached_data). Leaves the compile
// exception pending on syntax errors.
static bool CreateScriptCachedDataBuffer(napi_env env,
                                         napi_value code,
                                         napi_value filename,
                                         int32_t line_offset,
                                         int32_t column_offset,
                                         napi_value* buffer_out) {
  *buffer_out = nullptr;
  void* bytecode = nullptr;
  if (unofficial_napi_bytecode_compile(env,
                                       code,
                                       filename,
                                       unofficial_napi_bytecode_shape_script,
                                       nullptr,
                                       nullptr,
                                       line_offset,
                                       column_offset,
                                       &bytecode,
                                       nullptr) != napi_ok ||
      bytecode == nullptr) {
    return false;
  }
  const napi_status status = unofficial_napi_bytecode_serialize(env, bytecode, buffer_out);
  (void)unofficial_napi_bytecode_release(env, bytecode);
  if (status != napi_ok || *buffer_out == nullptr) return false;
  // The provider's serialized bytes self-validate (V8 CachedData / QuickJS
  // QJSB header); just hand back a Node Buffer.
  *buffer_out = EnsureNodeBuffer(env, *buffer_out);
  return *buffer_out != nullptr;
}

static napi_value ContextifyScriptConstructorCallback(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value argv[8] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  napi_value code = nullptr;
  if (argc >= 1 && argv[0] != nullptr) {
    napi_coerce_to_string(env, argv[0], &code);
  }
  if (code == nullptr) {
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &code);
  }

  napi_value filename = nullptr;
  if (argc >= 2 && argv[1] != nullptr) {
    napi_coerce_to_string(env, argv[1], &filename);
  }
  if (filename == nullptr) {
    napi_create_string_utf8(env, "[eval]", NAPI_AUTO_LENGTH, &filename);
  }

  napi_set_named_property(env, this_arg, "contextifyCode", code);
  napi_set_named_property(env, this_arg, "contextifyFilename", filename);
  int32_t line_offset = 0;
  int32_t column_offset = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_set_named_property(env, this_arg, "contextifyLineOffset", argv[2]);
    napi_get_value_int32(env, argv[2], &line_offset);
  }
  if (argc >= 4 && argv[3] != nullptr) {
    napi_set_named_property(env, this_arg, "contextifyColumnOffset", argv[3]);
    napi_get_value_int32(env, argv[3], &column_offset);
  }
  napi_set_named_property(env, this_arg, "sourceURL", filename);
  napi_value host_defined_option_id = nullptr;
  if (argc >= 8) host_defined_option_id = argv[7];
  if (host_defined_option_id != nullptr) {
    SetContextifyHostDefinedOption(env, this_arg, host_defined_option_id);
    TrackContextifyScript(env, this_arg, host_defined_option_id);
  } else {
    SetHostDefinedOptionSymbol(env, this_arg, host_defined_option_id);
  }

  const bool has_cached_data_arg =
      argc >= 5 && argv[4] != nullptr && !IsUndefinedValue(env, argv[4]);
  bool produce_cached_data = false;
  if (argc >= 6 && argv[5] != nullptr) {
    napi_get_value_bool(env, argv[5], &produce_cached_data);
  }

  // Validate user cachedData (and report rejection honestly), then compile —
  // which doubles as Node's constructor-time syntax validation.
  bool cached_data_rejected = false;
  void* script_bytecode = nullptr;
  if (has_cached_data_arg) {
    const uint8_t* bytes = nullptr;
    size_t length = 0;
    // The provider validates source/shape/integrity itself (V8 CachedData /
    // QuickJS QJSB header) and reports a mismatch via rejected_out.
    if (GetTypedArrayBytes(env, argv[4], &bytes, &length) && length > 0) {
      bool rejected = false;
      if (unofficial_napi_bytecode_deserialize(env,
                                               bytes,
                                               length,
                                               code,
                                               filename,
                                               unofficial_napi_bytecode_shape_script,
                                               nullptr,
                                               host_defined_option_id,
                                               &script_bytecode,
                                               &rejected) == napi_ok) {
        cached_data_rejected = rejected;
      } else {
        cached_data_rejected = true;
      }
    } else {
      cached_data_rejected = true;
    }
  }
  if (script_bytecode == nullptr) {
    if (unofficial_napi_bytecode_compile(env,
                                         code,
                                         filename,
                                         unofficial_napi_bytecode_shape_script,
                                         nullptr,
                                         host_defined_option_id,
                                         line_offset,
                                         column_offset,
                                         &script_bytecode,
                                         nullptr) != napi_ok ||
        script_bytecode == nullptr) {
      return nullptr;  // Preserve the syntax error.
    }
  }

  if (has_cached_data_arg) {
    napi_value rejected = nullptr;
    napi_get_boolean(env, cached_data_rejected, &rejected);
    if (rejected != nullptr) {
      napi_set_named_property(env, this_arg, "cachedDataRejected", rejected);
    }
  }
  if (produce_cached_data) {
    napi_value cache_buffer = nullptr;
    const uint8_t* produced_bytes = nullptr;
    size_t produced_len = 0;
    // Only report cachedDataProduced when the engine actually emitted bytes
    // (an empty buffer round-trips as "rejected", so reporting it as produced
    // would be misleading).
    bool produced_ok =
        unofficial_napi_bytecode_serialize(env, script_bytecode, &cache_buffer) == napi_ok &&
        cache_buffer != nullptr &&
        GetTypedArrayBytes(env, cache_buffer, &produced_bytes, &produced_len) && produced_len > 0;
    if (produced_ok) {
      cache_buffer = EnsureNodeBuffer(env, cache_buffer);
      produced_ok = cache_buffer != nullptr;
    }
    napi_value produced = nullptr;
    napi_get_boolean(env, produced_ok, &produced);
    if (produced != nullptr) {
      napi_set_named_property(env, this_arg, "cachedDataProduced", produced);
    }
    if (produced_ok) {
      napi_set_named_property(env, this_arg, "cachedData", cache_buffer);
    }
  }
  (void)unofficial_napi_bytecode_release(env, script_bytecode);
  return this_arg;
}

static napi_value ContextifyScriptRunInContextCallback(napi_env env, napi_callback_info info) {
  size_t argc = 5;
  napi_value argv[5] = {nullptr, nullptr, nullptr, nullptr, nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  napi_value sandbox = nullptr;
  if (argc >= 1 && argv[0] != nullptr) {
    sandbox = argv[0];
  } else {
    napi_get_null(env, &sandbox);
  }

  int64_t timeout = -1;
  if (argc >= 2 && argv[1] != nullptr) {
    napi_get_value_int64(env, argv[1], &timeout);
  }

  bool display_errors = true;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_get_value_bool(env, argv[2], &display_errors);
  }

  bool break_on_sigint = false;
  if (argc >= 4 && argv[3] != nullptr) {
    napi_get_value_bool(env, argv[3], &break_on_sigint);
  }

  bool break_on_first_line = false;
  if (argc >= 5 && argv[4] != nullptr) {
    napi_get_value_bool(env, argv[4], &break_on_first_line);
  }

  napi_value code_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyCode");
  napi_value filename_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyFilename");
  napi_value line_offset_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyLineOffset");
  napi_value column_offset_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyColumnOffset");
  napi_value host_defined_option_id = GetContextifyHostDefinedOption(env, this_arg);
  SetContextifyHostDefinedOption(env, this_arg, host_defined_option_id);

  int32_t line_offset = 0;
  int32_t column_offset = 0;
  if (line_offset_value != nullptr && !IsUndefinedValue(env, line_offset_value)) {
    napi_get_value_int32(env, line_offset_value, &line_offset);
  }
  if (column_offset_value != nullptr && !IsUndefinedValue(env, column_offset_value)) {
    napi_get_value_int32(env, column_offset_value, &column_offset);
  }

  napi_value result = nullptr;
  const unofficial_napi_js_source run_source{code_value, nullptr};
  const napi_status st = unofficial_napi_contextify_run_script(env,
                                                               sandbox,
                                                               &run_source,
                                                               filename_value,
                                                               line_offset,
                                                               column_offset,
                                                               timeout,
                                                               display_errors,
                                                               break_on_sigint,
                                                               break_on_first_line,
                                                               host_defined_option_id,
                                                               &result);
  ClearContextifyHostDefinedOptionProperties(env, this_arg);
  if (st != napi_ok || result == nullptr) {
    return nullptr;
  }
  return result;
}

static bool EvalEntryUsesModuleInputType(napi_env env) {
  if (env == nullptr) return false;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value process = nullptr;
  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process ||
      napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) {
    return false;
  }

  napi_value exec_argv = nullptr;
  bool has_exec_argv = false;
  if (napi_has_named_property(env, process, "execArgv", &has_exec_argv) != napi_ok || !has_exec_argv ||
      napi_get_named_property(env, process, "execArgv", &exec_argv) != napi_ok || exec_argv == nullptr) {
    return false;
  }

  bool is_array = false;
  if (napi_is_array(env, exec_argv, &is_array) != napi_ok || !is_array) return false;

  uint32_t length = 0;
  if (napi_get_array_length(env, exec_argv, &length) != napi_ok) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value element = nullptr;
    if (napi_get_element(env, exec_argv, i, &element) != napi_ok || element == nullptr) continue;
    const std::string token = ValueToUtf8(env, element);
    if (token == "--input-type" && i + 1 < length) {
      napi_value next = nullptr;
      if (napi_get_element(env, exec_argv, i + 1, &next) == napi_ok && next != nullptr) {
        const std::string value = ValueToUtf8(env, next);
        return value.rfind("module", 0) == 0;
      }
      return false;
    }
    if (token.rfind("--input-type=", 0) == 0) {
      return token.substr(std::string("--input-type=").size()).rfind("module", 0) == 0;
    }
  }

  return false;
}

static napi_value ContextifyScriptCreateCachedDataCallback(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  if (napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  napi_value code_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyCode");
  napi_value filename_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyFilename");
  napi_value line_offset_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyLineOffset");
  napi_value column_offset_value = GetNamedPropertyOrUndefined(env, this_arg, "contextifyColumnOffset");
  napi_value host_defined_option_id = GetNamedPropertyOrUndefined(env, this_arg, "contextifyHostDefinedOptionId");

  int32_t line_offset = 0;
  int32_t column_offset = 0;
  if (line_offset_value != nullptr && !IsUndefinedValue(env, line_offset_value)) {
    napi_get_value_int32(env, line_offset_value, &line_offset);
  }
  if (column_offset_value != nullptr && !IsUndefinedValue(env, column_offset_value)) {
    napi_get_value_int32(env, column_offset_value, &column_offset);
  }

  (void)host_defined_option_id;
  napi_value out = nullptr;
  if (!CreateScriptCachedDataBuffer(env, code_value, filename_value, line_offset, column_offset, &out)) {
    return nullptr;
  }
  return out;
}

static napi_value ContextifyStartSigintWatchdogCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, internal_binding::StartSigintWatchdog(), &out);
  return out;
}

static napi_value ContextifyStopSigintWatchdogCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, internal_binding::StopSigintWatchdog(), &out);
  return out;
}

static napi_value ContextifyWatchdogHasPendingSigintCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  napi_get_boolean(env, internal_binding::WatchdogHasPendingSigint(), &out);
  return out;
}

static void SetContextifyNamedInt(napi_env env, napi_value object, const char* key, int32_t value) {
  napi_value out = nullptr;
  if (napi_create_int32(env, value, &out) == napi_ok && out != nullptr) {
    napi_set_named_property(env, object, key, out);
  }
}

static napi_value ContextifyMeasureMemoryFallback(napi_env env, napi_callback_info info) {
  (void)info;

  napi_deferred deferred = nullptr;
  napi_value promise = nullptr;
  if (napi_create_promise(env, &deferred, &promise) != napi_ok || promise == nullptr) {
    return UndefinedValue(env);
  }

  napi_value value = nullptr;
  napi_create_object(env, &value);
  napi_resolve_deferred(env, deferred, value != nullptr ? value : UndefinedValue(env));
  return promise;
}

static bool InstallContextifyMeasureMemoryShape(napi_env env, napi_value binding) {
  napi_value constants = nullptr;
  napi_value measure_memory = nullptr;
  napi_value mode = nullptr;
  napi_value execution = nullptr;
  napi_value measure_memory_fn = nullptr;

  if (napi_create_object(env, &constants) != napi_ok || constants == nullptr ||
      napi_create_object(env, &measure_memory) != napi_ok || measure_memory == nullptr ||
      napi_create_object(env, &mode) != napi_ok || mode == nullptr ||
      napi_create_object(env, &execution) != napi_ok || execution == nullptr ||
      napi_create_function(env,
                           "measureMemory",
                           NAPI_AUTO_LENGTH,
                           ContextifyMeasureMemoryFallback,
                           nullptr,
                           &measure_memory_fn) != napi_ok ||
      measure_memory_fn == nullptr) {
    return false;
  }

  SetContextifyNamedInt(env, mode, "SUMMARY", 0);
  SetContextifyNamedInt(env, mode, "DETAILED", 1);
  SetContextifyNamedInt(env, execution, "DEFAULT", 0);
  SetContextifyNamedInt(env, execution, "EAGER", 1);

  return napi_set_named_property(env, measure_memory, "mode", mode) == napi_ok &&
         napi_set_named_property(env, measure_memory, "execution", execution) == napi_ok &&
         napi_set_named_property(env, constants, "measureMemory", measure_memory) == napi_ok &&
         napi_set_named_property(env, binding, "constants", constants) == napi_ok &&
         napi_set_named_property(env, binding, "measureMemory", measure_memory_fn) == napi_ok;
}

static napi_value ContextifyMakeContextCallback(napi_env env, napi_callback_info info) {
  size_t argc = 7;
  napi_value argv[7] = {nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  napi_value sandbox_or_symbol = nullptr;
  if (argc >= 1 && argv[0] != nullptr) {
    sandbox_or_symbol = argv[0];
  } else {
    napi_create_object(env, &sandbox_or_symbol);
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, sandbox_or_symbol, &type) != napi_ok ||
      (type != napi_object && type != napi_function && type != napi_symbol)) {
    napi_create_object(env, &sandbox_or_symbol);
  }

  napi_value name = nullptr;
  if (argc >= 2 && argv[1] != nullptr) {
    name = argv[1];
  } else {
    napi_create_string_utf8(env, "VM Context", NAPI_AUTO_LENGTH, &name);
  }

  napi_value origin = nullptr;
  if (argc >= 3 && argv[2] != nullptr) {
    origin = argv[2];
  } else {
    napi_get_undefined(env, &origin);
  }

  bool allow_strings = true;
  if (argc >= 4 && argv[3] != nullptr) {
    napi_get_value_bool(env, argv[3], &allow_strings);
  }
  bool allow_wasm = true;
  if (argc >= 5 && argv[4] != nullptr) {
    napi_get_value_bool(env, argv[4], &allow_wasm);
  }
  bool own_microtask_queue = false;
  if (argc >= 6 && argv[5] != nullptr) {
    napi_get_value_bool(env, argv[5], &own_microtask_queue);
  }

  napi_value host_defined_option_id = nullptr;
  if (argc >= 7 && argv[6] != nullptr) {
    host_defined_option_id = argv[6];
  } else {
    napi_get_undefined(env, &host_defined_option_id);
  }

  napi_value out = nullptr;
  if (unofficial_napi_contextify_make_context(env,
                                              sandbox_or_symbol,
                                              name,
                                              origin,
                                              allow_strings,
                                              allow_wasm,
                                              own_microtask_queue,
                                              host_defined_option_id,
                                              &out) != napi_ok ||
      out == nullptr) {
    return nullptr;
  }

  napi_value context_symbol = GetUtilPrivateSymbolByName(env, "contextify_context_private_symbol");
  if (context_symbol != nullptr) {
    napi_value true_value = nullptr;
    if (napi_get_boolean(env, true, &true_value) == napi_ok && true_value != nullptr) {
      (void)napi_set_property(env, out, context_symbol, true_value);
    }
  }

  return out;
}

static napi_value ContextifyCompileFunctionCallback(napi_env env, napi_callback_info info) {
  size_t argc = 10;
  napi_value argv[10] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  napi_value code = argc >= 1 && argv[0] != nullptr ? argv[0] : nullptr;
  if (code == nullptr) {
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &code);
  }
  napi_value filename = argc >= 2 && argv[1] != nullptr ? argv[1] : nullptr;
  if (filename == nullptr) {
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &filename);
  }

  int32_t line_offset = 0;
  int32_t column_offset = 0;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_get_value_int32(env, argv[2], &line_offset);
  }
  if (argc >= 4 && argv[3] != nullptr) {
    napi_get_value_int32(env, argv[3], &column_offset);
  }

  napi_value cached_data = nullptr;
  if (argc >= 5 && argv[4] != nullptr) {
    cached_data = argv[4];
  } else {
    napi_get_undefined(env, &cached_data);
  }

  bool produce_cached_data = false;
  if (argc >= 6 && argv[5] != nullptr) {
    napi_get_value_bool(env, argv[5], &produce_cached_data);
  }

  napi_value parsing_context = nullptr;
  if (argc >= 7 && argv[6] != nullptr) {
    parsing_context = argv[6];
  } else {
    napi_get_undefined(env, &parsing_context);
  }

  napi_value context_extensions = nullptr;
  if (argc >= 8 && argv[7] != nullptr) {
    context_extensions = argv[7];
  } else {
    napi_get_undefined(env, &context_extensions);
  }

  napi_value params = nullptr;
  if (argc >= 9 && argv[8] != nullptr) {
    params = argv[8];
  } else {
    napi_get_undefined(env, &params);
  }

  napi_value host_defined_option_id = nullptr;
  if (argc >= 10 && argv[9] != nullptr) {
    host_defined_option_id = argv[9];
  } else {
    napi_get_undefined(env, &host_defined_option_id);
  }

  // vm.compileFunction cache handling now lives here: user cachedData is
  // validated via bytecode_deserialize (honest rejected flag), production via
  // an up-front eager bytecode_compile whose artifact the compile consumes.
  const bool has_cached_data = cached_data != nullptr && !IsUndefinedValue(env, cached_data);
  void* fn_bytecode = nullptr;
  bool cached_data_rejected = false;
  if (has_cached_data) {
    const uint8_t* bytes = nullptr;
    size_t length = 0;
    // The provider validates source/shape/params/integrity itself (V8
    // CachedData / QuickJS QJSB header) and reports a mismatch via rejected_out.
    if (GetTypedArrayBytes(env, cached_data, &bytes, &length) && length > 0) {
      bool rejected = false;
      if (unofficial_napi_bytecode_deserialize(env,
                                               bytes,
                                               length,
                                               code,
                                               filename,
                                               unofficial_napi_bytecode_shape_cjs_function,
                                               params,
                                               host_defined_option_id,
                                               &fn_bytecode,
                                               &rejected) == napi_ok) {
        cached_data_rejected = rejected;
      } else {
        cached_data_rejected = true;
      }
    } else {
      cached_data_rejected = true;
    }
  }
  // Produce a fresh cache when asked — including after a rejected cachedData,
  // so a caller refreshing a stale cache (cachedData + produceCachedData) gets
  // a new buffer instead of silently nothing.
  if (fn_bytecode == nullptr && produce_cached_data) {
    if (unofficial_napi_bytecode_compile(env,
                                         code,
                                         filename,
                                         unofficial_napi_bytecode_shape_cjs_function,
                                         params,
                                         host_defined_option_id,
                                         line_offset,
                                         column_offset,
                                         &fn_bytecode,
                                         nullptr) != napi_ok ||
        fn_bytecode == nullptr) {
      return nullptr;  // Preserve the compile error.
    }
  }

  const unofficial_napi_js_source compile_source{fn_bytecode != nullptr ? nullptr : code, fn_bytecode};
  napi_value out = nullptr;
  const napi_status compile_status =
      unofficial_napi_contextify_compile_function(env,
                                                  &compile_source,
                                                  filename,
                                                  line_offset,
                                                  column_offset,
                                                  parsing_context,
                                                  context_extensions,
                                                  params,
                                                  host_defined_option_id,
                                                  &out);
  if (compile_status != napi_ok || out == nullptr) {
    if (fn_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, fn_bytecode);
    return nullptr;
  }

  if (has_cached_data) {
    napi_value rejected = nullptr;
    napi_get_boolean(env, cached_data_rejected, &rejected);
    if (rejected != nullptr) {
      napi_set_named_property(env, out, "cachedDataRejected", rejected);
    }
  }
  if (produce_cached_data && fn_bytecode != nullptr) {
    napi_value cache_buffer = nullptr;
    const uint8_t* produced_bytes = nullptr;
    size_t produced_len = 0;
    bool produced_ok =
        unofficial_napi_bytecode_serialize(env, fn_bytecode, &cache_buffer) == napi_ok &&
        cache_buffer != nullptr &&
        GetTypedArrayBytes(env, cache_buffer, &produced_bytes, &produced_len) && produced_len > 0;
    if (produced_ok) {
      cache_buffer = EnsureNodeBuffer(env, cache_buffer);
      produced_ok = cache_buffer != nullptr;
    }
    napi_value produced = nullptr;
    napi_get_boolean(env, produced_ok, &produced);
    if (produced != nullptr) {
      napi_set_named_property(env, out, "cachedDataProduced", produced);
    }
    if (produced_ok) {
      napi_set_named_property(env, out, "cachedData", cache_buffer);
    }
  }
  if (fn_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, fn_bytecode);
  return out;
}

static napi_value ContextifyCompileFunctionForCJSLoaderCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  napi_value code = argc >= 1 && argv[0] != nullptr ? argv[0] : nullptr;
  if (code == nullptr) {
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &code);
  }
  napi_value filename = argc >= 2 && argv[1] != nullptr ? argv[1] : nullptr;
  if (filename == nullptr) {
    napi_create_string_utf8(env, "[eval]", NAPI_AUTO_LENGTH, &filename);
  }
  bool is_sea_main = false;
  if (argc >= 3 && argv[2] != nullptr) {
    napi_get_value_bool(env, argv[2], &is_sea_main);
  }
  (void)is_sea_main;
  bool should_detect_module = false;
  if (argc >= 4 && argv[3] != nullptr) {
    napi_get_value_bool(env, argv[3], &should_detect_module);
  }

  napi_value undefined = UndefinedValue(env);
  napi_value params = nullptr;
  if (!CreateStringArray(env, {"exports", "require", "module", "__filename", "__dirname"}, &params)) {
    return nullptr;
  }
  napi_value host_defined_option_id = GetPerIsolateSymbolByName(env, "vm_dynamic_import_default_internal");
  if (host_defined_option_id == nullptr) host_defined_option_id = undefined;

  // Bytecode sidecar cache: deserialize <filename>.v8b/.qjsb when it matches
  // the exact source about to be compiled, otherwise compile to a fresh
  // bytecode handle and write the sidecar (write-on-first-run). Only real
  // files participate; [eval]/[stdin]/REPL sources have no sidecar identity.
  void* cjs_bytecode = nullptr;
  std::string sidecar_source_path;
  std::string sidecar_source_utf8;
  bool sidecar_eligible = false;
  if (edge_bytecode_cache::Enabled()) {
    std::string filename_utf8 = ValueToUtf8(env, filename);
    fs::path filename_path(filename_utf8);
    if (filename_path.is_absolute() && PathExistsRegularFile(filename_path)) {
      sidecar_eligible = true;
      sidecar_source_path = std::move(filename_utf8);
      sidecar_source_utf8 = ValueToUtf8(env, code);
      edge_bytecode_cache::SidecarPayload payload;
      if (edge_bytecode_cache::ReadSidecar(sidecar_source_path, sidecar_source_utf8,
                                           edge_bytecode_cache::kFlagCjsFunctionV1, &payload)) {
        bool rejected = false;
        if (unofficial_napi_bytecode_deserialize(env,
                                                 payload.data(),
                                                 payload.payload_size,
                                                 code,
                                                 filename,
                                                 unofficial_napi_bytecode_shape_cjs_function,
                                                 params,
                                                 host_defined_option_id,
                                                 &cjs_bytecode,
                                                 &rejected) != napi_ok ||
            rejected || cjs_bytecode == nullptr) {
          // Engine refused bytecode our header considered valid (e.g. engine
          // flag drift); drop it so the next run rewrites instead of
          // re-rejecting forever.
          edge_bytecode_cache::RemoveSidecar(sidecar_source_path);
          cjs_bytecode = nullptr;
        }
      }
      if (cjs_bytecode == nullptr) {
        // No valid sidecar: compile once (eagerly) and persist the bytes; the
        // compile below consumes the same handle, so the source is parsed
        // exactly once.
        bool can_parse_as_module = false;
        const napi_status produce_status =
            unofficial_napi_bytecode_compile(env,
                                             code,
                                             filename,
                                             unofficial_napi_bytecode_shape_cjs_function,
                                             params,
                                             host_defined_option_id,
                                             0,
                                             0,
                                             &cjs_bytecode,
                                             &can_parse_as_module);
        if (produce_status != napi_ok || cjs_bytecode == nullptr) {
          if (produce_status == napi_pending_exception && should_detect_module && can_parse_as_module) {
            bool has_pending = false;
            napi_value ignored = nullptr;
            if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
              (void)napi_get_and_clear_last_exception(env, &ignored);
            }
            return CreateContextifyCjsLoaderResult(env, nullptr, true);
          }
          return nullptr;  // Preserve the compile error.
        }
        napi_value cache_buffer = nullptr;
        if (unofficial_napi_bytecode_serialize(env, cjs_bytecode, &cache_buffer) == napi_ok &&
            cache_buffer != nullptr) {
          const uint8_t* bytes = nullptr;
          size_t length = 0;
          if (GetTypedArrayBytes(env, cache_buffer, &bytes, &length) && length > 0) {
            edge_bytecode_cache::WriteSidecar(sidecar_source_path,
                                              sidecar_source_utf8,
                                              edge_bytecode_cache::kFlagCjsFunctionV1,
                                              bytes,
                                              length);
          }
        }
      }
    }
  }
  (void)sidecar_eligible;

  const unofficial_napi_js_source compile_source{cjs_bytecode != nullptr ? nullptr : code, cjs_bytecode};
  napi_value compile_result = nullptr;
  napi_status status = unofficial_napi_contextify_compile_function(env,
                                                                   &compile_source,
                                                                   filename,
                                                                   0,
                                                                   0,
                                                                   undefined,
                                                                   undefined,
                                                                   params,
                                                                   host_defined_option_id,
                                                                   &compile_result);
  if (cjs_bytecode != nullptr) (void)unofficial_napi_bytecode_release(env, cjs_bytecode);
  if (status == napi_ok && compile_result != nullptr) {
    return CreateContextifyCjsLoaderResult(env, compile_result, false);
  }
  if (status != napi_pending_exception || !should_detect_module) {
    return nullptr;
  }

  napi_value compile_error = nullptr;
  bool has_pending_exception = false;
  if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
    (void)napi_get_and_clear_last_exception(env, &compile_error);
  }

  bool can_parse_as_esm = false;
  if (unofficial_napi_contextify_contains_module_syntax(
          env, code, filename, undefined, true, &can_parse_as_esm) != napi_ok) {
    if (compile_error != nullptr) napi_throw(env, compile_error);
    return nullptr;
  }
  if (!can_parse_as_esm) {
    if (compile_error != nullptr) napi_throw(env, compile_error);
    return nullptr;
  }

  return CreateContextifyCjsLoaderResult(env, nullptr, true);
}

static napi_value ContextifyContainsModuleSyntaxCallback(napi_env env, napi_callback_info info) {
  size_t argc = 4;
  napi_value argv[4] = {nullptr, nullptr, nullptr, nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }

  napi_value code = argc >= 1 && argv[0] != nullptr ? argv[0] : nullptr;
  if (code == nullptr) {
    napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &code);
  }
  napi_value filename = argc >= 2 && argv[1] != nullptr ? argv[1] : nullptr;
  if (filename == nullptr) {
    napi_create_string_utf8(env, "[eval]", NAPI_AUTO_LENGTH, &filename);
  }
  napi_value resource_name = nullptr;
  if (argc >= 3 && argv[2] != nullptr) {
    resource_name = argv[2];
  } else {
    napi_get_undefined(env, &resource_name);
  }
  bool cjs_var_in_scope = true;
  if (argc >= 4 && argv[3] != nullptr) {
    napi_valuetype arg_type = napi_undefined;
    if (napi_typeof(env, argv[3], &arg_type) == napi_ok) {
      if (arg_type == napi_string) {
        // Match Node's ContainsModuleSyntax(): a string sentinel means the
        // CommonJS eval scope should not provide CJS wrapper variables.
        cjs_var_in_scope = false;
      } else if (arg_type == napi_boolean) {
        napi_get_value_bool(env, argv[3], &cjs_var_in_scope);
      }
    }
  }

  bool result = false;
  if (unofficial_napi_contextify_contains_module_syntax(
          env, code, filename, resource_name, cjs_var_in_scope, &result) != napi_ok) {
    return nullptr;
  }
  napi_value out = nullptr;
  napi_get_boolean(env, result, &out);
  return out;
}

static napi_value BuiltinsCreateStringArray(napi_env env, const std::vector<std::string>& values) {
  napi_value out = nullptr;
  if (napi_create_array_with_length(env, values.size(), &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  for (size_t i = 0; i < values.size(); ++i) {
    napi_value value = nullptr;
    if (napi_create_string_utf8(env, values[i].c_str(), NAPI_AUTO_LENGTH, &value) != napi_ok ||
        value == nullptr ||
        napi_set_element(env, out, static_cast<uint32_t>(i), value) != napi_ok) {
      return nullptr;
    }
  }
  return out;
}

static napi_value BuiltinsCreateSet(napi_env env) {
  napi_value global = nullptr;
  napi_value set_ctor = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "Set", &set_ctor) != napi_ok || set_ctor == nullptr) {
    return nullptr;
  }
  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, set_ctor, &type) != napi_ok || type != napi_function) return nullptr;
  napi_value set_obj = nullptr;
  if (napi_new_instance(env, set_ctor, 0, nullptr, &set_obj) != napi_ok || set_obj == nullptr) {
    return nullptr;
  }
  return set_obj;
}

static void BuiltinsAppendArrayItemsToSet(napi_env env, napi_value set_obj, napi_value input) {
  bool is_array = false;
  if (set_obj == nullptr || input == nullptr || napi_is_array(env, input, &is_array) != napi_ok ||
      !is_array) {
    return;
  }
  napi_value add_fn = nullptr;
  if (napi_get_named_property(env, set_obj, "add", &add_fn) != napi_ok || add_fn == nullptr) return;
  napi_valuetype add_type = napi_undefined;
  if (napi_typeof(env, add_fn, &add_type) != napi_ok || add_type != napi_function) return;
  uint32_t len = 0;
  napi_get_array_length(env, input, &len);
  for (uint32_t i = 0; i < len; ++i) {
    napi_value item = nullptr;
    if (napi_get_element(env, input, i, &item) != napi_ok || item == nullptr) continue;
    napi_value ignored = nullptr;
    napi_call_function(env, set_obj, add_fn, 1, &item, &ignored);
  }
}

static napi_value BuiltinsGetCacheUsageCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return UndefinedValue(env);

  napi_value with_cache = BuiltinsCreateSet(env);
  if (with_cache != nullptr) napi_set_named_property(env, out, "compiledWithCache", with_cache);
  napi_value without_cache = BuiltinsCreateSet(env);
  if (without_cache != nullptr) {
    napi_set_named_property(env, out, "compiledWithoutCache", without_cache);
  }

  napi_value builtin_ids_array = BuiltinsCreateStringArray(env, builtin_catalog::AllBuiltinIds());
  if (builtin_ids_array != nullptr) {
    napi_set_named_property(env, out, "compiledInSnapshot", builtin_ids_array);
    BuiltinsAppendArrayItemsToSet(env, without_cache, builtin_ids_array);
  } else {
    napi_value empty = nullptr;
    napi_create_array_with_length(env, 0, &empty);
    if (empty != nullptr) napi_set_named_property(env, out, "compiledInSnapshot", empty);
  }

  return out;
}

static bool InstallBuiltinsCompatExports(napi_env env, napi_value binding) {
  const builtin_catalog::BuiltinCategories& builtin_categories =
      builtin_catalog::GetBuiltinCategories();
  napi_value categories = nullptr;
  napi_value can_array = BuiltinsCreateStringArray(env, builtin_categories.can_be_required);
  napi_value cannot_array = BuiltinsCreateStringArray(env, builtin_categories.cannot_be_required);
  if (napi_create_object(env, &categories) != napi_ok || categories == nullptr ||
      can_array == nullptr || cannot_array == nullptr ||
      napi_set_named_property(env, categories, "canBeRequired", can_array) != napi_ok ||
      napi_set_named_property(env, categories, "cannotBeRequired", cannot_array) != napi_ok ||
      napi_set_named_property(env, binding, "builtinCategories", categories) != napi_ok) {
    return false;
  }

  // Legacy process.binding('natives') reads `configs` as an alias of `config`.
  napi_value config = nullptr;
  if (napi_get_named_property(env, binding, "config", &config) != napi_ok || config == nullptr ||
      napi_set_named_property(env, binding, "configs", config) != napi_ok) {
    return false;
  }

  napi_value get_cache_usage_fn = nullptr;
  if (napi_create_function(env,
                           "getCacheUsage",
                           NAPI_AUTO_LENGTH,
                           BuiltinsGetCacheUsageCallback,
                           nullptr,
                           &get_cache_usage_fn) != napi_ok ||
      get_cache_usage_fn == nullptr ||
      napi_set_named_property(env, binding, "getCacheUsage", get_cache_usage_fn) != napi_ok) {
    return false;
  }
  return true;
}

static napi_value GetOrCreateNativeBuiltinsBinding(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) return nullptr;
  if (state->native_builtins_binding_ref != nullptr) {
    napi_value existing = nullptr;
    if (napi_get_reference_value(env, state->native_builtins_binding_ref, &existing) == napi_ok &&
        existing != nullptr) {
      return existing;
    }
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;

  const std::vector<std::string>& builtin_ids = CollectRuntimeBuiltinIds();
  napi_value builtin_ids_array = nullptr;
  if (napi_create_array_with_length(env, builtin_ids.size(), &builtin_ids_array) != napi_ok ||
      builtin_ids_array == nullptr) {
    return nullptr;
  }
  for (size_t i = 0; i < builtin_ids.size(); ++i) {
    napi_value id = nullptr;
    if (napi_create_string_utf8(env, builtin_ids[i].c_str(), NAPI_AUTO_LENGTH, &id) != napi_ok ||
        id == nullptr ||
        napi_set_element(env, builtin_ids_array, i, id) != napi_ok) {
      return nullptr;
    }
  }
  if (napi_set_named_property(env, binding, "builtinIds", builtin_ids_array) != napi_ok) {
    return nullptr;
  }

  napi_value natives = CreateBuiltinsNativesObject(env, builtin_ids);
  if (natives == nullptr || napi_set_named_property(env, binding, "natives", natives) != napi_ok) {
    return nullptr;
  }

  napi_value config_json = nullptr;
  std::string builtins_config_json = LoadBuiltinsConfigJson(RuntimeHasIntl(env));
  if (napi_create_string_utf8(env, builtins_config_json.c_str(), NAPI_AUTO_LENGTH, &config_json) != napi_ok ||
      config_json == nullptr ||
      napi_set_named_property(env, binding, "config", config_json) != napi_ok) {
    return nullptr;
  }

  napi_value has_cached_builtins_fn = nullptr;
  if (napi_create_function(env, "hasCachedBuiltins", NAPI_AUTO_LENGTH, ReturnFalseCallback, nullptr,
                           &has_cached_builtins_fn) != napi_ok ||
      has_cached_builtins_fn == nullptr ||
      napi_set_named_property(env, binding, "hasCachedBuiltins", has_cached_builtins_fn) != napi_ok) {
    return nullptr;
  }

  napi_value compile_fn = nullptr;
  if (napi_create_function(
          env, "compileFunction", NAPI_AUTO_LENGTH, BuiltinsCompileFunctionCallback, state, &compile_fn) !=
          napi_ok ||
      compile_fn == nullptr ||
      napi_set_named_property(env, binding, "compileFunction", compile_fn) != napi_ok) {
    return nullptr;
  }
  napi_value set_internal_loaders_fn = nullptr;
  if (napi_create_function(env,
                           "setInternalLoaders",
                           NAPI_AUTO_LENGTH,
                           BuiltinsSetInternalLoadersCallback,
                           state,
                           &set_internal_loaders_fn) != napi_ok ||
      set_internal_loaders_fn == nullptr ||
      napi_set_named_property(env, binding, "setInternalLoaders", set_internal_loaders_fn) != napi_ok) {
    return nullptr;
  }

  if (!InstallBuiltinsCompatExports(env, binding)) {
    return nullptr;
  }

  if (state->native_builtins_binding_ref != nullptr) {
    napi_delete_reference(env, state->native_builtins_binding_ref);
    state->native_builtins_binding_ref = nullptr;
  }
  if (napi_create_reference(env, binding, 1, &state->native_builtins_binding_ref) != napi_ok ||
      state->native_builtins_binding_ref == nullptr) {
    return nullptr;
  }
  return binding;
}

static napi_value TraceEventsTrace(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static TraceEventsBindingState* GetTraceEventsState(napi_env env) {
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return &state->trace_events_state;
}

static bool TraceEventsIsEnabled(const TraceEventsBindingState& st, const std::string& category) {
  auto it = st.category_refcounts.find(category);
  return it != st.category_refcounts.end() && it->second > 0;
}

static void TraceEventsSetBufferByteIfPresent(TraceEventsBindingState& st, const std::string& category, bool enabled) {
  auto it = st.category_buffers.find(category);
  if (it == st.category_buffers.end() || it->second.data == nullptr) return;
  it->second.data[0] = enabled ? 1 : 0;
}

static void TraceEventsNotifyStateHandler(napi_env env, TraceEventsBindingState& st) {
  if (st.state_update_handler_ref == nullptr) return;
  napi_value fn = nullptr;
  if (napi_get_reference_value(env, st.state_update_handler_ref, &fn) != napi_ok || fn == nullptr) return;
  napi_valuetype fn_type = napi_undefined;
  if (napi_typeof(env, fn, &fn_type) != napi_ok || fn_type != napi_function) return;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return;
  const bool async_hooks_enabled = TraceEventsIsEnabled(st, "node.async_hooks");
  napi_value enabled_value = nullptr;
  napi_get_boolean(env, async_hooks_enabled, &enabled_value);
  napi_value ignored = nullptr;
  napi_call_function(env, global, fn, 1, &enabled_value, &ignored);
}

static void TraceEventsApplyCategoryDelta(napi_env env,
                                          TraceEventsBindingState& st,
                                          const std::vector<std::string>& categories,
                                          bool enable) {
  bool changed = false;
  for (const std::string& category : categories) {
    if (category.empty()) continue;
    const bool was_enabled = TraceEventsIsEnabled(st, category);
    int32_t count = 0;
    auto it = st.category_refcounts.find(category);
    if (it != st.category_refcounts.end()) count = it->second;

    if (enable) {
      ++count;
    } else if (count > 0) {
      --count;
    }

    if (count > 0) {
      st.category_refcounts[category] = count;
    } else {
      st.category_refcounts.erase(category);
    }

    const bool now_enabled = count > 0;
    if (was_enabled != now_enabled) {
      changed = true;
      TraceEventsSetBufferByteIfPresent(st, category, now_enabled);
    }
  }

  if (changed) {
    TraceEventsNotifyStateHandler(env, st);
  }
}

static std::string TrimAsciiWhitespace(std::string value) {
  size_t start = 0;
  while (start < value.size() && std::isspace(static_cast<unsigned char>(value[start])) != 0) {
    ++start;
  }
  size_t end = value.size();
  while (end > start && std::isspace(static_cast<unsigned char>(value[end - 1])) != 0) {
    --end;
  }
  return value.substr(start, end - start);
}

static std::vector<std::string> TraceEventsParseCategoriesFromArg(napi_env env, napi_value arg) {
  std::vector<std::string> categories;
  if (arg == nullptr) return categories;

  bool is_array = false;
  if (napi_is_array(env, arg, &is_array) == napi_ok && is_array) {
    uint32_t len = 0;
    if (napi_get_array_length(env, arg, &len) != napi_ok) return categories;
    for (uint32_t i = 0; i < len; i++) {
      napi_value element = nullptr;
      if (napi_get_element(env, arg, i, &element) != napi_ok || element == nullptr) continue;
      std::string category = TrimAsciiWhitespace(ValueToUtf8(env, element));
      if (!category.empty()) categories.push_back(category);
    }
    return categories;
  }

  const std::string text = TrimAsciiWhitespace(ValueToUtf8(env, arg));
  if (text.empty()) return categories;
  size_t start = 0;
  while (start <= text.size()) {
    size_t comma = text.find(',', start);
    std::string category = TrimAsciiWhitespace(
        comma == std::string::npos ? text.substr(start) : text.substr(start, comma - start));
    if (!category.empty()) categories.push_back(std::move(category));
    if (comma == std::string::npos) break;
    start = comma + 1;
  }
  return categories;
}

static napi_value TraceEventsGetCategoryBuffer(napi_env env, TraceEventsBindingState& st, const std::string& category) {
  auto it = st.category_buffers.find(category);
  if (it != st.category_buffers.end() && it->second.typed_array_ref != nullptr) {
    napi_value existing = nullptr;
    if (napi_get_reference_value(env, it->second.typed_array_ref, &existing) == napi_ok && existing != nullptr) {
      return existing;
    }
  }

  napi_value ab = nullptr;
  void* data = nullptr;
  if (napi_create_arraybuffer(env, sizeof(uint8_t), &data, &ab) != napi_ok || ab == nullptr || data == nullptr) {
    return nullptr;
  }
  static_cast<uint8_t*>(data)[0] = TraceEventsIsEnabled(st, category) ? 1 : 0;

  napi_value ta = nullptr;
  if (napi_create_typedarray(env, napi_uint8_array, 1, ab, 0, &ta) != napi_ok || ta == nullptr) {
    return nullptr;
  }

  TraceEventsBindingState::CategoryBufferState buffer_state;
  buffer_state.data = static_cast<uint8_t*>(data);
  if (napi_create_reference(env, ta, 1, &buffer_state.typed_array_ref) != napi_ok || buffer_state.typed_array_ref == nullptr) {
    return ta;
  }

  auto existing = st.category_buffers.find(category);
  if (existing != st.category_buffers.end() && existing->second.typed_array_ref != nullptr) {
    napi_delete_reference(env, existing->second.typed_array_ref);
  }
  st.category_buffers[category] = buffer_state;
  return ta;
}

static napi_value TraceEventsIsTraceCategoryEnabled(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  std::string category;
  if (argc >= 1 && argv[0] != nullptr) {
    category = ValueToUtf8(env, argv[0]);
  }
  TraceEventsBindingState* st = GetTraceEventsState(env);
  const bool enabled = st != nullptr && !category.empty() && TraceEventsIsEnabled(*st, category);
  napi_value out = nullptr;
  napi_get_boolean(env, enabled, &out);
  return out;
}

static napi_value TraceEventsGetEnabledCategories(napi_env env, napi_callback_info /*info*/) {
  TraceEventsBindingState* st = GetTraceEventsState(env);
  std::vector<std::string> enabled_categories;
  if (st != nullptr) {
    enabled_categories.reserve(st->category_refcounts.size());
    for (const auto& entry : st->category_refcounts) {
      if (entry.second > 0) enabled_categories.push_back(entry.first);
    }
  }
  std::sort(enabled_categories.begin(), enabled_categories.end());
  std::string joined;
  for (size_t i = 0; i < enabled_categories.size(); i++) {
    if (i != 0) joined.push_back(',');
    joined.append(enabled_categories[i]);
  }
  napi_value out = nullptr;
  napi_create_string_utf8(env, joined.c_str(), NAPI_AUTO_LENGTH, &out);
  return out;
}

static napi_value TraceEventsGetCategoryEnabledBuffer(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;
  std::string category = argc >= 1 && argv[0] != nullptr ? ValueToUtf8(env, argv[0]) : "";
  TraceEventsBindingState* st = GetTraceEventsState(env);
  if (st == nullptr) return nullptr;
  return TraceEventsGetCategoryBuffer(env, *st, category);
}

struct TraceEventsCategorySetWrap {
  std::vector<std::string> categories;
  bool enabled = false;
  napi_ref wrapper_ref = nullptr;
};

static void TraceEventsCategorySetFinalize(napi_env env, void* data, void* /*hint*/) {
  auto* wrap = static_cast<TraceEventsCategorySetWrap*>(data);
  if (wrap == nullptr) return;
  if (wrap->enabled) {
    TraceEventsBindingState* st = GetTraceEventsState(env);
    if (st != nullptr) {
      TraceEventsApplyCategoryDelta(env, *st, wrap->categories, false);
    }
  }
  if (wrap->wrapper_ref != nullptr) napi_delete_reference(env, wrap->wrapper_ref);
  delete wrap;
}

static TraceEventsCategorySetWrap* TraceEventsUnwrapCategorySet(napi_env env, napi_value this_arg) {
  void* data = nullptr;
  if (this_arg == nullptr || napi_unwrap(env, this_arg, &data) != napi_ok || data == nullptr) return nullptr;
  return static_cast<TraceEventsCategorySetWrap*>(data);
}

static napi_value TraceEventsCategorySetEnable(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  auto* wrap = TraceEventsUnwrapCategorySet(env, this_arg);
  TraceEventsBindingState* st = GetTraceEventsState(env);
  if (wrap != nullptr && st != nullptr && !wrap->enabled) {
    wrap->enabled = true;
    TraceEventsApplyCategoryDelta(env, *st, wrap->categories, true);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value TraceEventsCategorySetDisable(napi_env env, napi_callback_info info) {
  napi_value this_arg = nullptr;
  size_t argc = 0;
  napi_get_cb_info(env, info, &argc, nullptr, &this_arg, nullptr);
  auto* wrap = TraceEventsUnwrapCategorySet(env, this_arg);
  TraceEventsBindingState* st = GetTraceEventsState(env);
  if (wrap != nullptr && st != nullptr && wrap->enabled) {
    wrap->enabled = false;
    TraceEventsApplyCategoryDelta(env, *st, wrap->categories, false);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value TraceEventsCategorySetCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  napi_value this_arg = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, &this_arg, nullptr) != napi_ok || this_arg == nullptr) {
    return nullptr;
  }

  auto* wrap = new TraceEventsCategorySetWrap();
  if (argc >= 1 && argv[0] != nullptr) {
    wrap->categories = TraceEventsParseCategoriesFromArg(env, argv[0]);
  }
  napi_wrap(env, this_arg, wrap, TraceEventsCategorySetFinalize, nullptr, &wrap->wrapper_ref);
  return this_arg;
}

static napi_value TraceEventsSetTraceCategoryStateUpdateHandler(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) return nullptr;

  TraceEventsBindingState* st = GetTraceEventsState(env);
  if (st == nullptr) return nullptr;
  if (st->state_update_handler_ref != nullptr) {
    napi_delete_reference(env, st->state_update_handler_ref);
    st->state_update_handler_ref = nullptr;
  }
  if (argc >= 1 && argv[0] != nullptr) {
    napi_valuetype t = napi_undefined;
    if (napi_typeof(env, argv[0], &t) == napi_ok && t == napi_function) {
      napi_create_reference(env, argv[0], 1, &st->state_update_handler_ref);
      TraceEventsNotifyStateHandler(env, *st);
    }
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

static napi_value GetOrCreateTraceEventsBinding(napi_env env) {
  TraceEventsBindingState* st = GetTraceEventsState(env);
  if (st == nullptr) return nullptr;
  if (st->binding_ref != nullptr) {
    napi_value existing = nullptr;
    if (napi_get_reference_value(env, st->binding_ref, &existing) == napi_ok && existing != nullptr) {
      return existing;
    }
  }

  napi_value binding = nullptr;
  if (napi_create_object(env, &binding) != napi_ok || binding == nullptr) return nullptr;
  auto define_method = [&](const char* name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    return napi_create_function(env, name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
           fn != nullptr &&
           napi_set_named_property(env, binding, name, fn) == napi_ok;
  };
  if (!define_method("trace", TraceEventsTrace) ||
      !define_method("isTraceCategoryEnabled", TraceEventsIsTraceCategoryEnabled) ||
      !define_method("getEnabledCategories", TraceEventsGetEnabledCategories) ||
      !define_method("getCategoryEnabledBuffer", TraceEventsGetCategoryEnabledBuffer) ||
      !define_method("setTraceCategoryStateUpdateHandler", TraceEventsSetTraceCategoryStateUpdateHandler)) {
    return nullptr;
  }

  napi_value category_set_ctor = nullptr;
  napi_property_descriptor category_set_props[] = {
      {"enable", nullptr, TraceEventsCategorySetEnable, nullptr, nullptr, nullptr, napi_default, nullptr},
      {"disable", nullptr, TraceEventsCategorySetDisable, nullptr, nullptr, nullptr, napi_default, nullptr},
  };
  if (napi_define_class(env,
                        "CategorySet",
                        NAPI_AUTO_LENGTH,
                        TraceEventsCategorySetCtor,
                        nullptr,
                        2,
                        category_set_props,
                        &category_set_ctor) != napi_ok ||
      category_set_ctor == nullptr ||
      napi_set_named_property(env, binding, "CategorySet", category_set_ctor) != napi_ok) {
    return nullptr;
  }

  if (st->binding_ref != nullptr) {
    napi_delete_reference(env, st->binding_ref);
    st->binding_ref = nullptr;
  }
  if (napi_create_reference(env, binding, 1, &st->binding_ref) != napi_ok || st->binding_ref == nullptr) {
    return nullptr;
  }
  return binding;
}

static napi_value CreateUvBinding(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  auto set_int32 = [&](const char* key, int32_t value) {
    napi_value v = nullptr;
    if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
      napi_set_named_property(env, out, key, v);
    }
  };

#define EDGE_SET_UV_CONSTANT(name, message) \
  set_int32("UV_" #name, static_cast<int32_t>(UV_##name));
  UV_ERRNO_MAP(EDGE_SET_UV_CONSTANT);
#undef EDGE_SET_UV_CONSTANT

#ifdef UV_UNKNOWN
  set_int32("UV_UNKNOWN", static_cast<int32_t>(UV_UNKNOWN));
#else
  set_int32("UV_UNKNOWN", -4094);
#endif
#ifdef UV_EAI_MEMORY
  set_int32("UV_EAI_MEMORY",
            static_cast<int32_t>(UV_EAI_MEMORY) != static_cast<int32_t>(UV_EAI_AGAIN)
                ? static_cast<int32_t>(UV_EAI_MEMORY)
                : -3006);
#else
  set_int32("UV_EAI_MEMORY", -3006);
#endif

  auto define_method = [&](const char* method_name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    return napi_create_function(env, method_name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
           fn != nullptr &&
           napi_set_named_property(env, out, method_name, fn) == napi_ok;
  };

  if (!define_method("getErrorMap", UvGetErrorMapCallback) ||
      !define_method("getErrorMessage", UvGetErrorMessageCallback) ||
      !define_method("errname", UvErrNameCallback)) {
    return undefined;
  }

  return out;
}

static napi_value CreateContextifyBinding(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_INTERNAL_BINDING")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_INTERNAL_BINDING contextify state=%p finalized=%s\n",
                 static_cast<void*>(state),
                 state != nullptr && state->finalized ? "true" : "false");
  }
  if (state == nullptr || state->finalized) return undefined;
  if (state->contextify_binding_ref != nullptr) {
    napi_value cached = nullptr;
    if (napi_get_reference_value(env, state->contextify_binding_ref, &cached) == napi_ok &&
        cached != nullptr) {
      return cached;
    }
  }

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;
  if (!InstallContextifyMeasureMemoryShape(env, out)) return undefined;

  napi_value contains_module_syntax = nullptr;
  if (napi_create_function(env,
                           "containsModuleSyntax",
                           NAPI_AUTO_LENGTH,
                           ContextifyContainsModuleSyntaxCallback,
                           nullptr,
                           &contains_module_syntax) == napi_ok &&
      contains_module_syntax != nullptr) {
    napi_set_named_property(env, out, "containsModuleSyntax", contains_module_syntax);
  }

  napi_value make_context = nullptr;
  if (napi_create_function(env,
                           "makeContext",
                           NAPI_AUTO_LENGTH,
                           ContextifyMakeContextCallback,
                           nullptr,
                           &make_context) == napi_ok &&
      make_context != nullptr) {
    napi_set_named_property(env, out, "makeContext", make_context);
  }

  napi_property_descriptor script_properties[] = {
      {"runInContext",
       nullptr,
       ContextifyScriptRunInContextCallback,
       nullptr,
       nullptr,
       nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable),
       nullptr},
      {"createCachedData",
       nullptr,
       ContextifyScriptCreateCachedDataCallback,
       nullptr,
       nullptr,
       nullptr,
       static_cast<napi_property_attributes>(napi_writable | napi_configurable),
       nullptr},
  };
  napi_value contextify_script_ctor = nullptr;
  if (napi_define_class(env,
                        "ContextifyScript",
                        NAPI_AUTO_LENGTH,
                        ContextifyScriptConstructorCallback,
                        nullptr,
                        sizeof(script_properties) / sizeof(script_properties[0]),
                        script_properties,
                        &contextify_script_ctor) == napi_ok &&
      contextify_script_ctor != nullptr) {
    napi_set_named_property(env, out, "ContextifyScript", contextify_script_ctor);
  }

  napi_value start_sigint_watchdog = nullptr;
  if (napi_create_function(env,
                           "startSigintWatchdog",
                           NAPI_AUTO_LENGTH,
                           ContextifyStartSigintWatchdogCallback,
                           nullptr,
                           &start_sigint_watchdog) == napi_ok &&
      start_sigint_watchdog != nullptr) {
    napi_set_named_property(env, out, "startSigintWatchdog", start_sigint_watchdog);
  }

  napi_value stop_sigint_watchdog = nullptr;
  if (napi_create_function(env,
                           "stopSigintWatchdog",
                           NAPI_AUTO_LENGTH,
                           ContextifyStopSigintWatchdogCallback,
                           nullptr,
                           &stop_sigint_watchdog) == napi_ok &&
      stop_sigint_watchdog != nullptr) {
    napi_set_named_property(env, out, "stopSigintWatchdog", stop_sigint_watchdog);
  }

  napi_value watchdog_has_pending_sigint = nullptr;
  if (napi_create_function(env,
                           "watchdogHasPendingSigint",
                           NAPI_AUTO_LENGTH,
                           ContextifyWatchdogHasPendingSigintCallback,
                           nullptr,
                           &watchdog_has_pending_sigint) == napi_ok &&
      watchdog_has_pending_sigint != nullptr) {
    napi_set_named_property(env, out, "watchdogHasPendingSigint", watchdog_has_pending_sigint);
  }

  napi_value compile_function = nullptr;
  if (napi_create_function(env,
                           "compileFunction",
                           NAPI_AUTO_LENGTH,
                           ContextifyCompileFunctionCallback,
                           nullptr,
                           &compile_function) == napi_ok &&
      compile_function != nullptr) {
    napi_set_named_property(env, out, "compileFunction", compile_function);
  }

  napi_value compile_function_for_cjs = nullptr;
  if (napi_create_function(env,
                           "compileFunctionForCJSLoader",
                           NAPI_AUTO_LENGTH,
                           ContextifyCompileFunctionForCJSLoaderCallback,
                           nullptr,
                           &compile_function_for_cjs) == napi_ok &&
      compile_function_for_cjs != nullptr) {
    napi_set_named_property(env, out, "compileFunctionForCJSLoader", compile_function_for_cjs);
  }

  DeleteRefIfPresent(env, &state->contextify_binding_ref);
  (void)napi_create_reference(env, out, 1, &state->contextify_binding_ref);
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_INTERNAL_BINDING")) {
    std::fprintf(stderr, "EDGE_TRACE_INTERNAL_BINDING contextify resolved\n");
  }
  return out;
}

static napi_value CreateModulesBinding(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  auto define_method = [&](const char* method_name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    return napi_create_function(env, method_name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
           fn != nullptr &&
           napi_set_named_property(env, out, method_name, fn) == napi_ok;
  };

  if (!define_method("readPackageJSON", ModulesReadPackageJSONCallback) ||
      !define_method("getNearestParentPackageJSONType", ModulesGetNearestParentPackageJSONTypeCallback) ||
      !define_method("getNearestParentPackageJSON", ModulesGetNearestParentPackageJSONCallback) ||
      !define_method("getPackageScopeConfig", ModulesGetPackageScopeConfigCallback) ||
      !define_method("getPackageType", ModulesGetPackageTypeCallback) ||
      !define_method("enableCompileCache", ModulesEnableCompileCacheCallback) ||
      !define_method("getCompileCacheDir", ModulesGetCompileCacheDirCallback) ||
      !define_method("flushCompileCache", ModulesFlushCompileCacheCallback) ||
      !define_method("getCompileCacheEntry", ModulesGetCompileCacheEntryCallback) ||
      !define_method("saveCompileCacheEntry", ModulesSaveCompileCacheEntryCallback) ||
      !define_method("setLazyPathHelpers", ModulesSetLazyPathHelpersCallback)) {
    return undefined;
  }

  napi_value status = nullptr;
  if (napi_create_array_with_length(env, 4, &status) != napi_ok || status == nullptr) return undefined;
  auto set_status = [&](uint32_t idx, const char* value) {
    napi_value v = nullptr;
    if (napi_create_string_utf8(env, value, NAPI_AUTO_LENGTH, &v) == napi_ok && v != nullptr) {
      napi_set_element(env, status, idx, v);
    }
  };
  set_status(0, "FAILED");
  set_status(1, "ENABLED");
  set_status(2, "ALREADY_ENABLED");
  set_status(3, "DISABLED");
  if (napi_set_named_property(env, out, "compileCacheStatus", status) != napi_ok) return undefined;

  napi_value cached_types = nullptr;
  if (napi_create_object(env, &cached_types) != napi_ok || cached_types == nullptr) return undefined;
  auto set_int32 = [&](const char* key, int32_t value) {
    napi_value v = nullptr;
    if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
      napi_set_named_property(env, cached_types, key, v);
    }
  };
  set_int32("kCommonJS", 0);
  set_int32("kESM", 1);
  set_int32("kStrippedTypeScript", 2);
  set_int32("kTransformedTypeScript", 3);
  set_int32("kTransformedTypeScriptWithSourceMaps", 4);
  if (napi_set_named_property(env, out, "cachedCodeTypes", cached_types) != napi_ok) return undefined;

  return out;
}

static napi_value CreateOptionsBinding(napi_env env) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);

  napi_value out = nullptr;
  if (napi_create_object(env, &out) != napi_ok || out == nullptr) return undefined;

  napi_value env_settings = nullptr;
  if (napi_create_object(env, &env_settings) != napi_ok || env_settings == nullptr) return undefined;
  auto set_int32 = [&](napi_value target, const char* key, int32_t value) {
    napi_value v = nullptr;
    if (napi_create_int32(env, value, &v) == napi_ok && v != nullptr) {
      napi_set_named_property(env, target, key, v);
    }
  };
  set_int32(env_settings, "kAllowedInEnvvar", 0);
  set_int32(env_settings, "kDisallowedInEnvvar", 1);
  if (napi_set_named_property(env, out, "envSettings", env_settings) != napi_ok) return undefined;

  napi_value types = nullptr;
  if (napi_create_object(env, &types) != napi_ok || types == nullptr) return undefined;
  set_int32(types, "kNoOp", 0);
  set_int32(types, "kV8Option", 1);
  set_int32(types, "kBoolean", 2);
  set_int32(types, "kInteger", 3);
  set_int32(types, "kUInteger", 4);
  set_int32(types, "kString", 5);
  set_int32(types, "kHostPort", 6);
  set_int32(types, "kStringList", 7);
  if (napi_set_named_property(env, out, "types", types) != napi_ok) return undefined;

  auto define_method = [&](const char* method_name, napi_callback cb) -> bool {
    napi_value fn = nullptr;
    return napi_create_function(env, method_name, NAPI_AUTO_LENGTH, cb, nullptr, &fn) == napi_ok &&
           fn != nullptr &&
           napi_set_named_property(env, out, method_name, fn) == napi_ok;
  };
  if (!define_method("getCLIOptionsValues", OptionsGetCLIOptionsValuesCallback) ||
      !define_method("getCLIOptionsInfo", OptionsGetCLIOptionsInfoCallback) ||
      !define_method("getOptionsAsFlags", OptionsGetOptionsAsFlagsCallback) ||
      !define_method("getEmbedderOptions", OptionsGetEmbedderOptionsCallback) ||
      !define_method("getEnvOptionsInputType", OptionsGetEnvOptionsInputTypeCallback) ||
      !define_method("getNamespaceOptionsInputType", OptionsGetNamespaceOptionsInputTypeCallback)) {
    return undefined;
  }

  return out;
}

static napi_value NativeGetInternalBindingCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, nullptr) != napi_ok) {
    return nullptr;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  if (argc < 1 || argv[0] == nullptr) {
    return undefined;
  }
  const std::string name = ValueToUtf8(env, argv[0]);
  napi_value resolved = edge::binding_registry::Get(env, name);
  if (resolved != nullptr) return resolved;

  bool has_pending_exception = false;
  if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
    return nullptr;
  }
  return undefined;
}

bool ResolveModulePath(const std::string& specifier, const std::string& base_dir, fs::path* out) {
  if (out == nullptr || specifier.empty()) return false;

  fs::path base = base_dir.empty() ? fs::current_path() : fs::path(base_dir);
  base = NormalizeResolvedPath(base);

  fs::path resolved;
  if (!IsRelativeOrAbsoluteRequest(specifier)) {
    if (!ResolveNodeModules(specifier, base.string(), &resolved)) {
      return false;
    }
    *out = NormalizeResolvedPath(resolved);
    return true;
  }

  const fs::path candidate = specifier[0] == '/' ? fs::path(specifier) : base / specifier;
  if (!ResolveAsFile(candidate, &resolved) && !ResolveAsDirectory(candidate, &resolved)) {
    return false;
  }

  *out = NormalizeResolvedPath(resolved);
  return true;
}

napi_value MakeError(napi_env env, const char* code, const std::string& message) {
  napi_value code_value = nullptr;
  napi_value msg_value = nullptr;
  napi_value error_value = nullptr;
  if (napi_create_string_utf8(env, code, NAPI_AUTO_LENGTH, &code_value) != napi_ok ||
      napi_create_string_utf8(env, message.c_str(), NAPI_AUTO_LENGTH, &msg_value) != napi_ok ||
      napi_create_error(env, code_value, msg_value, &error_value) != napi_ok) {
    return nullptr;
  }
  if (napi_set_named_property(env, error_value, "code", code_value) != napi_ok) {
    return nullptr;
  }
  return error_value;
}

bool ThrowModuleNotFound(napi_env env, const std::string& specifier) {
  const std::string message = "Cannot find module '" + specifier + "'";
  napi_value error_value = MakeError(env, "MODULE_NOT_FOUND", message);
  if (error_value == nullptr) {
    return false;
  }
  return napi_throw(env, error_value) == napi_ok;
}

bool ThrowLoaderError(napi_env env, const std::string& message) {
  napi_value error_value = MakeError(env, "ERR_EDGE_MODULE_LOAD", message);
  if (error_value == nullptr) {
    return false;
  }
  return napi_throw(env, error_value) == napi_ok;
}

napi_value GetGlobal(napi_env env) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return nullptr;
  }
  return global;
}

napi_value GetCacheObject(napi_env env, ModuleLoaderState* state) {
  if (state == nullptr) {
    return nullptr;
  }
  if (state->cache_object_ref == nullptr) {
    napi_value cache_obj = nullptr;
    if (napi_create_object(env, &cache_obj) != napi_ok || cache_obj == nullptr) {
      return nullptr;
    }
    if (napi_create_reference(env, cache_obj, 1, &state->cache_object_ref) != napi_ok || state->cache_object_ref == nullptr) {
      return nullptr;
    }
    return cache_obj;
  }
  napi_value cache_obj = nullptr;
  if (napi_get_reference_value(env, state->cache_object_ref, &cache_obj) != napi_ok || cache_obj == nullptr) {
    return nullptr;
  }
  return cache_obj;
}

napi_value GetCachedExportsFromJsCache(napi_env env, ModuleLoaderState* state, const std::string& resolved_key) {
  napi_value cache_obj = GetCacheObject(env, state);
  if (cache_obj == nullptr) {
    return nullptr;
  }
  bool has_entry = false;
  if (napi_has_named_property(env, cache_obj, resolved_key.c_str(), &has_entry) != napi_ok || !has_entry) {
    return nullptr;
  }
  napi_value cache_entry = nullptr;
  if (napi_get_named_property(env, cache_obj, resolved_key.c_str(), &cache_entry) != napi_ok || cache_entry == nullptr) {
    return nullptr;
  }
  bool has_exports = false;
  if (napi_has_named_property(env, cache_entry, "exports", &has_exports) != napi_ok || !has_exports) {
    return nullptr;
  }
  napi_value exports_value = nullptr;
  if (napi_get_named_property(env, cache_entry, "exports", &exports_value) != napi_ok || exports_value == nullptr) {
    return nullptr;
  }
  return exports_value;
}

napi_value CreateResolvedPathString(napi_env env, const fs::path& resolved_path) {
  napi_value out = nullptr;
  if (napi_create_string_utf8(env, resolved_path.string().c_str(), NAPI_AUTO_LENGTH, &out) != napi_ok || out == nullptr) {
    return nullptr;
  }
  return out;
}

napi_value ResolveSpecifierForContext(napi_env env, RequireContext* context, const std::string& specifier, bool throw_on_error) {
  fs::path resolved_path;
  if (ResolveBuiltinPath(specifier, context->base_dir, &resolved_path) ||
      ResolveModulePath(specifier, context->base_dir, &resolved_path)) {
    return CreateResolvedPathString(env, resolved_path);
  }
  if (throw_on_error) {
    ThrowModuleNotFound(env, specifier);
  }
  return nullptr;
}

napi_value RequireResolveCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  void* data = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, &data) != napi_ok || data == nullptr) {
    return nullptr;
  }
  auto* context = static_cast<RequireContext*>(data);
  if (context->state == nullptr) {
    ThrowLoaderError(env, "Invalid require.resolve context");
    return nullptr;
  }
  if (argc < 1 || argv[0] == nullptr) {
    ThrowLoaderError(env, "Missing module specifier");
    return nullptr;
  }
  const std::string specifier = ValueToUtf8(env, argv[0]);
  if (specifier.empty()) {
    ThrowLoaderError(env, "Empty module specifier");
    return nullptr;
  }
  napi_value resolved_path = ResolveSpecifierForContext(env, context, specifier, false);
  if (resolved_path != nullptr) {
    return resolved_path;
  }

  // Node allows cached bare specifiers to resolve to themselves.
  if (GetCachedExportsFromJsCache(env, context->state, specifier) != nullptr) {
    napi_value key_value = nullptr;
    if (napi_create_string_utf8(env, specifier.c_str(), NAPI_AUTO_LENGTH, &key_value) == napi_ok && key_value != nullptr) {
      return key_value;
    }
    return nullptr;
  }

  ThrowModuleNotFound(env, specifier);
  return nullptr;
}

napi_value CreateRequireFunction(napi_env env, RequireContext* context);

bool EvaluateJsModule(napi_env env,
                      ModuleLoaderState* state,
                      const fs::path& resolved_path,
                      napi_value module_obj,
                      napi_value exports_obj,
                      napi_value require_fn) {
  std::string source;
  if (!ReadTextFileWithBuiltinCache(resolved_path, &source)) {
    return ThrowLoaderError(env, ("Failed to read module source: " + resolved_path.string()).c_str());
  }
  const std::string source_url = ModuleSourceUrlForResolvedPath(resolved_path);

  // Node-aligned: compile the wrapper as a function, then call it from C++ with (internalBinding, primordials)
  // as arguments (realm->primordials() in Node). No JS expression like globalThis.primordials at call time.
  std::string builtin_id;
  const bool is_builtin = TryGetBuiltinIdFromResolvedPath(resolved_path, &builtin_id);
  const bool is_per_context = is_builtin && IsPerContextBuiltinId(builtin_id);
  const std::string wrapped_source = is_per_context
                                         ? "(function(primordials, privateSymbols, perIsolateSymbols) {"
                                           "return function(exports, require, module, __filename, __dirname) {\n" +
                                               source + "\n//# sourceURL=" + source_url + "\n};"
                                               "})"
                                         : "(function(internalBinding, primordials) {"
                                           "return function(exports, require, module, __filename, __dirname) {\n" +
                                               source + "\n//# sourceURL=" + source_url + "\n};"
                                               "})";
  napi_value script_source = nullptr;
  if (napi_create_string_utf8(env, wrapped_source.c_str(), NAPI_AUTO_LENGTH, &script_source) != napi_ok ||
      script_source == nullptr) {
    return ThrowLoaderError(env, "Failed to create wrapped module source");
  }

  // Builtins consolidated bytecode cache (keyed by the full wrapped IIFE text,
  // so wrapper drift self-invalidates). Non-builtin files keep the plain
  // napi_run_script path untouched.
  napi_value outer_fn = nullptr;
  if (is_builtin) {
    napi_value undefined = nullptr;
    napi_get_undefined(env, &undefined);
    napi_value source_url_value = nullptr;
    if (napi_create_string_utf8(env, source_url.c_str(), NAPI_AUTO_LENGTH, &source_url_value) == napi_ok &&
        source_url_value != nullptr) {
      const edge_builtin_bytecode::Kind bytecode_kind =
          is_per_context ? edge_builtin_bytecode::Kind::kWrapperPerContext
                         : edge_builtin_bytecode::Kind::kWrapperStandard;
      void* wrapper_bytecode =
          AcquireBuiltinBytecode(env, bytecode_kind, builtin_id, wrapped_source, script_source,
                                 source_url_value, unofficial_napi_bytecode_shape_script,
                                 undefined, undefined);
      if (wrapper_bytecode != nullptr) {
        const unofficial_napi_js_source js_source{nullptr, wrapper_bytecode};
        const napi_status run_status =
            unofficial_napi_contextify_run_script(env,
                                                  nullptr,
                                                  &js_source,
                                                  source_url_value,
                                                  0,
                                                  0,
                                                  -1,
                                                  false,
                                                  false,
                                                  false,
                                                  undefined,
                                                  &outer_fn);
        (void)unofficial_napi_bytecode_release(env, wrapper_bytecode);
        if (run_status != napi_ok || outer_fn == nullptr) {
          // Fall back to the plain path for exact error shaping.
          outer_fn = nullptr;
          bool has_pending = false;
          napi_value ignored = nullptr;
          if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
            (void)napi_get_and_clear_last_exception(env, &ignored);
          }
        }
      }
    }
  }
  if (outer_fn == nullptr &&
      (napi_run_script(env, script_source, &outer_fn) != napi_ok || outer_fn == nullptr)) {
    return false;  // Preserve JS exception.
  }

  napi_value global = GetGlobal(env);
  if (global == nullptr) {
    return ThrowLoaderError(env, "Failed to fetch global object");
  }

  napi_value internal_binding_val = nullptr;
  napi_value primordials_val = GetStatePrimordials(env, state);
  if (primordials_val == nullptr) {
    napi_get_named_property(env, global, "primordials", &primordials_val);
  }
  if (primordials_val == nullptr) {
    napi_get_undefined(env, &primordials_val);
  }
  std::vector<napi_value> wrapper_args;
  if (is_per_context) {
    napi_value private_symbols = GetStatePrivateSymbols(env, state);
    napi_value per_isolate_symbols = GetStatePerIsolateSymbols(env, state);
    if (private_symbols == nullptr) napi_get_undefined(env, &private_symbols);
    if (per_isolate_symbols == nullptr) napi_get_undefined(env, &per_isolate_symbols);
    wrapper_args = {primordials_val, private_symbols, per_isolate_symbols};
  } else {
    internal_binding_val = GetStateInternalBinding(env, state);
    if (internal_binding_val == nullptr) {
      internal_binding_val = GetGlobalInternalBindingFunction(env, global);
    }
    if (internal_binding_val == nullptr) {
      napi_get_undefined(env, &internal_binding_val);
    }
    wrapper_args = {internal_binding_val, primordials_val};
  }
  napi_value inner_fn = nullptr;
  if (napi_call_function(env,
                         global,
                         outer_fn,
                         wrapper_args.size(),
                         wrapper_args.data(),
                         &inner_fn) != napi_ok ||
      inner_fn == nullptr) {
    return false;  // Preserve JS exception.
  }

  napi_value filename_value = nullptr;
  napi_value dirname_value = nullptr;
  if (napi_create_string_utf8(env, resolved_path.string().c_str(), NAPI_AUTO_LENGTH, &filename_value) != napi_ok ||
      napi_create_string_utf8(env, resolved_path.parent_path().string().c_str(), NAPI_AUTO_LENGTH, &dirname_value) !=
          napi_ok) {
    return ThrowLoaderError(env, "Failed to build module path values");
  }

  napi_value argv[5] = {exports_obj, require_fn, module_obj, filename_value, dirname_value};
  napi_value call_result = nullptr;
  if (napi_call_function(env, global, inner_fn, 5, argv, &call_result) != napi_ok) {
    return false;  // Preserve JS exception.
  }
  return true;
}

bool ParseJsonModule(napi_env env, const fs::path& resolved_path, napi_value module_obj) {
  std::string source;
  if (!ReadTextFileWithBuiltinCache(resolved_path, &source)) {
    return ThrowLoaderError(env, "Failed to read JSON module");
  }

  const std::string parse_source = "(function(__text){ return JSON.parse(__text); })";
  napi_value parse_script = nullptr;
  if (napi_create_string_utf8(env, parse_source.c_str(), NAPI_AUTO_LENGTH, &parse_script) != napi_ok ||
      parse_script == nullptr) {
    return ThrowLoaderError(env, "Failed to prepare JSON parser");
  }
  napi_value parse_fn = nullptr;
  if (napi_run_script(env, parse_script, &parse_fn) != napi_ok || parse_fn == nullptr) {
    return false;
  }

  napi_value json_text = nullptr;
  if (napi_create_string_utf8(env, source.c_str(), NAPI_AUTO_LENGTH, &json_text) != napi_ok || json_text == nullptr) {
    return ThrowLoaderError(env, "Failed to create JSON source string");
  }

  napi_value global = GetGlobal(env);
  if (global == nullptr) {
    return ThrowLoaderError(env, "Failed to fetch global object");
  }
  napi_value parsed = nullptr;
  if (napi_call_function(env, global, parse_fn, 1, &json_text, &parsed) != napi_ok || parsed == nullptr) {
    bool has_exception = false;
    if (napi_is_exception_pending(env, &has_exception) == napi_ok && has_exception) {
      napi_value exc = nullptr;
      if (napi_get_and_clear_last_exception(env, &exc) == napi_ok && exc != nullptr) {
        napi_value exc_msg = nullptr;
        if (napi_coerce_to_string(env, exc, &exc_msg) == napi_ok && exc_msg != nullptr) {
          const std::string msg_str = ValueToUtf8(env, exc_msg);
          const std::string path_for_msg = resolved_path.string();
          const std::string prefixed = path_for_msg + ": " + (msg_str.empty() ? "JSON parse error" : msg_str);
          napi_value new_msg = nullptr;
          if (napi_create_string_utf8(env, prefixed.c_str(), NAPI_AUTO_LENGTH, &new_msg) == napi_ok && new_msg != nullptr) {
            const char* throw_script = "(function(m){ throw new SyntaxError(m); })";
            napi_value throw_script_val = nullptr;
            if (napi_create_string_utf8(env, throw_script, NAPI_AUTO_LENGTH, &throw_script_val) == napi_ok &&
                throw_script_val != nullptr) {
              napi_value throw_fn = nullptr;
              if (napi_run_script(env, throw_script_val, &throw_fn) == napi_ok && throw_fn != nullptr) {
                napi_value ignore = nullptr;
                napi_call_function(env, global, throw_fn, 1, &new_msg, &ignore);
                return false;
              }
            }
            napi_throw(env, exc);
          } else {
            napi_throw(env, exc);
          }
        } else {
          napi_throw(env, exc);
        }
      }
    }
    return false;
  }

  return napi_set_named_property(env, module_obj, "exports", parsed) == napi_ok;
}

bool LoadResolvedModule(napi_env env, ModuleLoaderState* state, const fs::path& resolved_path, napi_value* out_exports);

bool LoadUnavailableInspectorBuiltin(napi_env env, ModuleLoaderState* state, napi_value* out_exports) {
  if (out_exports != nullptr) *out_exports = nullptr;
  if (env == nullptr || state == nullptr) return false;

  napi_value internal_binding = GetStateInternalBinding(env, state);
  if (!IsFunctionValue(env, internal_binding)) return false;

  napi_value name = nullptr;
  if (napi_create_string_utf8(env, "inspector", NAPI_AUTO_LENGTH, &name) != napi_ok || name == nullptr) {
    return false;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value exports = nullptr;
  if (napi_call_function(env, global, internal_binding, 1, &name, &exports) != napi_ok ||
      exports == nullptr ||
      IsUndefinedValue(env, exports)) {
    return false;
  }

  if (out_exports != nullptr) *out_exports = exports;
  return true;
}

bool CallRequireBuiltinLoader(napi_env env, ModuleLoaderState* state, const std::string& id, napi_value* out_exports) {
  if (out_exports != nullptr) *out_exports = nullptr;
  if (env == nullptr || state == nullptr || id.empty()) return false;

  if (id == "inspector") {
    return LoadUnavailableInspectorBuiltin(env, state, out_exports);
  }

  if (state->require_builtin_loader_ref == nullptr) return false;

  napi_value require_builtin = nullptr;
  if (napi_get_reference_value(env, state->require_builtin_loader_ref, &require_builtin) != napi_ok ||
      require_builtin == nullptr) {
    return false;
  }

  napi_value id_value = nullptr;
  if (napi_create_string_utf8(env, id.c_str(), NAPI_AUTO_LENGTH, &id_value) != napi_ok || id_value == nullptr) {
    return false;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value exports = nullptr;
  if (napi_call_function(env, global, require_builtin, 1, &id_value, &exports) != napi_ok || exports == nullptr) {
    return false;
  }

  if (out_exports != nullptr) *out_exports = exports;
  return true;
}

napi_value RequireCallback(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1] = {nullptr};
  void* data = nullptr;
  if (napi_get_cb_info(env, info, &argc, argv, nullptr, &data) != napi_ok || data == nullptr) {
    return nullptr;
  }
  auto* context = static_cast<RequireContext*>(data);
  if (context->state == nullptr) {
    ThrowLoaderError(env, "Invalid require context");
    return nullptr;
  }

  if (argc < 1 || argv[0] == nullptr) {
    ThrowLoaderError(env, "Missing module specifier");
    return nullptr;
  }
  const std::string specifier = ValueToUtf8(env, argv[0]);
  if (specifier.empty()) {
    ThrowLoaderError(env, "Empty module specifier");
    return nullptr;
  }
  napi_value from_js_cache = GetCachedExportsFromJsCache(env, context->state, specifier);
  if (from_js_cache != nullptr) {
    return from_js_cache;
  }

  std::string resolved_key;
  fs::path resolved_path;
  if (ResolveBuiltinPath(specifier, context->base_dir, &resolved_path)) {
    std::string builtin_id;
    if (TryGetBuiltinIdFromResolvedPath(resolved_path, &builtin_id) &&
        context->state->require_builtin_loader_ref != nullptr) {
      napi_value builtin_exports = nullptr;
      if (CallRequireBuiltinLoader(env, context->state, builtin_id, &builtin_exports) && builtin_exports != nullptr) {
        return builtin_exports;
      }
      return nullptr;  // Preserve exception from requireBuiltin().
    }
    resolved_key = CanonicalPathKey(resolved_path);
    resolved_path = fs::path(resolved_key);
  } else {
    napi_value resolved_path_value = ResolveSpecifierForContext(env, context, specifier, true);
    if (resolved_path_value == nullptr) {
      return nullptr;
    }
    const std::string resolved_path_text = ValueToUtf8(env, resolved_path_value);
    resolved_key = CanonicalPathKey(fs::path(resolved_path_text));
    if (resolved_key.empty()) {
      ThrowLoaderError(env, "Failed to resolve module path");
      return nullptr;
    }
    resolved_path = fs::path(resolved_key);
  }


  from_js_cache = GetCachedExportsFromJsCache(env, context->state, resolved_key);
  if (from_js_cache != nullptr) {
    return from_js_cache;
  }

  napi_value exports_value = nullptr;
  if (!LoadResolvedModule(env, context->state, resolved_path, &exports_value) || exports_value == nullptr) {
    return nullptr;
  }
  return exports_value;
}

napi_value CreateRequireFunction(napi_env env, RequireContext* context) {
  napi_value require_fn = nullptr;
  if (napi_create_function(env, "require", NAPI_AUTO_LENGTH, RequireCallback, context, &require_fn) != napi_ok ||
      require_fn == nullptr) {
    return nullptr;
  }
  napi_value cache_obj = GetCacheObject(env, context->state);
  if (cache_obj == nullptr) {
    return nullptr;
  }
  if (napi_set_named_property(env, require_fn, "cache", cache_obj) != napi_ok) {
    return nullptr;
  }
  napi_value resolve_fn = nullptr;
  if (napi_create_function(env, "resolve", NAPI_AUTO_LENGTH, RequireResolveCallback, context, &resolve_fn) != napi_ok ||
      resolve_fn == nullptr) {
    return nullptr;
  }
  if (napi_set_named_property(env, require_fn, "resolve", resolve_fn) != napi_ok) {
    return nullptr;
  }
  return require_fn;
}

bool GetCachedModuleExports(napi_env env, ModuleLoaderState* state, const std::string& resolved_key, napi_value* out) {
  auto it = state->module_cache.find(resolved_key);
  if (it == state->module_cache.end()) {
    return false;
  }
  napi_value module_obj = nullptr;
  if (napi_get_reference_value(env, it->second, &module_obj) != napi_ok || module_obj == nullptr) {
    return false;
  }
  return napi_get_named_property(env, module_obj, "exports", out) == napi_ok && *out != nullptr;
}

bool CacheModule(napi_env env, ModuleLoaderState* state, const std::string& resolved_key, napi_value module_obj) {
  napi_ref ref = nullptr;
  if (napi_create_reference(env, module_obj, 1, &ref) != napi_ok || ref == nullptr) {
    return false;
  }
  auto existing = state->module_cache.find(resolved_key);
  if (existing != state->module_cache.end()) {
    napi_delete_reference(env, existing->second);
  }
  state->module_cache[resolved_key] = ref;
  napi_value cache_obj = GetCacheObject(env, state);
  if (cache_obj == nullptr) {
    return false;
  }
  if (napi_set_named_property(env, cache_obj, resolved_key.c_str(), module_obj) != napi_ok) {
    return false;
  }
  return true;
}

void RemoveCachedModule(napi_env env, ModuleLoaderState* state, const std::string& resolved_key) {
  auto it = state->module_cache.find(resolved_key);
  if (it == state->module_cache.end()) {
    return;
  }
  napi_delete_reference(env, it->second);
  state->module_cache.erase(it);
  napi_value cache_obj = GetCacheObject(env, state);
  if (cache_obj != nullptr) {
    napi_value cache_key = nullptr;
    if (napi_create_string_utf8(env, resolved_key.c_str(), NAPI_AUTO_LENGTH, &cache_key) == napi_ok &&
        cache_key != nullptr) {
      bool ignored = false;
      napi_delete_property(env, cache_obj, cache_key, &ignored);
    }
  }
}

bool LoadResolvedModule(napi_env env, ModuleLoaderState* state, const fs::path& resolved_path, napi_value* out_exports) {
  const std::string resolved_key = CanonicalPathKey(resolved_path);
  if (GetCachedModuleExports(env, state, resolved_key, out_exports)) {
    return true;
  }

  napi_value module_obj = nullptr;
  if (napi_create_object(env, &module_obj) != napi_ok || module_obj == nullptr) {
    ThrowLoaderError(env, "Failed to create module object");
    return false;
  }
  napi_value exports_obj = nullptr;
  if (napi_create_object(env, &exports_obj) != napi_ok || exports_obj == nullptr) {
    ThrowLoaderError(env, "Failed to create exports object");
    return false;
  }
  napi_value filename_value = nullptr;
  if (napi_create_string_utf8(env, resolved_key.c_str(), NAPI_AUTO_LENGTH, &filename_value) != napi_ok ||
      filename_value == nullptr) {
    ThrowLoaderError(env, "Failed to create module filename");
    return false;
  }
  if (napi_set_named_property(env, module_obj, "id", filename_value) != napi_ok ||
      napi_set_named_property(env, module_obj, "filename", filename_value) != napi_ok ||
      napi_set_named_property(env, module_obj, "exports", exports_obj) != napi_ok) {
    ThrowLoaderError(env, "Failed to initialize module object");
    return false;
  }
  if (!CacheModule(env, state, resolved_key, module_obj)) {
    ThrowLoaderError(env, "Failed to cache module");
    return false;
  }

  RequireContext* child_context = AddRequireContext(state, resolved_path.parent_path().string());
  if (child_context == nullptr) {
    RemoveCachedModule(env, state, resolved_key);
    ThrowLoaderError(env, "Failed to create require context");
    return false;
  }
  napi_value require_fn = CreateRequireFunction(env, child_context);
  if (require_fn == nullptr) {
    RemoveCachedModule(env, state, resolved_key);
    ThrowLoaderError(env, "Failed to create require function");
    return false;
  }

  const std::string ext = resolved_path.extension().string();
  bool ok = false;
  if (ext == ".json") {
    ok = ParseJsonModule(env, resolved_path, module_obj);
  } else {
    ok = EvaluateJsModule(env, state, resolved_path, module_obj, exports_obj, require_fn);
  }
  if (!ok) {
    RemoveCachedModule(env, state, resolved_key);
    return false;
  }

  napi_value updated_exports = nullptr;
  if (napi_get_named_property(env, module_obj, "exports", &updated_exports) != napi_ok || updated_exports == nullptr) {
    RemoveCachedModule(env, state, resolved_key);
    ThrowLoaderError(env, "Failed to fetch module exports");
    return false;
  }
  *out_exports = updated_exports;
  return true;
}

}  // namespace

napi_value EdgeInstallBuiltinsBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return UndefinedValue(env);
  return GetOrCreateNativeBuiltinsBinding(env, state);
}

napi_value EdgeInstallContextifyBinding(napi_env env) {
  return CreateContextifyBinding(env);
}

napi_value EdgeInstallModulesBinding(napi_env env) {
  return CreateModulesBinding(env);
}

napi_value EdgeInstallOptionsBinding(napi_env env) {
  return CreateOptionsBinding(env);
}

napi_value EdgeInstallTraceEventsBinding(napi_env env) {
  return GetOrCreateTraceEventsBinding(env);
}

napi_value EdgeInstallUvBinding(napi_env env) {
  return CreateUvBinding(env);
}

void EdgeSetPrimordials(napi_env env, napi_value primordials) {
  if (env == nullptr || primordials == nullptr) return;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return;
  ResetStateRef(env, &state->primordials_ref, primordials);
}

void EdgeSetInternalBinding(napi_env env, napi_value internal_binding) {
  if (env == nullptr || internal_binding == nullptr) return;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return;
  ResetStateRef(env, &state->internal_binding_ref, internal_binding);
}

void EdgeSetPrivateSymbols(napi_env env, napi_value private_symbols) {
  if (env == nullptr || private_symbols == nullptr) return;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return;
  ResetStateRef(env, &state->private_symbols_ref, private_symbols);
}

void EdgeSetPerIsolateSymbols(napi_env env, napi_value per_isolate_symbols) {
  if (env == nullptr || per_isolate_symbols == nullptr) return;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return;
  ResetStateRef(env, &state->per_isolate_symbols_ref, per_isolate_symbols);
}

napi_value EdgeGetPerContextExports(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetRefValue(env, state->per_context_exports_ref);
}

napi_value EdgeGetPrivateSymbols(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetRefValue(env, state->private_symbols_ref);
}

napi_value EdgeGetPerIsolateSymbols(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetRefValue(env, state->per_isolate_symbols_ref);
}

napi_value EdgeGetRequireFunction(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetRefValue(env, state->require_ref);
}

napi_value EdgeGetInternalBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetStateInternalBinding(env, state);
}

napi_value EdgeGetBuiltinInternalBinding(napi_env env) {
  if (env == nullptr) return nullptr;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return nullptr;
  return GetStateInternalBindingLoader(env, state);
}

void EdgeFinalizeModuleLoaderEnv(napi_env env) {
  if (env == nullptr) return;
  FinalizeModuleLoaderState(env);
}

bool EdgeRequireBuiltin(napi_env env, const char* id, napi_value* out) {
  if (env == nullptr || id == nullptr || id[0] == '\0') return false;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return false;
  return CallRequireBuiltinLoader(env, state, id, out);
}

bool EdgeExecuteBuiltin(napi_env env, const char* id, napi_value* out) {
  if (env == nullptr || id == nullptr || id[0] == '\0') return false;
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr || state->finalized) return false;
  return ExecuteBuiltinFromNative(env, state, id, out);
}

napi_status EdgeInstallModuleLoader(napi_env env, const char* entry_script_path) {
  if (env == nullptr) {
    return napi_invalid_arg;
  }
  ModuleLoaderState* state = GetModuleLoaderState(env);
  if (state == nullptr) {
    auto* state_ptr = new ModuleLoaderState(env);
    if (!SetModuleLoaderState(env, state_ptr)) {
      delete state_ptr;
      return napi_generic_failure;
    }
    state = state_ptr;
  }
  state->finalized = false;

  const bool is_eval_entry = entry_script_path == nullptr || entry_script_path[0] == '\0';
  fs::path entry_path;
  if (!is_eval_entry) {
    entry_path = fs::path(edge_path::FromNamespacedPath(edge_path::PathResolve({entry_script_path})));
  } else {
    entry_path = fs::path(edge_path::PathResolve({})) / "<eval>";
  }

  ResetModuleLoaderState(env, state);
  state->entry_dir = entry_path.parent_path().string();

  RequireContext* root_context = AddRequireContext(state, state->entry_dir);
  if (root_context == nullptr) {
    return napi_generic_failure;
  }

  napi_value require_fn = CreateRequireFunction(env, root_context);
  if (require_fn == nullptr) {
    return napi_generic_failure;
  }
  napi_value cache_obj = GetCacheObject(env, state);
  if (cache_obj == nullptr) {
    return napi_generic_failure;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return napi_generic_failure;
  }

  if (napi_set_named_property(env, require_fn, "cache", cache_obj) != napi_ok) {
    return napi_generic_failure;
  }
  ResetStateRef(env, &state->require_ref, require_fn);

  if (is_eval_entry && !EvalEntryUsesModuleInputType(env)) {
    napi_value filename_value = nullptr;
    napi_value dirname_value = nullptr;
    if (napi_create_string_utf8(env, "[eval]", NAPI_AUTO_LENGTH, &filename_value) != napi_ok ||
        napi_create_string_utf8(env, ".", NAPI_AUTO_LENGTH, &dirname_value) != napi_ok) {
      return napi_generic_failure;
    }
    if (napi_set_named_property(env, global, "require", require_fn) != napi_ok ||
        napi_set_named_property(env, global, "__filename", filename_value) != napi_ok ||
        napi_set_named_property(env, global, "__dirname", dirname_value) != napi_ok) {
      return napi_generic_failure;
    }
  }

  napi_value native_get_internal_binding_fn = nullptr;
  if (napi_create_function(env,
                           "internalBinding",
                           NAPI_AUTO_LENGTH,
                           NativeGetInternalBindingCallback,
                           state,
                           &native_get_internal_binding_fn) != napi_ok ||
      native_get_internal_binding_fn == nullptr ||
      napi_set_named_property(env,
                              global,
                              "internalBinding",
                              native_get_internal_binding_fn) != napi_ok) {
    return napi_generic_failure;
  }
  return napi_ok;
}
