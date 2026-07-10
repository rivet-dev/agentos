#include "edge_runtime.h"

#include "binding_registry/binding_registry.h"
#include <cstdlib>
#include <chrono>
#include <cerrno>
#include <cmath>
#include <cctype>
#include <cstring>
#include <ctime>
#include <fstream>
#include <iostream>
#include <limits>
#include <map>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <arpa/inet.h>
#include <cerrno>
#include <csignal>

#include <uv.h>
#include <openssl/crypto.h>
#include <openssl/conf.h>
#include <openssl/err.h>

#include "unofficial_napi.h"
#include "ncrypto.h"

#if defined(_WIN32)
#include <io.h>
#endif

#if !defined(_WIN32)
#include <unistd.h>
#endif

#include "edge_fs.h"
#include "edge_environment.h"
#include "edge_errors_binding.h"
#include "edge_buffer.h"
#include "edge_env_loop.h"
#include "edge_handle_scope.h"
#include "edge_icu_data.h"
#include "edge_intl.h"
#include "edge_crypto.h"
#include "edge_encoding.h"
#include "edge_http_parser.h"
#include "edge_module_loader.h"
#include "edge_os.h"
#include "edge_option_helpers.h"
#include "edge_path.h"
#include "edge_pipe_wrap.h"
#include "edge_process.h"
#include "edge_signal_wrap.h"
#include "edge_runtime_platform.h"
#include "edge_stream_wrap.h"
#include "edge_process_wrap.h"
#include "edge_string_decoder.h"
#include "edge_tcp_wrap.h"
#include "edge_tty_wrap.h"
#include "edge_udp_wrap.h"
#include "edge_url.h"
#include "edge_util.h"
#include "edge_worker_env.h"
#include "edge_cares_wrap.h"
#include "edge_timers_host.h"
#include "edge_spawn_sync.h"
#include "internal_binding/helpers.h"

#if defined(EDGE_NAPI_QUICKJS) && defined(EDGE_QUICKJS_WEBASSEMBLY)
#include "webassembly/edge_wasm.h"
#endif

namespace {

thread_local std::string g_edge_current_script_path;
thread_local std::vector<std::string> g_edge_exec_argv;
thread_local std::vector<std::string> g_edge_effective_exec_argv;
std::vector<std::string> g_edge_cli_exec_argv;
thread_local std::string g_edge_process_title;
thread_local std::vector<std::string> g_edge_script_argv;
const auto g_process_start_time = std::chrono::steady_clock::now();
std::once_flag g_process_stdio_init_once;
constexpr int kExitCodeInvalidFatalExceptionMonkeyPatching = 6;
constexpr int kExitCodeExceptionInFatalExceptionHandler = 7;
constexpr int kExitCodeUnsettledTopLevelAwait = 13;

void ResetDomainHelperRef(napi_env env, napi_ref* ref);

struct DomainCallbackCache {
  explicit DomainCallbackCache(napi_env env_in) : env(env_in) {}
  ~DomainCallbackCache() {
    ResetDomainHelperRef(env, &helper_ref);
  }

  napi_env env = nullptr;
  napi_ref helper_ref = nullptr;
};

enum class EdgeBootstrapMode {
  kMainThread,
  kWorkerThread,
};

void InitializeProcessStdioInheritanceOnce() {
  std::call_once(g_process_stdio_init_once, []() {
    uv_disable_stdio_inheritance();
  });
}

#if !defined(_WIN32)
void InstallDefaultSignalBehavior() {
  static std::once_flag once;
  std::call_once(once, []() {
#if defined(SIGPIPE)
    // Node ignores SIGPIPE by default; writes should surface as EPIPE instead
    // of terminating the process.
    signal(SIGPIPE, SIG_IGN);
#endif
  });
}
#endif

void ResetDomainHelperRef(napi_env env, napi_ref* ref) {
  if (env == nullptr || ref == nullptr || *ref == nullptr) return;
  napi_delete_reference(env, *ref);
  *ref = nullptr;
}

bool IsNullOrUndefinedValue(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok &&
         (type == napi_undefined || type == napi_null);
}

bool IsDomainHelperFunctionValue(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

napi_value GetCachedDomainCallbackHelper(napi_env env) {
  if (env == nullptr) return nullptr;
  auto& cache = EdgeEnvironmentGetOrCreateSlotData<DomainCallbackCache>(
      env, kEdgeEnvironmentSlotDomainCallbackCache);
  if (cache.helper_ref != nullptr) {
    napi_value helper = nullptr;
    if (napi_get_reference_value(env, cache.helper_ref, &helper) == napi_ok && helper != nullptr) {
      return helper;
    }
    ResetDomainHelperRef(env, &cache.helper_ref);
  }

  static const char kHelperSource[] =
      "(function(domain, recv, callback, args) {"
      "  domain.enter();"
      "  var ret = Reflect.apply(callback, recv, args);"
      "  domain.exit();"
      "  return ret;"
      "})";

  napi_value source = nullptr;
  napi_value helper = nullptr;
  if (napi_create_string_utf8(env, kHelperSource, NAPI_AUTO_LENGTH, &source) != napi_ok ||
      source == nullptr ||
      napi_run_script(env, source, &helper) != napi_ok ||
      helper == nullptr ||
      !IsDomainHelperFunctionValue(env, helper)) {
    return nullptr;
  }

  if (napi_create_reference(env, helper, 1, &cache.helper_ref) != napi_ok) {
    cache.helper_ref = nullptr;
    return nullptr;
  }
  return helper;
}

bool GetCallbackDomain(napi_env env, napi_value recv, napi_value* domain_out) {
  if (domain_out == nullptr) return false;
  *domain_out = nullptr;
  if (env == nullptr || recv == nullptr) return false;

  napi_valuetype recv_type = napi_undefined;
  if (napi_typeof(env, recv, &recv_type) != napi_ok ||
      (recv_type != napi_object && recv_type != napi_function)) {
    return false;
  }

  bool has_domain = false;
  if (napi_has_named_property(env, recv, "domain", &has_domain) != napi_ok || !has_domain) {
    return false;
  }

  napi_value domain = nullptr;
  if (napi_get_named_property(env, recv, "domain", &domain) != napi_ok ||
      IsNullOrUndefinedValue(env, domain)) {
    return false;
  }

  napi_value enter = nullptr;
  napi_value exit = nullptr;
  if (napi_get_named_property(env, domain, "enter", &enter) != napi_ok ||
      napi_get_named_property(env, domain, "exit", &exit) != napi_ok ||
      !IsDomainHelperFunctionValue(env, enter) ||
      !IsDomainHelperFunctionValue(env, exit)) {
    return false;
  }

  *domain_out = domain;
  return true;
}

bool DebugExceptionsEnabled() {
  const char* env = std::getenv("EDGE_DEBUG_EXCEPTIONS");
  return env != nullptr && env[0] != '\0' && env[0] != '0';
}

void WriteTextToFd(int fd, const std::string& text) {
  if (text.empty()) return;
  size_t offset = 0;
  while (offset < text.size()) {
#if defined(_WIN32)
    const unsigned int remaining = static_cast<unsigned int>(text.size() - offset);
    const int written = _write(fd, text.data() + offset, remaining);
    if (written <= 0) break;
    offset += static_cast<size_t>(written);
#else
    const size_t remaining = text.size() - offset;
    const ssize_t written = ::write(fd, text.data() + offset, remaining);
    if (written > 0) {
      offset += static_cast<size_t>(written);
      continue;
    }
    if (written < 0 && errno == EINTR) continue;
    break;
#endif
  }
}

bool ReadTextFile(const char* path, std::string* out) {
  if (out == nullptr) return false;
  out->clear();
  if (path == nullptr || path[0] == '\0') {
    return false;
  }
  std::ifstream in(path);
  if (!in.is_open()) {
    return false;
  }
  std::ostringstream ss;
  ss << in.rdbuf();
  *out = ss.str();
  return true;
}

std::string GetProcessVersion(napi_env env) {
  napi_value global = nullptr;
  napi_value process_obj = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr ||
      napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return {};
  }
  napi_value version = nullptr;
  if (napi_get_named_property(env, process_obj, "version", &version) != napi_ok || version == nullptr) {
    return {};
  }
  size_t len = 0;
  if (napi_get_value_string_utf8(env, version, nullptr, 0, &len) != napi_ok || len == 0) {
    return {};
  }
  std::string out(len + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, version, out.data(), out.size(), &copied) != napi_ok || copied == 0) {
    return {};
  }
  out.resize(copied);
  return out;
}

std::string NapiValueToUtf8(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return {};
  napi_value string_value = nullptr;
  if (napi_coerce_to_string(env, value, &string_value) != napi_ok || string_value == nullptr) {
    return {};
  }
  size_t length = 0;
  if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &length) != napi_ok) {
    return {};
  }
  std::string out(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) {
    return {};
  }
  out.resize(copied);
  return out;
}

std::string GetNamedStringProperty(napi_env env, napi_value object, const char* name) {
  if (env == nullptr || object == nullptr || name == nullptr) return {};
  napi_value value = nullptr;
  if (napi_get_named_property(env, object, name, &value) != napi_ok || value == nullptr) {
    return {};
  }
  return NapiValueToUtf8(env, value);
}

napi_value GetRuntimeInternalBinding(napi_env env, napi_value global);

std::string GetAssertThrowsExceptionLineForStderr(napi_env env, napi_value exception) {
  if (env == nullptr || exception == nullptr) return {};
  if (GetNamedStringProperty(env, exception, "code") != "ERR_ASSERTION") {
    return {};
  }
  if (GetNamedStringProperty(env, exception, "operator") != "throws") {
    return {};
  }

  napi_value actual = nullptr;
  if (napi_get_named_property(env, exception, "actual", &actual) != napi_ok || actual == nullptr) {
    return {};
  }
  const std::string actual_stack = GetNamedStringProperty(env, actual, "stack");
  size_t marker = actual_stack.find("node:assert:");
  if (marker == std::string::npos) {
    marker = actual_stack.find("node:assert/");
  }

  std::string line_number = "0";
  if (marker != std::string::npos) {
    marker = actual_stack.find(':', marker);
    if (marker != std::string::npos) {
      ++marker;
      size_t end = marker;
      while (end < actual_stack.size() && std::isdigit(static_cast<unsigned char>(actual_stack[end]))) {
        ++end;
      }
      if (end > marker) {
        line_number = actual_stack.substr(marker, end - marker);
      }
    }
  }

  return "node:assert:" + line_number + "\n      throw err;\n      ^\n";
}

std::string GetInternalAssertExceptionLineForStderr(napi_env env, napi_value exception) {
  if (env == nullptr || exception == nullptr) return {};
  if (GetNamedStringProperty(env, exception, "code") != "ERR_INTERNAL_ASSERTION") {
    return {};
  }

  const std::string stack = GetNamedStringProperty(env, exception, "stack");
  size_t frame_pos = stack.find("at assert");
  if (frame_pos == std::string::npos) return {};
  size_t resource_pos = stack.find("node:internal/assert:", frame_pos);
  if (resource_pos == std::string::npos) return {};

  size_t line_start = resource_pos + std::strlen("node:internal/assert:");
  size_t line_end = line_start;
  while (line_end < stack.size() && std::isdigit(static_cast<unsigned char>(stack[line_end]))) {
    ++line_end;
  }
  const std::string line_number =
      line_end > line_start ? stack.substr(line_start, line_end - line_start) : "0";

  const bool is_fail = stack.compare(frame_pos, std::strlen("at assert.fail"), "at assert.fail") == 0;
  const char* source_line = is_fail ? "  throw new ERR_INTERNAL_ASSERTION(message);"
                                    : "    throw new ERR_INTERNAL_ASSERTION(message);";
  const char* caret_line = is_fail ? "  ^\n" : "    ^\n";
  return "node:internal/assert:" + line_number + "\n" + source_line + "\n" + caret_line;
}

struct PendingExceptionInfo {
  bool has_exception = false;
  napi_value exception = nullptr;
  std::string exception_line;
  std::string thrown_at;
};

std::string GetErrorSourceLine(napi_env env,
                               napi_value exception,
                               bool align_internal_assert = false) {
  if (env == nullptr || exception == nullptr) {
    return {};
  }

  napi_value stderr_line = nullptr;
  if (unofficial_napi_get_error_source_line_for_stderr(env, exception, &stderr_line) == napi_ok &&
      stderr_line != nullptr) {
    std::string formatted = NapiValueToUtf8(env, stderr_line);
    if (!formatted.empty() && !align_internal_assert) {
      return formatted;
    }
  }

  unofficial_napi_error_source_positions positions = {};
  if (unofficial_napi_get_error_source_positions(env, exception, &positions) != napi_ok ||
      positions.source_line == nullptr ||
      positions.script_resource_name == nullptr ||
      positions.line_number <= 0) {
    return {};
  }

  const std::string source_line = NapiValueToUtf8(env, positions.source_line);
  const std::string script_resource_name = NapiValueToUtf8(env, positions.script_resource_name);
  if (source_line.empty() || script_resource_name.empty()) {
    return {};
  }

  int32_t start_column = positions.start_column;
  int32_t end_column = positions.end_column;
  if (align_internal_assert && script_resource_name == "node:internal/assert") {
    start_column = 0;
    while (start_column < static_cast<int32_t>(source_line.size()) &&
           (source_line[static_cast<size_t>(start_column)] == ' ' ||
            source_line[static_cast<size_t>(start_column)] == '\t')) {
      ++start_column;
    }
    end_column = start_column + 1;
  }
  if (start_column < 0) start_column = 0;
  if (end_column <= start_column) end_column = start_column + 1;

  std::string underline(static_cast<size_t>(start_column), ' ');
  underline.append(static_cast<size_t>(std::max<int32_t>(1, end_column - start_column)), '^');
  return script_resource_name + ":" + std::to_string(positions.line_number) + "\n" +
         source_line + "\n" +
         underline + "\n";
}

std::string GetArrowMessageFromError(napi_env env, napi_value exception) {
  if (env == nullptr || exception == nullptr) return {};

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return {};
  }

  napi_value internal_binding = GetRuntimeInternalBinding(env, global);
  if (internal_binding == nullptr) return {};

  napi_value util_name = nullptr;
  if (napi_create_string_utf8(env, "util", NAPI_AUTO_LENGTH, &util_name) != napi_ok || util_name == nullptr) {
    return {};
  }

  napi_value util_binding = nullptr;
  napi_value argv[1] = {util_name};
  if (napi_call_function(env, global, internal_binding, 1, argv, &util_binding) != napi_ok ||
      util_binding == nullptr) {
    return {};
  }

  napi_value private_symbols = nullptr;
  if (napi_get_named_property(env, util_binding, "privateSymbols", &private_symbols) != napi_ok ||
      private_symbols == nullptr) {
    return {};
  }

  napi_value arrow_symbol = nullptr;
  if (napi_get_named_property(env,
                              private_symbols,
                              "arrow_message_private_symbol",
                              &arrow_symbol) != napi_ok ||
      arrow_symbol == nullptr) {
    return {};
  }

  napi_value arrow_message = nullptr;
  if (napi_get_property(env, exception, arrow_symbol, &arrow_message) != napi_ok || arrow_message == nullptr) {
    return {};
  }

  napi_valuetype type = napi_undefined;
  if (napi_typeof(env, arrow_message, &type) != napi_ok ||
      type == napi_undefined ||
      type == napi_null) {
    return {};
  }

  return NapiValueToUtf8(env, arrow_message);
}

bool TakePendingExceptionInfo(napi_env env,
                              PendingExceptionInfo* out,
                              napi_value prior_exception = nullptr,
                              const std::string& prior_exception_line = {}) {
  if (env == nullptr || out == nullptr) return false;
  *out = {};

  bool has_pending = false;
  if (napi_is_exception_pending(env, &has_pending) != napi_ok) {
    return false;
  }
  if (!has_pending) {
    return true;
  }

  if (napi_get_and_clear_last_exception(env, &out->exception) != napi_ok || out->exception == nullptr) {
    return false;
  }

  out->has_exception = true;
  if (DebugExceptionsEnabled()) {
    napi_valuetype exception_type = napi_undefined;
    std::string exception_text;
    if (napi_typeof(env, out->exception, &exception_type) == napi_ok) {
      exception_text = NapiValueToUtf8(env, out->exception);
    }
    std::cerr << "[edge-exc] pending exception type=" << static_cast<int>(exception_type)
              << " value=" << exception_text << "\n";
  }
  napi_value preserved_exception_line_value = nullptr;
  napi_value preserved_thrown_at_value = nullptr;
  if (unofficial_napi_take_preserved_error_formatting(
          env,
          out->exception,
          &preserved_exception_line_value,
          &preserved_thrown_at_value) == napi_ok) {
    if (preserved_exception_line_value != nullptr) {
      out->exception_line = NapiValueToUtf8(env, preserved_exception_line_value);
    }
    if (preserved_thrown_at_value != nullptr) {
      out->thrown_at = NapiValueToUtf8(env, preserved_thrown_at_value);
    }
  }

  if (prior_exception != nullptr && !prior_exception_line.empty()) {
    bool same_exception = false;
    if (napi_strict_equals(env, out->exception, prior_exception, &same_exception) == napi_ok &&
        same_exception) {
      if (out->exception_line.empty()) {
        out->exception_line = prior_exception_line;
      }
      if (out->thrown_at.empty()) {
        napi_value thrown_at_value = nullptr;
        if (unofficial_napi_get_error_thrown_at(env, out->exception, &thrown_at_value) == napi_ok &&
            thrown_at_value != nullptr) {
          out->thrown_at = NapiValueToUtf8(env, thrown_at_value);
        }
      }
      return true;
    }
  }

  if (out->exception_line.empty()) {
    out->exception_line = GetArrowMessageFromError(env, out->exception);
  }
  if (out->exception_line.empty()) {
    out->exception_line = GetErrorSourceLine(env, out->exception);
  }
  if (out->thrown_at.empty()) {
    napi_value thrown_at_value = nullptr;
    if (unofficial_napi_get_error_thrown_at(env, out->exception, &thrown_at_value) == napi_ok &&
        thrown_at_value != nullptr) {
      out->thrown_at = NapiValueToUtf8(env, thrown_at_value);
    }
  }
  return true;
}

std::string GetExceptionLineForStderr(napi_env env, napi_value exception) {
  const std::string internal_assert_line = GetInternalAssertExceptionLineForStderr(env, exception);
  if (!internal_assert_line.empty()) {
    return internal_assert_line;
  }

  const std::string assert_throws_line = GetAssertThrowsExceptionLineForStderr(env, exception);
  if (!assert_throws_line.empty()) {
    return assert_throws_line;
  }
  return GetErrorSourceLine(env, exception, true);
}

void StripBuiltinModuleCompileFrame(std::string* message) {
  if (message == nullptr || message->empty()) return;
  static constexpr const char kFramePrefix[] =
      "\n    at BuiltinModule.compileForInternalLoader (node:internal/bootstrap/realm:";
  size_t pos = message->find(kFramePrefix);
  while (pos != std::string::npos) {
    const size_t end = message->find('\n', pos + 1);
    message->erase(pos, end == std::string::npos ? std::string::npos : end - pos);
    pos = message->find(kFramePrefix);
  }
}

void NormalizeAssertionPropertyBlock(std::string* message) {
  if (message == nullptr || message->empty()) return;
  size_t block_pos = message->find("\n  generatedMessage:");
  if (block_pos == std::string::npos) {
    block_pos = message->find("\n  code:");
  }
  if (block_pos == std::string::npos || block_pos == 0) return;

  const size_t line_start = message->rfind('\n', block_pos - 1);
  if (line_start == std::string::npos) return;
  if (block_pos >= 2 && message->compare(block_pos - 2, 2, " {") == 0) {
    return;
  }
  message->insert(block_pos, " {");
}

void StripLeadingExceptionLine(std::string* message) {
  if (message == nullptr || message->empty()) return;
  size_t first_newline = message->find('\n');
  if (first_newline == std::string::npos) return;
  size_t second_newline = message->find('\n', first_newline + 1);
  if (second_newline == std::string::npos) return;
  size_t third_newline = message->find('\n', second_newline + 1);
  if (third_newline == std::string::npos) return;
  message->erase(0, third_newline + 1);
}

std::string FormatUncaughtExceptionForStderr(napi_env env,
                                             napi_value exception,
                                             const std::string& stack_message,
                                             const std::string& pending_exception_line = {},
                                             const std::string& pending_thrown_at = {}) {
  if (stack_message.empty()) return stack_message;

  std::string formatted = stack_message;
  const std::string code = GetNamedStringProperty(env, exception, "code");
  const std::string operator_name = GetNamedStringProperty(env, exception, "operator");
  const bool prefer_derived_exception_line =
      code == "ERR_INTERNAL_ASSERTION" || (code == "ERR_ASSERTION" && operator_name == "throws");
  const std::string derived_exception_line = GetExceptionLineForStderr(env, exception);
  if (prefer_derived_exception_line &&
      (formatted.rfind("node:internal/assert:", 0) == 0 || formatted.rfind("node:assert:", 0) == 0)) {
    StripLeadingExceptionLine(&formatted);
  }
  const std::string exception_line = prefer_derived_exception_line
                                         ? derived_exception_line
                                         : (pending_exception_line.empty() ? derived_exception_line
                                                                           : pending_exception_line);
  if (!exception_line.empty() && formatted.rfind(exception_line, 0) != 0) {
    std::string prefix = exception_line;
    if (!prefix.empty() && prefix.back() == '\n') {
      prefix.push_back('\n');
    }
    formatted = prefix + formatted;
  }
  StripBuiltinModuleCompileFrame(&formatted);
  NormalizeAssertionPropertyBlock(&formatted);
  if (EdgeExecArgvHasFlag("--trace-uncaught")) {
    std::string thrown_at = pending_thrown_at;
    if (thrown_at.empty()) {
      napi_value thrown_at_value = nullptr;
      if (unofficial_napi_get_error_thrown_at(env, exception, &thrown_at_value) == napi_ok &&
          thrown_at_value != nullptr) {
        thrown_at = NapiValueToUtf8(env, thrown_at_value);
      }
    }
    if (!thrown_at.empty()) {
      if (!formatted.empty()) {
        formatted += "\n";
      }
      formatted += thrown_at;
    }
  }
  const std::string version = GetProcessVersion(env);
  if (!version.empty()) {
    formatted += "\n\nNode.js " + version;
  }
  return formatted;
}

std::string StatusToString(napi_status status) {
  switch (status) {
    case napi_ok:
      return "napi_ok";
    case napi_invalid_arg:
      return "napi_invalid_arg";
    case napi_object_expected:
      return "napi_object_expected";
    case napi_string_expected:
      return "napi_string_expected";
    case napi_name_expected:
      return "napi_name_expected";
    case napi_function_expected:
      return "napi_function_expected";
    case napi_number_expected:
      return "napi_number_expected";
    case napi_boolean_expected:
      return "napi_boolean_expected";
    case napi_array_expected:
      return "napi_array_expected";
    case napi_generic_failure:
      return "napi_generic_failure";
    case napi_pending_exception:
      return "napi_pending_exception";
    case napi_cancelled:
      return "napi_cancelled";
    case napi_escape_called_twice:
      return "napi_escape_called_twice";
    case napi_handle_scope_mismatch:
      return "napi_handle_scope_mismatch";
    case napi_callback_scope_mismatch:
      return "napi_callback_scope_mismatch";
    case napi_queue_full:
      return "napi_queue_full";
    case napi_closing:
      return "napi_closing";
    case napi_bigint_expected:
      return "napi_bigint_expected";
    case napi_date_expected:
      return "napi_date_expected";
    case napi_arraybuffer_expected:
      return "napi_arraybuffer_expected";
    case napi_detachable_arraybuffer_expected:
      return "napi_detachable_arraybuffer_expected";
    case napi_would_deadlock:
      return "napi_would_deadlock";
    case napi_no_external_buffers_allowed:
      return "napi_no_external_buffers_allowed";
    case napi_cannot_run_js:
      return "napi_cannot_run_js";
    default:
      return "napi_unknown_error";
  }
}

std::string GetAndClearPendingException(napi_env env, bool* is_process_exit, int* process_exit_code) {
  if (is_process_exit != nullptr) *is_process_exit = false;
  if (process_exit_code != nullptr) *process_exit_code = 1;
  PendingExceptionInfo pending = {};
  if (!TakePendingExceptionInfo(env, &pending) || !pending.has_exception || pending.exception == nullptr) {
    return "";
  }

  const std::string enhanced_exception = EdgeFormatFatalExceptionAfterInspector(env, pending.exception);
  if (!enhanced_exception.empty()) {
    return FormatUncaughtExceptionForStderr(
        env, pending.exception, enhanced_exception, pending.exception_line, pending.thrown_at);
  }

  napi_value stack_value = nullptr;
  if (napi_get_named_property(env, pending.exception, "stack", &stack_value) == napi_ok &&
      stack_value != nullptr) {
    napi_value stack_string = nullptr;
    if (napi_coerce_to_string(env, stack_value, &stack_string) == napi_ok && stack_string != nullptr) {
      size_t stack_len = 0;
      if (napi_get_value_string_utf8(env, stack_string, nullptr, 0, &stack_len) == napi_ok && stack_len > 0) {
        std::vector<char> stack_buf(stack_len + 1, '\0');
        size_t copied = 0;
        if (napi_get_value_string_utf8(env, stack_string, stack_buf.data(), stack_buf.size(), &copied) == napi_ok) {
          return FormatUncaughtExceptionForStderr(
              env,
              pending.exception,
              std::string(stack_buf.data(), copied),
              pending.exception_line,
              pending.thrown_at);
        }
      }
    }
  }

  napi_value exception_string = nullptr;
  if (napi_coerce_to_string(env, pending.exception, &exception_string) != napi_ok ||
      exception_string == nullptr) {
    return "";
  }

  size_t length = 0;
  if (napi_get_value_string_utf8(env, exception_string, nullptr, 0, &length) != napi_ok) {
    return "";
  }

  std::vector<char> buffer(length + 1, '\0');
  size_t copied = 0;
  if (napi_get_value_string_utf8(env, exception_string, buffer.data(), buffer.size(), &copied) != napi_ok) {
    return "";
  }
  return FormatUncaughtExceptionForStderr(
      env,
      pending.exception,
      std::string(buffer.data(), copied),
      pending.exception_line,
      pending.thrown_at);
}

int GetProcessExitCode(napi_env env, bool* has_exit_code) {
  if (has_exit_code != nullptr) *has_exit_code = false;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return 0;
  }
  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process) {
    return 0;
  }
  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return 0;
  }
  bool has_exit_code_prop = false;
  if (napi_has_named_property(env, process_obj, "exitCode", &has_exit_code_prop) != napi_ok ||
      !has_exit_code_prop) {
    return 0;
  }
  napi_value exit_code_value = nullptr;
  if (napi_get_named_property(env, process_obj, "exitCode", &exit_code_value) != napi_ok ||
      exit_code_value == nullptr) {
    return 0;
  }
  napi_valuetype value_type = napi_undefined;
  if (napi_typeof(env, exit_code_value, &value_type) != napi_ok || value_type == napi_undefined ||
      value_type == napi_null) {
    return 0;
  }
  int32_t exit_code = 0;
  if (napi_get_value_int32(env, exit_code_value, &exit_code) != napi_ok) {
    return 0;
  }
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_exit_code(static_cast<int>(exit_code));
  }
  if (has_exit_code != nullptr) *has_exit_code = true;
  return static_cast<int>(exit_code);
}

int GetProcessExitCodeOrZero(napi_env env) {
  bool has_exit_code = false;
  const int exit_code = GetProcessExitCode(env, &has_exit_code);
  return has_exit_code ? exit_code : 0;
}

bool IsEnvironmentExitRequested(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->exiting();
  }
  return false;
}

int GetEnvironmentExitCodeOrFallback(napi_env env, int fallback) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr &&
      environment->has_exit_code()) {
    return environment->exit_code(fallback);
  }
  return fallback;
}

bool IsProcessExiting(napi_env env) {
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    return environment->exiting();
  }
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return false;
  }
  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process) {
    return false;
  }
  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return false;
  }
  bool has_exiting = false;
  if (napi_has_named_property(env, process_obj, "_exiting", &has_exiting) != napi_ok || !has_exiting) {
    return false;
  }
  napi_value exiting_value = nullptr;
  if (napi_get_named_property(env, process_obj, "_exiting", &exiting_value) != napi_ok ||
      exiting_value == nullptr) {
    return false;
  }
  bool exiting = false;
  return napi_get_value_bool(env, exiting_value, &exiting) == napi_ok && exiting;
}

bool ShouldSuppressExceptionsForExit(napi_env env) {
  if (env == nullptr) return false;
  if (IsProcessExiting(env)) return true;
  if (IsEnvironmentExitRequested(env)) return true;
  return !EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env);
}

bool SetProcessExitCodeIfNeeded(napi_env env, int exit_code, bool only_if_unset) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return false;
  }
  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process) {
    return false;
  }
  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return false;
  }
  if (only_if_unset) {
    bool has_exit_code = false;
    if (napi_has_named_property(env, process_obj, "exitCode", &has_exit_code) == napi_ok && has_exit_code) {
      napi_value existing = nullptr;
      napi_valuetype type = napi_undefined;
      if (napi_get_named_property(env, process_obj, "exitCode", &existing) == napi_ok &&
          existing != nullptr &&
          napi_typeof(env, existing, &type) == napi_ok &&
          type != napi_undefined &&
          type != napi_null) {
        int32_t existing_exit_code = 0;
        if (napi_get_value_int32(env, existing, &existing_exit_code) == napi_ok) {
          if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
            environment->set_exit_code(static_cast<int>(existing_exit_code));
          }
        }
        return true;
      }
    }
  }

  napi_value exit_code_value = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(exit_code), &exit_code_value) != napi_ok ||
      exit_code_value == nullptr) {
    return false;
  }
  if (napi_set_named_property(env, process_obj, "exitCode", exit_code_value) != napi_ok) {
    return false;
  }
  if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
    environment->set_exit_code(exit_code);
  }
  return true;
}

bool EmitProcessLifecycleEvent(napi_env env, const char* event_name, int exit_code, bool skip_task_queues = false) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return false;
  }
  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process) {
    return false;
  }
  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return false;
  }
  if (std::strcmp(event_name, "exit") == 0) {
    if (auto* environment = EdgeEnvironmentGet(env); environment != nullptr) {
      environment->set_exiting(true);
      environment->set_exit_code(exit_code);
    }
    napi_value exiting_value = nullptr;
    if (napi_get_boolean(env, true, &exiting_value) == napi_ok && exiting_value != nullptr) {
      (void)napi_set_named_property(env, process_obj, "_exiting", exiting_value);
    }
  }
  bool has_emit = false;
  if (napi_has_named_property(env, process_obj, "emit", &has_emit) != napi_ok || !has_emit) {
    return false;
  }
  napi_value emit_fn = nullptr;
  if (napi_get_named_property(env, process_obj, "emit", &emit_fn) != napi_ok || emit_fn == nullptr) {
    return false;
  }
  napi_valuetype emit_type = napi_undefined;
  if (napi_typeof(env, emit_fn, &emit_type) != napi_ok || emit_type != napi_function) {
    return false;
  }
  napi_value event_name_value = nullptr;
  napi_value exit_code_value = nullptr;
  if (napi_create_string_utf8(env, event_name, NAPI_AUTO_LENGTH, &event_name_value) != napi_ok ||
      event_name_value == nullptr ||
      napi_create_int32(env, static_cast<int32_t>(exit_code), &exit_code_value) != napi_ok ||
      exit_code_value == nullptr) {
    return false;
  }
  napi_value args[2] = {event_name_value, exit_code_value};
  napi_value ignored = nullptr;
  const int callback_flags = skip_task_queues ? kEdgeMakeCallbackSkipTaskQueues : kEdgeMakeCallbackNone;
  return EdgeMakeCallbackWithFlags(env, process_obj, emit_fn, 2, args, &ignored, callback_flags) == napi_ok;
}

int EmitProcessExitOnFatalException(napi_env env, int default_exit_code) {
  (void)SetProcessExitCodeIfNeeded(env, default_exit_code, true);
  if (!IsProcessExiting(env)) {
    const int exit_code_before_emit = GetProcessExitCodeOrZero(env);
    (void)EmitProcessLifecycleEvent(env, "exit", exit_code_before_emit, true);
  }
  bool has_exit_code = false;
  const int final_exit_code = GetProcessExitCode(env, &has_exit_code);
  return has_exit_code ? final_exit_code : default_exit_code;
}

bool DomainHasErrorHandler(napi_env env, napi_value domain) {
  if (env == nullptr || domain == nullptr) return false;
  napi_valuetype domain_type = napi_undefined;
  if (napi_typeof(env, domain, &domain_type) != napi_ok ||
      (domain_type != napi_object && domain_type != napi_function)) {
    return false;
  }

  napi_value listener_count_fn = nullptr;
  napi_valuetype listener_count_type = napi_undefined;
  if (napi_get_named_property(env, domain, "listenerCount", &listener_count_fn) != napi_ok ||
      listener_count_fn == nullptr ||
      napi_typeof(env, listener_count_fn, &listener_count_type) != napi_ok ||
      listener_count_type != napi_function) {
    return false;
  }

  napi_value error_name = nullptr;
  if (napi_create_string_utf8(env, "error", NAPI_AUTO_LENGTH, &error_name) != napi_ok ||
      error_name == nullptr) {
    return false;
  }

  napi_value count_value = nullptr;
  napi_value argv[1] = {error_name};
  if (napi_call_function(env, domain, listener_count_fn, 1, argv, &count_value) != napi_ok ||
      count_value == nullptr) {
    bool has_pending = false;
    if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
      napi_value ignored = nullptr;
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }
    return false;
  }

  uint32_t count = 0;
  return napi_get_value_uint32(env, count_value, &count) == napi_ok && count > 0;
}

bool HasActiveDomainErrorHandler(napi_env env) {
  if (env == nullptr) return false;

  napi_value global = nullptr;
  napi_value process_obj = nullptr;
  napi_value current_domain = nullptr;
  if (napi_get_global(env, &global) == napi_ok &&
      global != nullptr &&
      napi_get_named_property(env, global, "process", &process_obj) == napi_ok &&
      process_obj != nullptr &&
      napi_get_named_property(env, process_obj, "domain", &current_domain) == napi_ok &&
      current_domain != nullptr &&
      !IsNullOrUndefinedValue(env, current_domain) &&
      DomainHasErrorHandler(env, current_domain)) {
    return true;
  }

  napi_value require_fn = EdgeGetRequireFunction(env);
  napi_valuetype require_type = napi_undefined;
  if (require_fn == nullptr ||
      napi_typeof(env, require_fn, &require_type) != napi_ok ||
      require_type != napi_function) {
    return false;
  }

  napi_value domain_name = nullptr;
  if (napi_create_string_utf8(env, "domain", NAPI_AUTO_LENGTH, &domain_name) != napi_ok ||
      domain_name == nullptr) {
    return false;
  }

  napi_value domain_module = nullptr;
  napi_value require_argv[1] = {domain_name};
  if (napi_call_function(env, global, require_fn, 1, require_argv, &domain_module) != napi_ok ||
      domain_module == nullptr) {
    bool has_pending = false;
    if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
      napi_value ignored = nullptr;
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }
    return false;
  }

  napi_value stack = nullptr;
  bool is_array = false;
  if (napi_get_named_property(env, domain_module, "_stack", &stack) != napi_ok ||
      stack == nullptr ||
      napi_is_array(env, stack, &is_array) != napi_ok ||
      !is_array) {
    return false;
  }

  uint32_t length = 0;
  if (napi_get_array_length(env, stack, &length) != napi_ok) return false;
  for (uint32_t i = 0; i < length; ++i) {
    napi_value domain = nullptr;
    if (napi_get_element(env, stack, i, &domain) == napi_ok &&
        domain != nullptr &&
        !IsNullOrUndefinedValue(env, domain) &&
        DomainHasErrorHandler(env, domain)) {
      return true;
    }
  }

  return false;
}

bool ShouldAbortOnUncaughtException() {
  return EdgeExecArgvHasFlag("--abort-on-uncaught-exception") ||
         EdgeExecArgvHasFlag("--abort_on_uncaught_exception");
}

std::string FormatFatalExceptionMessage(napi_env env,
                                        napi_value exception,
                                        const std::string& pending_exception_line = {},
                                        const std::string& pending_thrown_at = {}) {
  if (env == nullptr || exception == nullptr) return "Unhandled async exception";

  std::string exception_message;
  const std::string enhanced_exception =
      EdgeFormatFatalExceptionAfterInspector(env, exception);
  if (!enhanced_exception.empty()) {
    exception_message = FormatUncaughtExceptionForStderr(
        env, exception, enhanced_exception, pending_exception_line, pending_thrown_at);
  }

  napi_value stack_value = nullptr;
  if (exception_message.empty() &&
      napi_get_named_property(env, exception, "stack", &stack_value) == napi_ok &&
      stack_value != nullptr) {
    napi_value stack_string = nullptr;
    if (napi_coerce_to_string(env, stack_value, &stack_string) == napi_ok &&
        stack_string != nullptr) {
      size_t stack_len = 0;
      if (napi_get_value_string_utf8(env, stack_string, nullptr, 0, &stack_len) == napi_ok &&
          stack_len > 0) {
        std::vector<char> stack_buf(stack_len + 1, '\0');
        size_t copied = 0;
        if (napi_get_value_string_utf8(
                env, stack_string, stack_buf.data(), stack_buf.size(), &copied) == napi_ok) {
          exception_message = FormatUncaughtExceptionForStderr(
              env,
              exception,
              std::string(stack_buf.data(), copied),
              pending_exception_line,
              pending_thrown_at);
        }
      }
    }
  }

  if (exception_message.empty()) {
    napi_value exception_string = nullptr;
    if (napi_coerce_to_string(env, exception, &exception_string) == napi_ok &&
        exception_string != nullptr) {
      exception_message = FormatUncaughtExceptionForStderr(
          env,
          exception,
          NapiValueToUtf8(env, exception_string),
          pending_exception_line,
          pending_thrown_at);
    }
  }

  return exception_message.empty() ? "Unhandled async exception" : exception_message;
}

[[noreturn]] void AbortOnUncaughtException(napi_env env,
                                          napi_value exception,
                                          const std::string& pending_exception_line = {},
                                          const std::string& pending_thrown_at = {}) {
  std::string exception_message =
      FormatFatalExceptionMessage(env, exception, pending_exception_line, pending_thrown_at);
  if (!exception_message.empty()) {
    if (exception_message.back() != '\n') exception_message.push_back('\n');
    WriteTextToFd(2, exception_message);
  }
  std::abort();
}

int FinalizeFatalException(napi_env env,
                           napi_value exception,
                           std::string* error_out,
                           int default_exit_code = 1,
                           const std::string& exception_line = {},
                           const std::string& thrown_at = {}) {
  if (exception == nullptr) {
    if (error_out != nullptr) {
      *error_out = "Unhandled async exception";
    }
    return default_exit_code;
  }

  (void)EdgeWriteReportForUncaughtException(env, exception);
  const int exit_code = EmitProcessExitOnFatalException(env, default_exit_code);
  const std::string exception_message =
      FormatFatalExceptionMessage(env, exception, exception_line, thrown_at);
  if (error_out != nullptr) {
    *error_out = exception_message.empty() ? "Unhandled async exception" : exception_message;
  }
  return exit_code;
}

bool DispatchUncaughtException(napi_env env,
                               napi_value exception,
                               bool* handled_out,
                               napi_value* effective_exception_out = nullptr,
                               int* fatal_exit_code_out = nullptr,
                               std::string* effective_exception_line_out = nullptr) {
  const std::string prior_exception_line =
      effective_exception_line_out != nullptr ? *effective_exception_line_out : "";
  if (handled_out != nullptr) *handled_out = false;
  if (effective_exception_out != nullptr) *effective_exception_out = exception;
  if (fatal_exit_code_out != nullptr) *fatal_exit_code_out = -1;
  if (effective_exception_line_out != nullptr) *effective_exception_line_out = prior_exception_line;
  if (exception == nullptr) return false;

  if (ShouldSuppressExceptionsForExit(env)) {
    bool has_pending = false;
    if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
      napi_value ignored = nullptr;
      (void)napi_get_and_clear_last_exception(env, &ignored);
    }
    (void)unofficial_napi_cancel_terminate_execution(env);
    if (handled_out != nullptr) *handled_out = true;
    return true;
  }

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;
  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return false;
  }

  napi_value fatal_exception_fn = nullptr;
  if (napi_get_named_property(env, process_obj, "_fatalException", &fatal_exception_fn) != napi_ok ||
      fatal_exception_fn == nullptr) {
    return false;
  }
  napi_valuetype fatal_type = napi_undefined;
  if (napi_typeof(env, fatal_exception_fn, &fatal_type) != napi_ok || fatal_type != napi_function) {
    if (fatal_exit_code_out != nullptr) {
      *fatal_exit_code_out = kExitCodeInvalidFatalExceptionMonkeyPatching;
    }
    return false;
  }

  napi_value from_promise = nullptr;
  if (napi_get_boolean(env, false, &from_promise) != napi_ok || from_promise == nullptr) {
    return false;
  }

  napi_value argv[2] = {exception, from_promise};
  napi_value handled_value = nullptr;
  const napi_status status = napi_call_function(env, process_obj, fatal_exception_fn, 2, argv, &handled_value);
  if (status != napi_ok) {
    if (ShouldSuppressExceptionsForExit(env)) {
      bool has_pending = false;
      if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
        napi_value ignored = nullptr;
        (void)napi_get_and_clear_last_exception(env, &ignored);
      }
      if (handled_out != nullptr) *handled_out = true;
      return true;
    }

    PendingExceptionInfo pending = {};
    if (TakePendingExceptionInfo(env, &pending, exception, prior_exception_line) &&
        pending.has_exception &&
        pending.exception != nullptr) {
      if (effective_exception_out != nullptr) {
        *effective_exception_out = pending.exception;
      }
      if (effective_exception_line_out != nullptr) {
        *effective_exception_line_out = pending.exception_line;
      }
    }
    if (DebugExceptionsEnabled()) {
      std::cerr << "[edge-exc] process._fatalException threw\n";
    }
    if (fatal_exit_code_out != nullptr) {
      *fatal_exit_code_out = kExitCodeExceptionInFatalExceptionHandler;
    }
    if (handled_out != nullptr) *handled_out = false;
    return true;
  }

  bool handled = true;
  napi_valuetype handled_type = napi_undefined;
  if (handled_value != nullptr && napi_typeof(env, handled_value, &handled_type) == napi_ok &&
      handled_type == napi_boolean) {
    bool bool_value = true;
    if (napi_get_value_bool(env, handled_value, &bool_value) == napi_ok && !bool_value) {
      handled = false;
    }
  }

  if (DebugExceptionsEnabled()) {
    std::cerr << "[edge-exc] process._fatalException handled=" << (handled ? "true" : "false") << "\n";
  }
  if (handled_out != nullptr) *handled_out = handled;
  return true;
}

int HandleExtractedException(napi_env env,
                             napi_value exception,
                             std::string* error_out,
                             const std::string& pending_exception_line = {},
                             const std::string& pending_thrown_at = {}) {
  const bool abort_on_uncaught = ShouldAbortOnUncaughtException();
  if (abort_on_uncaught && !HasActiveDomainErrorHandler(env)) {
    AbortOnUncaughtException(env, exception, pending_exception_line, pending_thrown_at);
  }

  bool handled = false;
  napi_value effective_exception = exception;
  std::string effective_exception_line = pending_exception_line;
  const std::string effective_exception_thrown_at = pending_thrown_at;
  int fatal_exit_code = -1;
  (void)DispatchUncaughtException(
      env,
      exception,
      &handled,
      &effective_exception,
      &fatal_exit_code,
      &effective_exception_line);
  if (handled) {
    if (DebugExceptionsEnabled()) {
      std::cerr << "[edge-exc] handled async exception, continue loop\n";
    }
    return -1;
  }

  if (abort_on_uncaught) {
    AbortOnUncaughtException(
        env, effective_exception, effective_exception_line, effective_exception_thrown_at);
  }

  return FinalizeFatalException(
      env,
      effective_exception,
      error_out,
      fatal_exit_code >= 0 ? fatal_exit_code : 1,
      effective_exception_line,
      effective_exception_thrown_at);
}

int HandlePendingExceptionAfterLoopStep(napi_env env, std::string* error_out) {
  PendingExceptionInfo pending = {};
  if (!TakePendingExceptionInfo(env, &pending) || !pending.has_exception) {
    return -1;
  }

  if (ShouldSuppressExceptionsForExit(env)) {
    (void)unofficial_napi_cancel_terminate_execution(env);
    return -1;
  }

  if (pending.exception == nullptr) {
    if (error_out != nullptr) *error_out = "Unhandled async exception";
    return 1;
  }

  return HandleExtractedException(
      env, pending.exception, error_out, pending.exception_line, pending.thrown_at);
}

// Mirrors Node's native tick dispatch by preferring the task_queue callback
// registered through setTickCallback(), and falling back to process._tickCallback.
napi_status DrainProcessTickCallback(napi_env env) {
  edge::HandleScope scope(env);

  bool called_task_queue_tick = false;
  const napi_status task_queue_status = EdgeRunTaskQueueTickCallback(env, &called_task_queue_tick);
  if (task_queue_status != napi_ok) {
    return task_queue_status;
  }
  if (called_task_queue_tick) {
    return napi_ok;
  }

  napi_value global = nullptr;
  napi_value process = nullptr;
  napi_value tick_cb = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return napi_ok;
  if (napi_get_named_property(env, global, "process", &process) != napi_ok || process == nullptr) return napi_ok;
  if (napi_get_named_property(env, process, "_tickCallback", &tick_cb) != napi_ok || tick_cb == nullptr) {
    return napi_ok;
  }
  napi_valuetype type = napi_undefined;
  napi_typeof(env, tick_cb, &type);
  if (type != napi_function) return napi_ok;
  napi_value ignored = nullptr;
  return napi_call_function(env, process, tick_cb, 0, nullptr, &ignored);
}

bool IsPromisePending(napi_env env, napi_value promise) {
  if (env == nullptr || promise == nullptr) return false;
  int32_t state = 0;
  napi_value result = nullptr;
  bool has_result = false;
  if (unofficial_napi_get_promise_details(env, promise, &state, &result, &has_result) != napi_ok) {
    return false;
  }
  return state == 0;
}

bool IsFunctionValue(napi_env env, napi_value value) {
  if (env == nullptr || value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

napi_value GetRuntimeInternalBinding(napi_env env, napi_value global) {
  napi_value internal_binding = EdgeGetInternalBinding(env);
  if (IsFunctionValue(env, internal_binding)) {
    return internal_binding;
  }
  if (global == nullptr && (napi_get_global(env, &global) != napi_ok || global == nullptr)) {
    return nullptr;
  }
  if (napi_get_named_property(env, global, "internalBinding", &internal_binding) != napi_ok ||
      !IsFunctionValue(env, internal_binding)) {
    return nullptr;
  }
  return internal_binding;
}

napi_value GetEntryPointPromiseFromUtilSymbol(napi_env env, napi_value global) {
  if (env == nullptr) return nullptr;
  if (global == nullptr && (napi_get_global(env, &global) != napi_ok || global == nullptr)) {
    return nullptr;
  }

  napi_value util_binding = edge::binding_registry::Get(env, "util");
  if (util_binding == nullptr) return nullptr;

  napi_value private_symbols = nullptr;
  if (napi_get_named_property(env, util_binding, "privateSymbols", &private_symbols) != napi_ok ||
      private_symbols == nullptr) {
    return nullptr;
  }

  napi_value entry_point_symbol = nullptr;
  if (napi_get_named_property(env,
                              private_symbols,
                              "entry_point_promise_private_symbol",
                              &entry_point_symbol) != napi_ok ||
      entry_point_symbol == nullptr) {
    return nullptr;
  }

  napi_value promise = nullptr;
  if (napi_get_property(env, global, entry_point_symbol, &promise) != napi_ok || promise == nullptr) {
    return nullptr;
  }
  return promise;
}

napi_value GetEntryPointModuleFromUtilSymbol(napi_env env, napi_value global) {
  if (env == nullptr) return nullptr;
  if (global == nullptr && (napi_get_global(env, &global) != napi_ok || global == nullptr)) {
    return nullptr;
  }

  napi_value internal_binding = GetRuntimeInternalBinding(env, global);
  if (!IsFunctionValue(env, internal_binding)) return nullptr;

  napi_value util_name = nullptr;
  if (napi_create_string_utf8(env, "util", NAPI_AUTO_LENGTH, &util_name) != napi_ok || util_name == nullptr) {
    return nullptr;
  }
  napi_value util_binding = nullptr;
  napi_value argv[1] = {util_name};
  if (napi_call_function(env, global, internal_binding, 1, argv, &util_binding) != napi_ok || util_binding == nullptr) {
    return nullptr;
  }

  napi_value private_symbols = nullptr;
  if (napi_get_named_property(env, util_binding, "privateSymbols", &private_symbols) != napi_ok ||
      private_symbols == nullptr) {
    return nullptr;
  }

  napi_value entry_point_symbol = nullptr;
  if (napi_get_named_property(env,
                              private_symbols,
                              "entry_point_module_private_symbol",
                              &entry_point_symbol) != napi_ok ||
      entry_point_symbol == nullptr) {
    return nullptr;
  }

  napi_value module = nullptr;
  if (napi_get_property(env, global, entry_point_symbol, &module) != napi_ok || module == nullptr) {
    return nullptr;
  }
  return module;
}

napi_value GetEntryPointPromiseBySymbolScan(napi_env env, napi_value global) {
  if (env == nullptr) return nullptr;
  if (global == nullptr && (napi_get_global(env, &global) != napi_ok || global == nullptr)) {
    return nullptr;
  }

  napi_value keys = nullptr;
  if (napi_get_all_property_names(env,
                                  global,
                                  napi_key_own_only,
                                  napi_key_skip_strings,
                                  napi_key_keep_numbers,
                                  &keys) != napi_ok ||
      keys == nullptr) {
    return nullptr;
  }
  bool is_array = false;
  if (napi_is_array(env, keys, &is_array) != napi_ok || !is_array) return nullptr;

  uint32_t length = 0;
  if (napi_get_array_length(env, keys, &length) != napi_ok) return nullptr;

  for (uint32_t i = 0; i < length; ++i) {
    napi_value key = nullptr;
    if (napi_get_element(env, keys, i, &key) != napi_ok || key == nullptr) continue;

    napi_value key_str = nullptr;
    if (napi_coerce_to_string(env, key, &key_str) != napi_ok || key_str == nullptr) continue;
    size_t len = 0;
    if (napi_get_value_string_utf8(env, key_str, nullptr, 0, &len) != napi_ok || len == 0) continue;
    std::string text(len + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, key_str, text.data(), text.size(), &copied) != napi_ok) continue;
    text.resize(copied);
    if (text.find("entry_point_promise_private_symbol") == std::string::npos) continue;

    napi_value promise = nullptr;
    if (napi_get_property(env, global, key, &promise) != napi_ok || promise == nullptr) continue;
    return promise;
  }

  return nullptr;
}

napi_value GetEntryPointPromiseValue(napi_env env) {
  if (env == nullptr) return nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return nullptr;

  napi_value promise = GetEntryPointPromiseFromUtilSymbol(env, global);
  if (promise != nullptr) return promise;

  return GetEntryPointPromiseBySymbolScan(env, global);
}

bool HasPendingEntryPointPromise(napi_env env) {
  if (env == nullptr) return false;
  napi_value promise = GetEntryPointPromiseValue(env);
  if (promise == nullptr) return false;
  return IsPromisePending(env, promise);
}

bool AreProcessWarningsEnabled() {
  const char* value = std::getenv("NODE_NO_WARNINGS");
  if (value != nullptr && value[0] != '\0' && std::strcmp(value, "0") != 0) {
    return false;
  }

  const std::vector<std::string>& exec_argv =
      !g_edge_effective_exec_argv.empty() ? g_edge_effective_exec_argv : g_edge_cli_exec_argv;
  bool warnings = true;
  for (const auto& token : exec_argv) {
    if (token == "--no-warnings") {
      warnings = false;
      continue;
    }
    if (token == "--warnings") {
      warnings = true;
      continue;
    }
    if (token == "--warnings=false" || token == "--warnings=0") {
      warnings = false;
      continue;
    }
    if (token == "--warnings=true" || token == "--warnings=1") {
      warnings = true;
      continue;
    }
  }
  return warnings;
}

bool ApplyUnsettledTopLevelAwaitExitCodeIfNeeded(napi_env env) {
  if (env == nullptr) return false;

  bool has_exit_code = false;
  (void)GetProcessExitCode(env, &has_exit_code);
  if (has_exit_code) return false;

  napi_value promise = GetEntryPointPromiseValue(env);
  if (!IsPromisePending(env, promise)) return false;

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;
  napi_value module = GetEntryPointModuleFromUtilSymbol(env, global);
  if (module == nullptr) return false;

  bool settled = true;
  if (unofficial_napi_module_wrap_check_unsettled_top_level_await(
          env, module, AreProcessWarningsEnabled(), &settled) != napi_ok) {
    return false;
  }
  if (settled) return false;

  return SetProcessExitCodeIfNeeded(env, kExitCodeUnsettledTopLevelAwait, true);
}

int WaitForTopLevelPromiseToSettle(napi_env env, napi_value value, std::string* error_out) {
  if (!IsPromisePending(env, value)) return -1;

  uv_loop_t* loop = EdgeGetEnvLoop(env);
  while (IsPromisePending(env, value)) {
    if (loop != nullptr) {
      (void)uv_run(loop, UV_RUN_NOWAIT);
    }
    if (IsEnvironmentExitRequested(env)) {
      break;
    }
    if (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env)) {
      break;
    }
    (void)EdgeRuntimePlatformDrainTasks(env);

    const int async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (async_status >= 0) {
      return async_status;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(1));
  }

  int32_t state = 0;
  napi_value settled_result = nullptr;
  bool has_result = false;
  if (unofficial_napi_get_promise_details(env, value, &state, &settled_result, &has_result) == napi_ok &&
      state == 2 &&
      has_result &&
      settled_result != nullptr) {
    if (DebugExceptionsEnabled()) {
      napi_valuetype result_type = napi_undefined;
      std::string result_text;
      if (napi_typeof(env, settled_result, &result_type) == napi_ok) {
        result_text = NapiValueToUtf8(env, settled_result);
      }
      std::cerr << "[edge-esm] entry promise rejected type=" << static_cast<int>(result_type)
                << " value=" << result_text << "\n";
    }
    napi_throw(env, settled_result);
    if (DebugExceptionsEnabled()) {
      bool has_pending = false;
      napi_is_exception_pending(env, &has_pending);
      std::cerr << "[edge-esm] after napi_throw pending=" << (has_pending ? "true" : "false") << "\n";
    }
    const int async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (async_status >= 0) {
      return async_status;
    }
    return 1;
  }

  return -1;
}

int RunEventLoopUntilQuiescent(napi_env env, std::string* error_out) {
  uv_loop_t* loop = EdgeGetEnvLoop(env);
  if (loop == nullptr) {
    if (error_out != nullptr) {
      *error_out = "Missing env libuv loop";
    }
    return 1;
  }
  int64_t loop_timeout_ms = 0;
  if (const char* timeout_env = std::getenv("EDGE_LOOP_TIMEOUT_MS")) {
    char* end = nullptr;
    const long long parsed = std::strtoll(timeout_env, &end, 10);
    if (end != timeout_env && parsed > 0) loop_timeout_ms = parsed;
  }
  const auto loop_start = std::chrono::steady_clock::now();

  struct TimeoutCleanupState {
    int killed_processes = 0;
    int closed_handles = 0;
  };
  auto cleanup_handles_on_timeout = [&](TimeoutCleanupState* state) {
    if (state == nullptr) return;
    uv_walk(
        loop,
        [](uv_handle_t* h, void* arg) {
          if (h == nullptr || arg == nullptr) return;
          auto* st = static_cast<TimeoutCleanupState*>(arg);
          if (uv_handle_get_type(h) == UV_PROCESS) {
            auto* p = reinterpret_cast<uv_process_t*>(h);
            if (p->pid > 0) {
              (void)uv_process_kill(p, SIGKILL);
            }
            st->killed_processes += 1;
          }
          if (!uv_is_closing(h)) {
            uv_close(h, [](uv_handle_t* /*handle*/) {});
            st->closed_handles += 1;
          }
        },
        state);
    // Best effort drain to allow close callbacks/process exits to run.
    for (int i = 0; i < 8; i++) {
      if (uv_run(loop, UV_RUN_NOWAIT) == 0) break;
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
  };

  auto active_handles_summary = [&](std::string* out) {
    if (out == nullptr) return;
    const char* kScript =
        "(function(){"
        "try{"
        "var hs=(process&&typeof process._getActiveHandles==='function')?process._getActiveHandles():[];"
        "var names=[];"
        "for(var i=0;i<hs.length;i++){"
        "var h=hs[i];"
        "names.push((h&&h.constructor&&h.constructor.name)||typeof h);"
        "}"
        "return names.join(',');"
        "}catch(_){return '';}"
        "})()";
    napi_value script = nullptr;
    napi_value result = nullptr;
    if (napi_create_string_utf8(env, kScript, NAPI_AUTO_LENGTH, &script) != napi_ok || script == nullptr) return;
    if (napi_run_script(env, script, &result) != napi_ok || result == nullptr) return;
    size_t len = 0;
    if (napi_get_value_string_utf8(env, result, nullptr, 0, &len) != napi_ok) return;
    std::string tmp(len + 1, '\0');
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, result, tmp.data(), tmp.size(), &copied) != napi_ok) return;
    tmp.resize(copied);
    *out = std::move(tmp);
    if (!out->empty()) return;

    struct WalkState {
      std::map<std::string, int> counts;
      int active = 0;
      int referenced = 0;
      std::vector<int64_t> process_pids;
      std::vector<std::string> tcp_socks;
    } ws;
    uv_walk(
        loop,
        [](uv_handle_t* h, void* arg) {
          if (h == nullptr || arg == nullptr) return;
          auto* st = static_cast<WalkState*>(arg);
          const uv_handle_type t = uv_handle_get_type(h);
          const char* tn = uv_handle_type_name(t);
          const std::string key = (tn != nullptr) ? std::string(tn) : std::string("unknown");
          st->counts[key] += 1;
          st->active += (uv_is_active(h) ? 1 : 0);
          st->referenced += (uv_has_ref(h) ? 1 : 0);
          if (t == UV_PROCESS) {
            auto* p = reinterpret_cast<uv_process_t*>(h);
            st->process_pids.push_back(static_cast<int64_t>(p->pid));
          } else if (t == UV_TCP) {
            auto* tcp = reinterpret_cast<uv_tcp_t*>(h);
            sockaddr_storage ss{};
            int slen = sizeof(ss);
            if (uv_tcp_getsockname(tcp, reinterpret_cast<sockaddr*>(&ss), &slen) == 0) {
              char ip[INET6_ADDRSTRLEN] = {0};
              int port = 0;
              const char* fam = "IPv4";
              if (ss.ss_family == AF_INET6) {
                const auto* a6 = reinterpret_cast<const sockaddr_in6*>(&ss);
                uv_ip6_name(a6, ip, sizeof(ip));
                port = ntohs(a6->sin6_port);
                fam = "IPv6";
              } else if (ss.ss_family == AF_INET) {
                const auto* a4 = reinterpret_cast<const sockaddr_in*>(&ss);
                uv_ip4_name(a4, ip, sizeof(ip));
                port = ntohs(a4->sin_port);
              }
              std::ostringstream tcp_desc;
              tcp_desc << fam << ":" << ip << ":" << port
                       << ":active=" << (uv_is_active(h) ? 1 : 0)
                       << ":ref=" << (uv_has_ref(h) ? 1 : 0);
              st->tcp_socks.push_back(tcp_desc.str());
            }
          }
        },
        &ws);
    if (ws.counts.empty()) return;
    std::ostringstream oss;
    oss << "uv_handles active=" << ws.active << " ref=" << ws.referenced << " [";
    bool first = true;
    for (const auto& kv : ws.counts) {
      if (!first) oss << ", ";
      first = false;
      oss << kv.first << ":" << kv.second;
    }
    if (!ws.process_pids.empty()) {
      oss << "; process_pids=";
      for (size_t i = 0; i < ws.process_pids.size(); i++) {
        if (i > 0) oss << ",";
        oss << ws.process_pids[i];
      }
    }
    if (!ws.tcp_socks.empty()) {
      oss << "; tcp_socks=";
      for (size_t i = 0; i < ws.tcp_socks.size(); i++) {
        if (i > 0) oss << " | ";
        oss << ws.tcp_socks[i];
      }
    }
    oss << "]";
    *out = oss.str();
  };

  int idle_drain_turns = 0;
  while (true) {
    if (IsProcessExiting(env)) {
      break;
    }
    if (IsEnvironmentExitRequested(env)) {
      break;
    }
    if (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env)) {
      break;
    }
    if (loop_timeout_ms > 0) {
      const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
          std::chrono::steady_clock::now() - loop_start)
                                  .count();
      if (elapsed_ms >= loop_timeout_ms) {
        std::string handles;
        active_handles_summary(&handles);
        TimeoutCleanupState cleanup_state;
        cleanup_handles_on_timeout(&cleanup_state);
        if (error_out != nullptr) {
          *error_out = "EDGE loop timeout after " + std::to_string(elapsed_ms) + "ms";
          if (!handles.empty()) *error_out += "; active handles: " + handles;
          if (cleanup_state.killed_processes > 0 || cleanup_state.closed_handles > 0) {
            *error_out += "; timeout cleanup: killed_processes=" +
                          std::to_string(cleanup_state.killed_processes) +
                          ", closed_handles=" + std::to_string(cleanup_state.closed_handles);
          }
        }
        uv_stop(loop);
        return 1;
      }
    }
    const bool use_polling_loop = loop_timeout_ms > 0;
    if (use_polling_loop) {
      uv_run(loop, UV_RUN_NOWAIT);
      if (uv_loop_alive(loop) != 0) {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
    } else {
      uv_run(loop, UV_RUN_DEFAULT);
    }
    if (IsEnvironmentExitRequested(env)) {
      break;
    }
    if (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env)) {
      break;
    }
    // Match Node's embedder loop shape: libuv callbacks and callback scopes
    // own nextTick draining; the loop turn itself only drains platform tasks.
    (void)EdgeRuntimePlatformDrainTasks(env);
    // Some cross-thread V8 async wakeups, notably Atomics.waitAsync() used by
    // worker messaging, can resolve Promises without a JS callback entering the
    // isolate on that turn. Run a plain microtask checkpoint here so those
    // promises settle without waiting for an unrelated timer or I/O callback.
    napi_status microtask_status = napi_ok;
    {
      edge::HandleScope scope(env);
      if (!scope.is_open()) {
        microtask_status = scope.status();
      } else {
        microtask_status = unofficial_napi_process_microtasks(env);
      }
    }
    if (microtask_status != napi_ok) {
      if (error_out != nullptr) {
        *error_out = "Failed to process microtasks";
      }
      return 1;
    }

    int async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (async_status >= 0) {
      return async_status;
    }

    if (IsEnvironmentExitRequested(env)) {
      break;
    }
    if (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env)) {
      break;
    }

    bool more = uv_loop_alive(loop) != 0;
    if (more) {
      idle_drain_turns = 0;
      continue;
    }

    // Give V8 foreground tasks (e.g. FinalizationRegistry cleanup callbacks)
    // a short grace window before declaring the loop quiescent.
    if (idle_drain_turns < 8) {
      idle_drain_turns++;
      (void)EdgeRuntimePlatformDrainTasks(env);
      async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
      if (async_status >= 0) {
        return async_status;
      }
      if (uv_loop_alive(loop) != 0) {
        idle_drain_turns = 0;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
      continue;
    }
    idle_drain_turns = 0;

    const int before_exit_code = GetProcessExitCodeOrZero(env);
    EmitProcessLifecycleEvent(env, "beforeExit", before_exit_code);
    (void)EdgeRuntimePlatformDrainTasks(env);

    async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (async_status >= 0) {
      return async_status;
    }

    more = uv_loop_alive(loop) != 0;
    if (!more) {
      break;
    }
  }

  if (!IsProcessExiting(env) && !IsEnvironmentExitRequested(env)) {
    (void)ApplyUnsettledTopLevelAwaitExitCodeIfNeeded(env);
    EmitProcessLifecycleEvent(env, "exit", GetProcessExitCodeOrZero(env), true);
  }
  const int async_status = HandlePendingExceptionAfterLoopStep(env, error_out);
  if (async_status >= 0) {
    return async_status;
  }

  return -1;
}

std::string EscapeForSingleQuotedJs(const std::string& in) {
  std::string out;
  out.reserve(in.size() + 8);
  for (char ch : in) {
    switch (ch) {
      case '\\':
        out += "\\\\";
        break;
      case '\'':
        out += "\\'";
        break;
      case '\n':
        out += "\\n";
        break;
      case '\r':
        out += "\\r";
        break;
      default:
        out.push_back(ch);
        break;
    }
  }
  return out;
}

std::string ExtractProcessTitleFromExecArgv(const std::vector<std::string>& exec_argv) {
  std::string title;
  for (size_t i = 0; i < exec_argv.size(); ++i) {
    const std::string& token = exec_argv[i];
    static constexpr const char kTitlePrefix[] = "--title=";
    if (token.rfind(kTitlePrefix, 0) == 0) {
      title = token.substr(sizeof(kTitlePrefix) - 1);
      continue;
    }
    if (token == "--title" && i + 1 < exec_argv.size()) {
      title = edge_options::MaybeUnescapeLeadingDashOptionValue(exec_argv[++i]);
    }
  }
  return title;
}

edge_options::EffectiveCliState BuildEffectiveCliStateForCurrentEnv(
    napi_env env,
    const std::vector<std::string>& raw_exec_argv) {
  edge_options::EffectiveCliState state;

  std::vector<std::pair<edge_options::fs::path, bool>> env_specs;
  edge_options::CollectEnvFileSpecs(raw_exec_argv, &env_specs);
  edge_options::ApplyEnvFiles(env_specs, &state.env_updates, &state.warnings, &state.error);
  if (!state.error.empty()) {
    state.ok = false;
    return state;
  }

  std::string node_options_source;
  bool has_env_override = false;
  if (env != nullptr && !EdgeWorkerEnvOwnsProcessState(env) && !EdgeWorkerEnvSharesEnvironment(env)) {
    const std::map<std::string, std::string> entries = EdgeWorkerEnvSnapshotEnvVars(env);
    auto it = entries.find("NODE_OPTIONS");
    if (it != entries.end()) {
      node_options_source = it->second;
      has_env_override = true;
    }
  }

  if (!has_env_override) {
    if (const char* env_node_options = std::getenv("NODE_OPTIONS");
        env_node_options != nullptr) {
      node_options_source = env_node_options;
      has_env_override = true;
    }
  }

  if (!has_env_override) {
    auto it = state.env_updates.find("NODE_OPTIONS");
    if (it != state.env_updates.end()) {
      node_options_source = it->second;
    }
  }

  if (!node_options_source.empty()) {
    state.node_options_tokens =
        edge_options::ParseNodeOptionsString(node_options_source, &state.error);
    if (!state.error.empty()) {
      state.ok = false;
      return state;
    }
  }

  std::vector<edge_options::fs::path> config_paths;
  edge_options::CollectConfigFileSpecs(raw_exec_argv, &config_paths);
  for (const auto& path : config_paths) {
    std::string error;
    std::vector<std::string> flags = edge_options::ParseConfigFileFlags(path, &error);
    if (!error.empty()) {
      state.ok = false;
      state.error = error;
      return state;
    }
    state.config_tokens.insert(state.config_tokens.end(), flags.begin(), flags.end());
  }

  state.effective_tokens.reserve(state.node_options_tokens.size() +
                                 state.config_tokens.size() +
                                 raw_exec_argv.size());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                state.node_options_tokens.begin(),
                                state.node_options_tokens.end());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                state.config_tokens.begin(),
                                state.config_tokens.end());
  state.effective_tokens.insert(state.effective_tokens.end(),
                                raw_exec_argv.begin(),
                                raw_exec_argv.end());
  return state;
}

void ParseNodeStyleFlagsFromSource(napi_env env, const char* source_text) {
  if (env != nullptr && EdgeWorkerEnvOwnsProcessState(env)) {
    g_edge_exec_argv = g_edge_cli_exec_argv;
  }

  const edge_options::EffectiveCliState effective_state =
      BuildEffectiveCliStateForCurrentEnv(env, g_edge_exec_argv);
  g_edge_effective_exec_argv =
      effective_state.ok ? effective_state.effective_tokens : g_edge_exec_argv;

  if (source_text == nullptr) {
    g_edge_process_title = ExtractProcessTitleFromExecArgv(g_edge_effective_exec_argv);
    return;
  }

  std::vector<std::string> source_flag_tokens;
  std::istringstream in{std::string(source_text)};
  std::string line;
  while (std::getline(in, line)) {
    const auto pos = line.find("Flags:");
    if (pos == std::string::npos) continue;
    std::string rest = line.substr(pos + 6);
    std::istringstream tokens(rest);
    std::string token;
    while (tokens >> token) {
      source_flag_tokens.push_back(token);
    }
  }

  g_edge_exec_argv.insert(g_edge_exec_argv.end(), source_flag_tokens.begin(), source_flag_tokens.end());
  g_edge_effective_exec_argv.insert(
      g_edge_effective_exec_argv.end(), source_flag_tokens.begin(), source_flag_tokens.end());
  g_edge_process_title = ExtractProcessTitleFromExecArgv(g_edge_effective_exec_argv);
}

bool IsPowerOfTwo(uint64_t value) {
  return value != 0 && (value & (value - 1)) == 0;
}

bool ParseUint64(std::string_view text, uint64_t* out) {
  if (out == nullptr || text.empty()) return false;
  uint64_t value = 0;
  for (char ch : text) {
    if (ch < '0' || ch > '9') return false;
    const uint64_t digit = static_cast<uint64_t>(ch - '0');
    if (value > (std::numeric_limits<uint64_t>::max() - digit) / 10) return false;
    value = value * 10 + digit;
  }
  *out = value;
  return true;
}

bool ReadExecArgvUint64Option(const char* prefix, uint64_t* out, bool* found) {
  if (found != nullptr) *found = false;
  if (prefix == nullptr || out == nullptr) return false;
  const std::string needle(prefix);
  for (const auto& arg : g_edge_effective_exec_argv) {
    if (arg.rfind(needle, 0) != 0) continue;
    std::string_view raw(arg.data() + needle.size(), arg.size() - needle.size());
    uint64_t parsed = 0;
    if (!ParseUint64(raw, &parsed)) return false;
    *out = parsed;
    if (found != nullptr) *found = true;
  }
  return true;
}

bool ReadExecArgvStringOptionFrom(const std::vector<std::string>& exec_argv,
                                  const char* prefix,
                                  std::string* out,
                                  bool* found) {
  if (found != nullptr) *found = false;
  if (prefix == nullptr || out == nullptr) return false;
  const std::string needle(prefix);
  for (const auto& arg : exec_argv) {
    if (arg.rfind(needle, 0) != 0) continue;
    out->assign(arg.data() + needle.size(), arg.size() - needle.size());
    if (found != nullptr) *found = true;
  }
  return true;
}

bool ExecArgvHasFlagIn(const std::vector<std::string>& exec_argv, const char* flag) {
  if (flag == nullptr || flag[0] == '\0') return false;
  for (const auto& arg : exec_argv) {
    if (arg == flag) return true;
  }
  return false;
}

std::string GetOpenSslErrorString() {
  std::string out;
  ERR_print_errors_cb(
      [](const char* str, size_t len, void* opaque) -> int {
        std::string* text = static_cast<std::string*>(opaque);
        text->append(str, len);
        text->push_back('\n');
        return 0;
      },
      static_cast<void*>(&out));
  return out;
}

bool ConfigureSecureHeapFromExecArgv(std::string* error_out) {
  uint64_t secure_heap = 0;
  uint64_t secure_heap_min = 0;
  bool has_secure_heap = false;
  bool has_secure_heap_min = false;
  const bool parsed_heap =
      ReadExecArgvUint64Option("--secure-heap=", &secure_heap, &has_secure_heap);
  const bool parsed_heap_min =
      ReadExecArgvUint64Option("--secure-heap-min=", &secure_heap_min, &has_secure_heap_min);
  if (!parsed_heap || !parsed_heap_min) {
    if (error_out != nullptr) {
      *error_out = "Invalid --secure-heap or --secure-heap-min value";
    }
    return false;
  }
  if (!has_secure_heap && !has_secure_heap_min) return true;

  if (!has_secure_heap) secure_heap = 0;
  if (!has_secure_heap_min) secure_heap_min = 0;

  std::string error;
  if (!IsPowerOfTwo(secure_heap)) {
    error += "--secure-heap must be a power of 2";
  }
  if (!IsPowerOfTwo(secure_heap_min)) {
    if (!error.empty()) error += "\n";
    error += "--secure-heap-min must be a power of 2";
  }
  if (!error.empty()) {
    if (error_out != nullptr) *error_out = error;
    return false;
  }

  if (CRYPTO_secure_malloc_initialized() == 0 &&
      CRYPTO_secure_malloc_init(static_cast<size_t>(secure_heap), static_cast<size_t>(secure_heap_min)) != 1) {
    if (error_out != nullptr) {
      *error_out = "Failed to initialize OpenSSL secure heap";
    }
    return false;
  }
  return true;
}

bool ConfigureOpenSslFromExecArgv(const std::vector<std::string>& exec_argv,
                                  std::string* error_out) {
#if OPENSSL_VERSION_MAJOR < 3
  return true;
#else
  static std::mutex openssl_config_mutex;
  static bool openssl_config_loaded = false;
  std::lock_guard<std::mutex> openssl_config_lock(openssl_config_mutex);
  if (openssl_config_loaded) return true;

  const char* conf_file = nullptr;
  const char* conf_section_name = "nodejs_conf";
  if (ExecArgvHasFlagIn(exec_argv, "--openssl-shared-config")) {
    conf_section_name = "openssl_conf";
  }

  std::string default_openssl_conf;
#if defined(__linux__)
  default_openssl_conf = "/etc/ssl/openssl.cnf";
#else
  char* default_conf_file = CONF_get1_default_config_file();
  if (default_conf_file != nullptr) {
    default_openssl_conf = default_conf_file;
    OPENSSL_free(default_conf_file);
  }
#endif
  if (!default_openssl_conf.empty()) {
    conf_file = default_openssl_conf.c_str();
  }

  std::string env_openssl_conf;
  if (const char* env = std::getenv("OPENSSL_CONF"); env != nullptr && env[0] != '\0') {
    env_openssl_conf = env;
    conf_file = env_openssl_conf.c_str();
  }

  std::string arg_openssl_conf;
  bool has_arg_openssl_conf = false;
  if (!ReadExecArgvStringOptionFrom(
          exec_argv, "--openssl-config=", &arg_openssl_conf, &has_arg_openssl_conf)) {
    if (error_out != nullptr) {
      *error_out = "Invalid --openssl-config value";
    }
    return false;
  }
  if (has_arg_openssl_conf) {
    conf_file = arg_openssl_conf.c_str();
  }

  ERR_clear_error();
  const int config_result =
      CONF_modules_load_file(conf_file, conf_section_name, CONF_MFLAGS_IGNORE_MISSING_FILE);
  const int init_result = OPENSSL_init_crypto(OPENSSL_INIT_NO_LOAD_CONFIG, nullptr);

  if (config_result <= 0 || init_result != 1 || ERR_peek_error() != 0) {
    if (error_out != nullptr) {
      *error_out = "OpenSSL configuration error:\n" + GetOpenSslErrorString();
    }
    return false;
  }

  openssl_config_loaded = true;
  return true;
#endif
}

bool ExecArgvHasFlag(const char* flag) {
  if (flag == nullptr || flag[0] == '\0') return false;
  for (const auto& arg : g_edge_effective_exec_argv) {
    if (arg == flag) return true;
  }
  return false;
}

bool ExecArgvHasAnyFlag(std::initializer_list<const char*> flags) {
  for (const char* flag : flags) {
    if (ExecArgvHasFlag(flag)) return true;
  }
  return false;
}

bool CliSourceRunsMainEntry(const char* source_text) {
  if (source_text == nullptr) return false;
  return std::strstr(source_text, "require('internal/main/") != nullptr ||
         std::strstr(source_text, "require(\"internal/main/") != nullptr ||
         std::strstr(source_text, "__edge_skip_pre_execution__") != nullptr;
}

bool ShouldExposeGc() {
  return ExecArgvHasFlag("--expose-gc") || ExecArgvHasFlag("--expose_gc");
}

bool ShouldEnableSharedArrayBufferPerContext() {
  return ExecArgvHasFlag("--enable-sharedarraybuffer-per-context");
}

napi_value GlobalGcCallback(napi_env env, napi_callback_info /*info*/) {
  if (unofficial_napi_request_gc_for_testing(env) != napi_ok) {
    napi_throw_error(env, nullptr, "Failed to run gc()");
    return nullptr;
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

bool EnsureGlobalGcIfRequested(napi_env env, napi_value global, std::string* error_out) {
  if (!ShouldExposeGc()) return true;
  if (env == nullptr || global == nullptr) return false;

  bool has_gc = false;
  if (napi_has_named_property(env, global, "gc", &has_gc) != napi_ok) return false;
  if (has_gc) {
    napi_value existing_gc = nullptr;
    napi_valuetype type = napi_undefined;
    if (napi_get_named_property(env, global, "gc", &existing_gc) == napi_ok &&
        existing_gc != nullptr &&
        napi_typeof(env, existing_gc, &type) == napi_ok &&
        type == napi_function) {
      return true;
    }
  }

  napi_value gc_fn = nullptr;
  if (napi_create_function(env, "gc", NAPI_AUTO_LENGTH, GlobalGcCallback, nullptr, &gc_fn) != napi_ok ||
      gc_fn == nullptr ||
      napi_set_named_property(env, global, "gc", gc_fn) != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "Failed to install global gc()";
    }
    return false;
  }
  return true;
}

napi_value ConsoleLogCallback(napi_env env, napi_callback_info info) {
  size_t argc = 8;
  napi_value args[8] = {nullptr};
  napi_status status = napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
  if (status == napi_ok) {
    std::string line;
    for (size_t i = 0; i < argc; ++i) {
      if (i > 0) line.push_back(' ');
      napi_value string_value = nullptr;
      if (napi_coerce_to_string(env, args[i], &string_value) != napi_ok || string_value == nullptr) {
        continue;
      }
      size_t length = 0;
      if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &length) != napi_ok) {
        continue;
      }
      std::string out(length + 1, '\0');
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) {
        continue;
      }
      out.resize(copied);
      line += out;
    }
    line.push_back('\n');
    WriteTextToFd(1, line);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ConsoleErrorCallback(napi_env env, napi_callback_info info) {
  size_t argc = 16;
  napi_value args[16];
  napi_status status = napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
  if (status == napi_ok) {
    std::string line;
    for (size_t i = 0; i < argc; ++i) {
      if (i > 0) line.push_back(' ');
      napi_value string_value = nullptr;
      if (napi_coerce_to_string(env, args[i], &string_value) != napi_ok || string_value == nullptr) {
        continue;
      }
      size_t length = 0;
      if (napi_get_value_string_utf8(env, string_value, nullptr, 0, &length) != napi_ok) {
        continue;
      }
      std::string out(length + 1, '\0');
      size_t copied = 0;
      if (napi_get_value_string_utf8(env, string_value, out.data(), out.size(), &copied) != napi_ok) {
        continue;
      }
      out.resize(copied);
      line += out;
    }
    line.push_back('\n');
    WriteTextToFd(2, line);
  }
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

napi_value ReturnUndefinedCallback(napi_env env, napi_callback_info /*info*/) {
  napi_value undefined = nullptr;
  napi_get_undefined(env, &undefined);
  return undefined;
}

bool GetNamedProperty(napi_env env, napi_value obj, const char* key, napi_value* out) {
  if (out == nullptr) return false;
  *out = nullptr;
  if (obj == nullptr) return false;
  return napi_get_named_property(env, obj, key, out) == napi_ok && *out != nullptr;
}

bool IsFunction(napi_env env, napi_value value) {
  if (value == nullptr) return false;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_function;
}

bool IsUndefinedValue(napi_env env, napi_value value) {
  if (value == nullptr) return true;
  napi_valuetype type = napi_undefined;
  return napi_typeof(env, value, &type) == napi_ok && type == napi_undefined;
}

bool CallFunction(napi_env env,
                  napi_value recv,
                  napi_value fn,
                  size_t argc,
                  napi_value* argv,
                  napi_value* out) {
  if (env == nullptr || recv == nullptr || fn == nullptr) return false;
  napi_value result = nullptr;
  if (napi_call_function(env, recv, fn, argc, argv, &result) != napi_ok) return false;
  if (out != nullptr) *out = result;
  return true;
}

bool RequireModule(napi_env env, const char* id, napi_value* out) {
  if (out != nullptr) *out = nullptr;
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value require_fn = EdgeGetRequireFunction(env);
  if (!IsFunction(env, require_fn) &&
      (!GetNamedProperty(env, global, "require", &require_fn) || !IsFunction(env, require_fn))) {
    return false;
  }

  napi_value id_value = nullptr;
  if (napi_create_string_utf8(env, id, NAPI_AUTO_LENGTH, &id_value) != napi_ok || id_value == nullptr) {
    return false;
  }

  return CallFunction(env, global, require_fn, 1, &id_value, out);
}

bool EnsureProcessConstructorPrototypeLink(napi_env env, napi_value process_proto) {
  if (env == nullptr || process_proto == nullptr) return false;

  napi_value constructor_key = nullptr;
  if (napi_create_string_utf8(env, "constructor", NAPI_AUTO_LENGTH, &constructor_key) != napi_ok ||
      constructor_key == nullptr) {
    return false;
  }

  bool has_own_constructor = false;
  if (napi_has_own_property(env, process_proto, constructor_key, &has_own_constructor) != napi_ok ||
      !has_own_constructor) {
    return true;
  }

  napi_value ctor = nullptr;
  if (!GetNamedProperty(env, process_proto, "constructor", &ctor) || !IsFunction(env, ctor)) {
    return true;
  }

  napi_value ctor_proto = nullptr;
  if (!GetNamedProperty(env, ctor, "prototype", &ctor_proto) || ctor_proto == nullptr) {
    return true;
  }

  bool already_linked = false;
  if (napi_strict_equals(env, ctor_proto, process_proto, &already_linked) != napi_ok) {
    return false;
  }
  if (already_linked) return true;

  return napi_set_named_property(env, ctor, "prototype", process_proto) == napi_ok;
}

bool PrepareProcessPrototypeForBootstrap(napi_env env, std::string* error_out) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) return false;

  napi_value process_obj = nullptr;
  if (!GetNamedProperty(env, global, "process", &process_obj)) return false;

  napi_value process_proto = nullptr;
  if (napi_get_prototype(env, process_obj, &process_proto) != napi_ok || process_proto == nullptr) {
    return false;
  }

  napi_value object_ctor = nullptr;
  if (!GetNamedProperty(env, global, "Object", &object_ctor)) return false;
  napi_value object_proto = nullptr;
  if (!GetNamedProperty(env, object_ctor, "prototype", &object_proto)) return false;

  bool is_default_object_proto = false;
  if (napi_strict_equals(env, process_proto, object_proto, &is_default_object_proto) != napi_ok ||
      !is_default_object_proto) {
    if (!EnsureProcessConstructorPrototypeLink(env, process_proto)) {
      if (error_out != nullptr) {
        bool is_exit = false;
        int exit_code = 0;
        std::string msg = "Failed to link process.constructor.prototype";
        const std::string exc = GetAndClearPendingException(env, &is_exit, &exit_code);
        if (!exc.empty()) {
          msg += ": ";
          msg += exc;
        }
        *error_out = msg;
      }
      return false;
    }
    return true;
  }

  napi_value fresh_process_proto = nullptr;
  if (napi_create_object(env, &fresh_process_proto) != napi_ok || fresh_process_proto == nullptr) {
    return false;
  }
  // Node bootstrap expects process.__proto__.constructor.name === "process".
  // Keep this constructor on the process prototype before inheriting EventEmitter.prototype.
  napi_value process_ctor = nullptr;
  if (napi_create_function(env,
                           "process",
                           NAPI_AUTO_LENGTH,
                           ReturnUndefinedCallback,
                           nullptr,
                           &process_ctor) != napi_ok ||
      process_ctor == nullptr) {
    return false;
  }
  napi_property_descriptor constructor_desc = {};
  constructor_desc.utf8name = "constructor";
  constructor_desc.value = process_ctor;
  constructor_desc.attributes =
      static_cast<napi_property_attributes>(napi_writable | napi_configurable);
  if (napi_define_properties(env, fresh_process_proto, 1, &constructor_desc) != napi_ok) {
    return false;
  }
  if (napi_set_named_property(env, process_ctor, "prototype", fresh_process_proto) != napi_ok) {
    return false;
  }

  napi_value set_prototype_of = nullptr;
  if (!GetNamedProperty(env, object_ctor, "setPrototypeOf", &set_prototype_of) ||
      !IsFunction(env, set_prototype_of)) {
    return false;
  }

  napi_value argv[2] = {process_obj, fresh_process_proto};
  if (!CallFunction(env, object_ctor, set_prototype_of, 2, argv, nullptr)) {
    if (error_out != nullptr) {
      bool is_exit = false;
      int exit_code = 0;
      std::string msg = "Failed to prepare process prototype for bootstrap";
      const std::string exc = GetAndClearPendingException(env, &is_exit, &exit_code);
      if (!exc.empty()) {
        msg += ": ";
        msg += exc;
      }
      *error_out = msg;
    }
    return false;
  }

  return true;
}

int RunScriptWithGlobals(napi_env env,
                         const char* source_text,
                         const char* entry_script_path,
                         const char* native_main_builtin_id,
                         std::string* error_out,
                         bool keep_event_loop_alive,
                         EdgeBootstrapMode mode) {
  InitializeProcessStdioInheritanceOnce();
#if !defined(_WIN32)
  InstallDefaultSignalBehavior();
#endif
  if (env == nullptr) {
    if (error_out != nullptr) {
      *error_out = "Invalid environment";
    }
    return 1;
  }
  auto should_abort_worker_bootstrap = [&]() -> bool {
    return !EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env);
  };
  if (EdgeRuntimePlatformInstallHooks(env) != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "Failed to attach runtime platform hooks";
    }
    return 1;
  }
  if (should_abort_worker_bootstrap()) return 1;
  if (EdgeInitializeTimersHost(env) != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "Failed to initialize timers host";
    }
    return 1;
  }
  if (should_abort_worker_bootstrap()) return 1;
  if (source_text == nullptr || source_text[0] == '\0') {
    if (error_out != nullptr) {
      *error_out = "Empty script source";
    }
    return 1;
  }
  ParseNodeStyleFlagsFromSource(env, source_text);
  if (!ConfigureSecureHeapFromExecArgv(error_out)) {
    return 1;
  }

  // Activate the embedded ICU common data before any locale-aware code runs, so
  // ICU-backed Intl (and legacy charset conversion) sees real data instead of
  // the empty stubdata image.
  {
    std::string icu_data_error;
    if (!EdgeActivateIcuData(&icu_data_error)) {
      fprintf(stderr, "[edge] warning: %s\n", icu_data_error.c_str());
    }
  }

  // Install the ICU-backed Intl surface. On the QuickJS provider this is the
  // entire Intl (the engine ships none); on the V8 provider each constructor is
  // skipped because V8 already exposes a native one. Non-fatal.
  {
    std::string intl_error;
    if (!EdgeInstallIntl(env, &intl_error)) {
      fprintf(stderr, "[edge] warning: %s\n", intl_error.c_str());
    }
  }
  if (should_abort_worker_bootstrap()) return 1;

  napi_status status = EdgeInstallProcessObject(
      env, g_edge_current_script_path, g_edge_exec_argv, g_edge_script_argv, g_edge_process_title);
  if (status != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "EdgeInstallProcessObject failed: " + StatusToString(status);
    }
    return 1;
  }
  if (should_abort_worker_bootstrap()) return 1;

  status = EdgeInstallModuleLoader(env, entry_script_path);
  if (status != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "EdgeInstallModuleLoader failed: " + StatusToString(status);
    }
    return 1;
  }
  if (should_abort_worker_bootstrap()) return 1;

  // Create empty primordials container on the native side first (Node-aligned).
  napi_value primordials_container = nullptr;
  if (napi_create_object(env, &primordials_container) != napi_ok || primordials_container == nullptr) {
    if (error_out != nullptr) {
      *error_out = "napi_create_object(primordials) failed";
    }
    return 1;
  }
  EdgeSetPrimordials(env, primordials_container);

  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    if (error_out != nullptr) {
      *error_out = "Failed to fetch global object";
    }
    return 1;
  }
  if (ShouldEnableSharedArrayBufferPerContext()) {
    napi_value sab_key = nullptr;
    if (napi_create_string_utf8(env, "SharedArrayBuffer", NAPI_AUTO_LENGTH, &sab_key) == napi_ok &&
        sab_key != nullptr) {
      bool deleted = false;
      (void)napi_delete_property(env, global, sab_key, &deleted);
    }
  }
  if (!EnsureGlobalGcIfRequested(env, global, error_out)) {
    if (error_out != nullptr && error_out->empty()) {
      *error_out = "Failed to expose global gc()";
    }
    return 1;
  }
  if (!PrepareProcessPrototypeForBootstrap(env, error_out)) {
    if (error_out != nullptr && error_out->empty()) {
      *error_out = "Failed to prepare process prototype for bootstrap";
    }
    return 1;
  }
  if (should_abort_worker_bootstrap()) return 1;
  auto execute_bootstrapper = [&](const char* id, napi_value* out_result) -> bool {
    if (should_abort_worker_bootstrap()) {
      return false;
    }
    napi_value result = nullptr;
    if (EdgeExecuteBuiltin(env, id, &result)) {
      if (out_result != nullptr) *out_result = result;
      return true;
    }
    if (error_out != nullptr) {
      std::string msg = std::string("Failed to execute ") + id;
      bool is_exit = false;
      int exit_code = 0;
      const std::string exc = GetAndClearPendingException(env, &is_exit, &exit_code);
      if (!exc.empty()) {
        msg += ": ";
        msg += exc;
      }
      *error_out = msg;
    }
    return false;
  };

  auto define_hidden_global = [&](const char* name, napi_value value) -> bool {
    if (name == nullptr || value == nullptr) return false;
    napi_property_descriptor desc = {};
    desc.utf8name = name;
    desc.value = value;
    desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    return napi_define_properties(env, global, 1, &desc) == napi_ok;
  };

  napi_value native_internal_binding = nullptr;
  if (!GetNamedProperty(env, global, "internalBinding", &native_internal_binding) ||
      !IsFunction(env, native_internal_binding)) {
    if (error_out != nullptr) {
      *error_out = "Native internalBinding callback is not installed on global";
    }
    return 1;
  }
  if (!define_hidden_global("getInternalBinding", native_internal_binding)) {
    if (error_out != nullptr) {
      *error_out = "Failed to expose getInternalBinding bootstrap hook";
    }
    return 1;
  }
  {
    napi_value private_symbols = EdgeGetPrivateSymbols(env);
    if (private_symbols == nullptr || IsUndefinedValue(env, private_symbols)) {
      private_symbols = EdgeCreatePrivateSymbolsObject(env);
      if (private_symbols == nullptr) {
        if (error_out != nullptr) {
          *error_out = "Failed to initialize bootstrap privateSymbols";
        }
        return 1;
      }
      EdgeSetPrivateSymbols(env, private_symbols);
    }

    napi_value per_isolate_symbols = EdgeGetPerIsolateSymbols(env);
    if (per_isolate_symbols == nullptr || IsUndefinedValue(env, per_isolate_symbols)) {
      per_isolate_symbols = EdgeCreatePerIsolateSymbolsObject(env);
      if (per_isolate_symbols == nullptr) {
        if (error_out != nullptr) {
          *error_out = "Failed to initialize bootstrap perIsolateSymbols";
        }
        return 1;
      }
      EdgeSetPerIsolateSymbols(env, per_isolate_symbols);
    }
  }
  napi_value get_linked_binding = nullptr;
  if (napi_create_function(env,
                           "getLinkedBinding",
                           NAPI_AUTO_LENGTH,
                           ReturnUndefinedCallback,
                           nullptr,
                           &get_linked_binding) == napi_ok &&
      get_linked_binding != nullptr) {
    define_hidden_global("getLinkedBinding", get_linked_binding);
  }
#if defined(EDGE_NAPI_QUICKJS) && defined(EDGE_QUICKJS_WEBASSEMBLY)
  if (!EdgeInstallQuickJsWebAssembly(env, error_out)) {
    if (error_out != nullptr && error_out->empty()) {
      *error_out = "Failed to install QuickJS WebAssembly";
    }
    return 1;
  }
#endif
  if (!execute_bootstrapper("internal/per_context/primordials", nullptr)) {
    if (should_abort_worker_bootstrap()) return 1;
    return 1;
  }
  if (!execute_bootstrapper("internal/per_context/domexception", nullptr)) {
    if (should_abort_worker_bootstrap()) return 1;
    return 1;
  }
  if (!execute_bootstrapper("internal/per_context/messageport", nullptr)) {
    if (should_abort_worker_bootstrap()) return 1;
    return 1;
  }
  if (napi_set_named_property(env, global, "primordials", primordials_container) != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "Failed to expose primordials during bootstrap";
    }
    return 1;
  }

  if (!execute_bootstrapper("internal/bootstrap/realm", nullptr)) {
    if (should_abort_worker_bootstrap()) return 1;
    return 1;
  }

  {
    napi_value primordials_key = nullptr;
    if (napi_create_string_utf8(env, "primordials", NAPI_AUTO_LENGTH, &primordials_key) == napi_ok &&
        primordials_key != nullptr) {
      bool deleted = false;
      (void)napi_delete_property(env, global, primordials_key, &deleted);
    }
  }

  napi_value internal_binding = EdgeGetBuiltinInternalBinding(env);
  if (!IsFunction(env, internal_binding)) {
    internal_binding = EdgeGetInternalBinding(env);
  }
  if (!IsFunction(env, internal_binding)) {
    internal_binding = native_internal_binding;
  }
  if (!define_hidden_global("internalBinding", internal_binding)) {
    if (error_out != nullptr) {
      *error_out = "Failed to install global internalBinding";
    }
    return 1;
  }
  EdgeSetInternalBinding(env, internal_binding);

  // Node's C++ bootstrap populates process[exit_info_private_symbol] before
  // internal/bootstrap/node runs. Mirror that setup so process._exiting and
  // process.exitCode accessors have backing storage.
  {
    constexpr uint32_t kExitInfoExitingIndex = 0;
    constexpr uint32_t kExitInfoExitCodeIndex = 1;
    constexpr uint32_t kExitInfoHasExitCodeIndex = 2;

    napi_value process_obj = nullptr;
    if (!GetNamedProperty(env, global, "process", &process_obj)) {
      if (error_out != nullptr) {
        *error_out = "Failed to fetch process object during bootstrap";
      }
      return 1;
    }

    napi_value private_symbols = EdgeGetPrivateSymbols(env);
    if (private_symbols == nullptr || IsUndefinedValue(env, private_symbols)) {
      if (error_out != nullptr) {
        *error_out = "Bootstrap privateSymbols state missing during exit info setup";
      }
      return 1;
    }

    napi_value exit_info_symbol = nullptr;
    if (!GetNamedProperty(env, private_symbols, "exit_info_private_symbol", &exit_info_symbol)) {
      if (error_out != nullptr) {
        *error_out = "util.privateSymbols.exit_info_private_symbol missing";
      }
      return 1;
    }

    napi_value existing_exit_info = nullptr;
    if (napi_get_property(env, process_obj, exit_info_symbol, &existing_exit_info) == napi_ok &&
        existing_exit_info != nullptr &&
        !IsUndefinedValue(env, existing_exit_info)) {
      // Already initialized.
    } else {
      napi_value fields = nullptr;
      if (napi_create_object(env, &fields) != napi_ok || fields == nullptr) {
        if (error_out != nullptr) {
          *error_out = "Failed to create exit info fields object";
        }
        return 1;
      }
      napi_value zero = nullptr;
      if (napi_create_int32(env, 0, &zero) != napi_ok || zero == nullptr) {
        if (error_out != nullptr) {
          *error_out = "Failed to create zero for exit info fields";
        }
        return 1;
      }
      napi_set_element(env, fields, kExitInfoExitCodeIndex, zero);
      napi_set_element(env, fields, kExitInfoExitingIndex, zero);
      napi_set_element(env, fields, kExitInfoHasExitCodeIndex, zero);
      if (napi_set_property(env, process_obj, exit_info_symbol, fields) != napi_ok) {
        if (error_out != nullptr) {
          *error_out = "Failed to attach process exit info fields";
        }
        return 1;
      }
    }
  }

  EdgeSetPrimordials(env, primordials_container);

  const char* thread_switch_module = mode == EdgeBootstrapMode::kWorkerThread
                                         ? "internal/bootstrap/switches/is_not_main_thread"
                                         : "internal/bootstrap/switches/is_main_thread";
  const char* process_state_switch_module = mode == EdgeBootstrapMode::kWorkerThread
                                                ? "internal/bootstrap/switches/does_not_own_process_state"
                                                : "internal/bootstrap/switches/does_own_process_state";
  if (!execute_bootstrapper("internal/bootstrap/node", nullptr) ||
      !execute_bootstrapper("internal/bootstrap/web/exposed-wildcard", nullptr) ||
      !execute_bootstrapper("internal/bootstrap/web/exposed-window-or-worker", nullptr) ||
      !execute_bootstrapper(thread_switch_module, nullptr) ||
      !execute_bootstrapper(process_state_switch_module, nullptr)) {
    if (should_abort_worker_bootstrap()) return 1;
    return 1;
  }

  // Bridge V8 host dynamic import (napi/v8) into Node's module_wrap callback
  // registry so import('node:...') from CJS follows Node's ESM pathway.
  {
    napi_value module_wrap_name = nullptr;
    if (napi_create_string_utf8(env, "module_wrap", NAPI_AUTO_LENGTH, &module_wrap_name) != napi_ok ||
        module_wrap_name == nullptr) {
      if (error_out != nullptr) {
        *error_out = "Failed to create module_wrap binding key";
      }
      return 1;
    }

    napi_value module_wrap_binding = nullptr;
    if (!CallFunction(env, global, internal_binding, 1, &module_wrap_name, &module_wrap_binding) ||
        module_wrap_binding == nullptr) {
      if (error_out != nullptr) {
        std::string msg = "Failed to resolve internalBinding('module_wrap')";
        bool is_exit = false;
        int exit_code = 0;
        const std::string exc = GetAndClearPendingException(env, &is_exit, &exit_code);
        if (!exc.empty()) {
          msg += ": ";
          msg += exc;
        }
        *error_out = msg;
      }
      return 1;
    }

    napi_value import_dynamically = nullptr;
    if (!GetNamedProperty(env, module_wrap_binding, "importModuleDynamically", &import_dynamically) ||
        !IsFunction(env, import_dynamically)) {
      if (error_out != nullptr) {
        *error_out = "module_wrap binding missing importModuleDynamically";
      }
      return 1;
    }

    napi_value process_obj = nullptr;
    if (!GetNamedProperty(env, global, "process", &process_obj) || process_obj == nullptr) {
      if (error_out != nullptr) {
        *error_out = "Failed to resolve process object for __napi_dynamic_import bridge";
      }
      return 1;
    }
    napi_property_descriptor desc = {};
    desc.utf8name = "__napi_dynamic_import";
    desc.value = import_dynamically;
    desc.attributes = static_cast<napi_property_attributes>(napi_writable | napi_configurable);
    if (napi_define_properties(env, process_obj, 1, &desc) != napi_ok) {
      if (error_out != nullptr) {
        *error_out = "Failed to install __napi_dynamic_import bridge";
      }
      return 1;
    }
  }

  // Bootstrapped JS may assign these via plain property sets; enforce
  // Node-like hidden bootstrap hooks before user code starts.
  if (!define_hidden_global("internalBinding", internal_binding) ||
      !define_hidden_global("getInternalBinding", native_internal_binding) ||
      (get_linked_binding != nullptr && !define_hidden_global("getLinkedBinding", get_linked_binding))) {
    if (error_out != nullptr) {
      *error_out = "Failed to finalize hidden internal binding globals";
    }
    return 1;
  }
  auto delete_global_named = [&](const char* name) {
    if (name == nullptr) return;
    napi_value key = nullptr;
    if (napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &key) != napi_ok || key == nullptr) return;
    bool deleted = false;
    (void)napi_delete_property(env, global, key, &deleted);
  };
  // Hide `internalBinding` from user-script globals while keeping internal
  // test harness source-mode compatibility.
  if (mode == EdgeBootstrapMode::kMainThread &&
      entry_script_path != nullptr &&
      entry_script_path[0] != '\0') {
    delete_global_named("internalBinding");
  }

  const char* source_to_run = source_text;
  const char* selected_main_builtin_id = native_main_builtin_id;
  if (entry_script_path != nullptr && entry_script_path[0] != '\0') {
    const bool check_syntax_mode = ExecArgvHasAnyFlag({"--check", "-c"});
    selected_main_builtin_id = check_syntax_mode ? "internal/main/check_syntax" : "internal/main/run_main_module";
  }
  if (mode == EdgeBootstrapMode::kWorkerThread &&
      selected_main_builtin_id != nullptr &&
      std::strcmp(selected_main_builtin_id, "internal/main/worker_thread") == 0) {
    delete_global_named("require");
    delete_global_named("__filename");
    delete_global_named("__dirname");
  }

  napi_value result = nullptr;
  if (selected_main_builtin_id != nullptr && selected_main_builtin_id[0] != '\0') {
    if (EdgeExecuteBuiltin(env, selected_main_builtin_id, &result)) {
      status = napi_ok;
    } else {
      bool has_pending_exception = false;
      if (napi_is_exception_pending(env, &has_pending_exception) == napi_ok && has_pending_exception) {
        status = napi_pending_exception;
      } else {
        if (error_out != nullptr) {
          *error_out = std::string("Failed to execute ") + selected_main_builtin_id;
        }
        return 1;
      }
    }
  } else {
    napi_value script = nullptr;
    status = napi_create_string_utf8(env, source_to_run, NAPI_AUTO_LENGTH, &script);
    if (status != napi_ok || script == nullptr) {
      if (error_out != nullptr) {
        *error_out = "napi_create_string_utf8 failed: " + StatusToString(status);
      }
      return 1;
    }
    status = napi_run_script(env, script, &result);
  }
  if (status == napi_ok) {
    napi_value wait_target = result;
    if (DebugExceptionsEnabled()) {
      std::cerr << "[edge-esm] script result pending=" << (IsPromisePending(env, wait_target) ? "true" : "false")
                << "\n";
    }
    if (!IsPromisePending(env, wait_target)) {
      napi_value entry_promise = GetEntryPointPromiseValue(env);
      if (DebugExceptionsEnabled()) {
        std::cerr << "[edge-esm] entry promise pending="
                  << (IsPromisePending(env, entry_promise) ? "true" : "false") << "\n";
      }
      if (IsPromisePending(env, entry_promise)) {
        wait_target = entry_promise;
      }
    }

    if (!keep_event_loop_alive) {
      const int promise_status = WaitForTopLevelPromiseToSettle(env, wait_target, error_out);
      if (promise_status >= 0) {
        return promise_status;
      }
    }

    // Node semantics: flush the task queues once after top-level script eval.
    (void)DrainProcessTickCallback(env);
    const int post_script_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (post_script_status >= 0) {
      return post_script_status;
    }

    if (keep_event_loop_alive) {
      // Mirror Node's embedder loop semantics for process lifetime and beforeExit handling.
      const int loop_result = RunEventLoopUntilQuiescent(env, error_out);
      if (loop_result >= 0) {
        return loop_result;
      }
    }
    bool has_exit_code = false;
    const int exit_code = GetProcessExitCode(env, &has_exit_code);
    if (has_exit_code) {
      return exit_code;
    }
    if (IsEnvironmentExitRequested(env)) {
      return GetEnvironmentExitCodeOrFallback(env, 0);
    }
    return 0;
  }

  bool handled_top_level_exception = false;
  PendingExceptionInfo top_level_exception = {};
  if (TakePendingExceptionInfo(env, &top_level_exception) &&
      top_level_exception.has_exception &&
      top_level_exception.exception != nullptr) {
    const int exception_status =
        HandleExtractedException(
            env,
            top_level_exception.exception,
            error_out,
            top_level_exception.exception_line,
            top_level_exception.thrown_at);
    if (exception_status >= 0) {
      return exception_status;
    }
    handled_top_level_exception = true;
  }

  (void)DrainProcessTickCallback(env);
  bool has_post_exception = false;
  if (napi_is_exception_pending(env, &has_post_exception) == napi_ok && has_post_exception) {
    const int post_exception_status = HandlePendingExceptionAfterLoopStep(env, error_out);
    if (post_exception_status >= 0) {
      return post_exception_status;
    }
    handled_top_level_exception = true;
  }

  if (keep_event_loop_alive) {
    const int loop_result = RunEventLoopUntilQuiescent(env, error_out);
    if (loop_result >= 0) {
      return loop_result;
    }
  }
  bool has_exit_code = false;
  const int exit_code = GetProcessExitCode(env, &has_exit_code);
  if (has_exit_code) {
    return exit_code;
  }
  if (IsEnvironmentExitRequested(env)) {
    return GetEnvironmentExitCodeOrFallback(env, 0);
  }
  if (handled_top_level_exception) {
    return 0;
  }

  const std::string exception_message = GetAndClearPendingException(env, nullptr, nullptr);
  if (error_out != nullptr) {
    if (!exception_message.empty()) {
      *error_out = exception_message;
    } else {
      *error_out = "napi_run_script failed: " + StatusToString(status);
    }
  }
  return 1;
}

}  // namespace

bool EdgeExecArgvHasFlag(const char* flag) {
  return ExecArgvHasFlag(flag);
}

bool EdgeReadExecArgvUint64Option(const char* prefix, uint64_t* out, bool* found) {
  return ReadExecArgvUint64Option(prefix, out, found);
}

void EdgePrepareProcessExit(napi_env env, int exit_code) {
  napi_value global = nullptr;
  if (napi_get_global(env, &global) != napi_ok || global == nullptr) {
    return;
  }

  bool has_process = false;
  if (napi_has_named_property(env, global, "process", &has_process) != napi_ok || !has_process) {
    return;
  }

  napi_value process_obj = nullptr;
  if (napi_get_named_property(env, global, "process", &process_obj) != napi_ok || process_obj == nullptr) {
    return;
  }

  napi_value exit_code_value = nullptr;
  if (napi_create_int32(env, static_cast<int32_t>(exit_code), &exit_code_value) == napi_ok &&
      exit_code_value != nullptr) {
    (void)napi_set_named_property(env, process_obj, "exitCode", exit_code_value);
  }

  napi_value exiting_value = nullptr;
  if (napi_get_boolean(env, true, &exiting_value) == napi_ok && exiting_value != nullptr) {
    (void)napi_set_named_property(env, process_obj, "_exiting", exiting_value);
  }
}

napi_status EdgeMakeCallbackWithFlags(napi_env env,
                                     napi_value recv,
                                     napi_value callback,
                                     size_t argc,
                                     napi_value* argv,
                                     napi_value* result,
                                     int flags) {
  if (env == nullptr || recv == nullptr || callback == nullptr) {
    return napi_invalid_arg;
  }
  thread_local int detached_callback_scope_depth = 0;
  edge::Environment* environment = EdgeEnvironmentGet(env);
  if (environment != nullptr) {
    environment->IncrementAsyncCallbackScopeDepth();
  } else {
    detached_callback_scope_depth++;
  }
  napi_status status = EdgeCallCallbackWithDomain(env, recv, callback, argc, argv, result);

  auto handle_pending_exception = [&](napi_status current_status) -> napi_status {
    bool has_pending = false;
    if (napi_is_exception_pending(env, &has_pending) != napi_ok || !has_pending) {
      return current_status;
    }

    std::string fatal_error;
    const int fatal_status = HandlePendingExceptionAfterLoopStep(env, &fatal_error);
    if (fatal_status >= 0) {
      if (!fatal_error.empty()) {
        if (fatal_error.back() != '\n') fatal_error.push_back('\n');
        WriteTextToFd(2, fatal_error);
      }
      if (EdgeWorkerEnvIsMainThread(env) && !EdgeWorkerEnvIsInternalThread(env)) {
        std::_Exit(fatal_status);
      }
      EdgeWorkerEnvRequestStop(env);
      if (uv_loop_t* loop = EdgeGetEnvLoop(env); loop != nullptr) {
        uv_stop(loop);
      }
      return napi_pending_exception;
    }

    return current_status == napi_pending_exception ? napi_ok : current_status;
  };

  const bool skip_task_queues = (flags & kEdgeMakeCallbackSkipTaskQueues) != 0;
  const size_t callback_scope_depth =
      environment != nullptr ? environment->async_callback_scope_depth()
                             : static_cast<size_t>(detached_callback_scope_depth);
  status = handle_pending_exception(status);
  if (status == napi_pending_exception) {
    if (environment != nullptr) {
      environment->DecrementAsyncCallbackScopeDepth();
    } else {
      detached_callback_scope_depth--;
    }
    return status;
  } else if (status == napi_ok && callback_scope_depth == 1 && !skip_task_queues) {
    status = EdgeRunCallbackScopeCheckpoint(env);
    status = handle_pending_exception(status);
  }

  if (environment != nullptr) {
    environment->DecrementAsyncCallbackScopeDepth();
  } else {
    detached_callback_scope_depth--;
  }
  return status;
}

napi_status EdgeCallCallbackWithDomain(napi_env env,
                                      napi_value recv,
                                      napi_value callback,
                                      size_t argc,
                                      napi_value* argv,
                                      napi_value* result) {
  if (env == nullptr || recv == nullptr || callback == nullptr) {
    return napi_invalid_arg;
  }

  napi_value domain = nullptr;
  if (!GetCallbackDomain(env, recv, &domain) || domain == nullptr) {
    return napi_call_function(env, recv, callback, argc, argv, result);
  }

  napi_value helper = GetCachedDomainCallbackHelper(env);
  if (helper == nullptr) {
    return napi_generic_failure;
  }

  napi_value args_array = nullptr;
  if (napi_create_array_with_length(env, argc, &args_array) != napi_ok || args_array == nullptr) {
    return napi_generic_failure;
  }
  for (size_t i = 0; i < argc; ++i) {
    if (napi_set_element(env, args_array, static_cast<uint32_t>(i), argv[i]) != napi_ok) {
      return napi_generic_failure;
    }
  }

  napi_value global = internal_binding::GetGlobal(env);
  if (global == nullptr) return napi_generic_failure;

  napi_value helper_argv[4] = {domain, recv, callback, args_array};
  return napi_call_function(env, global, helper, 4, helper_argv, result);
}

napi_status EdgeMakeCallback(napi_env env,
                            napi_value recv,
                            napi_value callback,
                            size_t argc,
                            napi_value* argv,
                            napi_value* result) {
  return EdgeMakeCallbackWithFlags(env,
                                  recv,
                                  callback,
                                  argc,
                                  argv,
                                  result,
                                  kEdgeMakeCallbackNone);
}

napi_status EdgeRunCallbackScopeCheckpoint(napi_env env) {
  if (env == nullptr) {
    return napi_invalid_arg;
  }

  edge::HandleScope scope(env);
  if (!scope.is_open()) {
    return scope.status();
  }

  bool has_pending = false;
  if (napi_is_exception_pending(env, &has_pending) != napi_ok) {
    return napi_generic_failure;
  }
  if (has_pending) {
    return napi_pending_exception;
  }

  bool has_tick_scheduled = false;
  bool has_rejection_to_warn = false;
  const bool have_task_queue_flags =
      EdgeGetTaskQueueFlags(env, &has_tick_scheduled, &has_rejection_to_warn);

  // Match Node's InternalCallbackScope: when no nextTick or promise-rejection
  // work is pending, run a microtask checkpoint first and return early if no
  // task-queue work appeared as a result. Before task_queue is initialized,
  // fall back to running the microtask checkpoint only.
  if (!have_task_queue_flags || (!has_tick_scheduled && !has_rejection_to_warn)) {
    napi_status status = unofficial_napi_process_microtasks(env);
    if (status != napi_ok) {
      return status;
    }
    if (napi_is_exception_pending(env, &has_pending) != napi_ok) {
      return napi_generic_failure;
    }
    if (has_pending) {
      return napi_pending_exception;
    }
    if (!EdgeGetTaskQueueFlags(env, &has_tick_scheduled, &has_rejection_to_warn)) {
      return napi_ok;
    }
    if (!has_tick_scheduled && !has_rejection_to_warn) {
      return napi_ok;
    }
  }

  return DrainProcessTickCallback(env);
}

bool EdgeHandlePendingExceptionNow(napi_env env, bool* handled_out) {
  if (handled_out != nullptr) *handled_out = false;
  if (env == nullptr) return false;

  if (IsEnvironmentExitRequested(env) ||
      (!EdgeWorkerEnvOwnsProcessState(env) && EdgeWorkerEnvStopRequested(env))) {
    PendingExceptionInfo ignored = {};
    (void)TakePendingExceptionInfo(env, &ignored);
    (void)unofficial_napi_cancel_terminate_execution(env);
    if (handled_out != nullptr) *handled_out = true;
    return true;
  }

  PendingExceptionInfo pending = {};
  if (!TakePendingExceptionInfo(env, &pending) || !pending.has_exception || pending.exception == nullptr) {
    return false;
  }

  bool handled = false;
  napi_value effective_exception = pending.exception;
  std::string effective_exception_line = pending.exception_line;
  const bool dispatched = DispatchUncaughtException(
      env, pending.exception, &handled, &effective_exception, nullptr, &effective_exception_line);
  if (!dispatched || !handled) {
    bool has_pending = false;
    if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
      if (handled_out != nullptr) *handled_out = false;
      return true;
    }
    (void)napi_throw(env, effective_exception != nullptr ? effective_exception : pending.exception);
    if (handled_out != nullptr) *handled_out = false;
    return true;
  }

  if (handled_out != nullptr) *handled_out = true;
  return true;
}

bool EdgeFinalizeFatalExceptionNow(napi_env env,
                                   napi_value exception,
                                   int default_exit_code,
                                   const std::string& exception_line,
                                   const std::string& thrown_at) {
  if (env == nullptr || exception == nullptr) return false;

  const bool abort_on_uncaught = ShouldAbortOnUncaughtException();
  if (abort_on_uncaught && !HasActiveDomainErrorHandler(env)) {
    AbortOnUncaughtException(env, exception, exception_line, thrown_at);
  }

  std::string fatal_error;
  const int exit_code =
      FinalizeFatalException(
          env, exception, &fatal_error, default_exit_code, exception_line, thrown_at);
  if (!fatal_error.empty()) {
    if (fatal_error.back() != '\n') fatal_error.push_back('\n');
    WriteTextToFd(2, fatal_error);
  }

  if (EdgeWorkerEnvIsMainThread(env) && !EdgeWorkerEnvIsInternalThread(env)) {
    std::_Exit(exit_code);
  }

  EdgeWorkerEnvRequestStop(env);
  if (uv_loop_t* loop = EdgeGetEnvLoop(env); loop != nullptr) {
    uv_stop(loop);
  }
  return true;
}

napi_status EdgeInstallConsole(napi_env env) {
  if (env == nullptr) {
    return napi_invalid_arg;
  }
  napi_value global = nullptr;
  napi_status status = napi_get_global(env, &global);
  if (status != napi_ok || global == nullptr) {
    return (status == napi_ok) ? napi_generic_failure : status;
  }

  napi_value existing_console = nullptr;
  if (GetNamedProperty(env, global, "console", &existing_console) && existing_console != nullptr) {
    napi_valuetype console_type = napi_undefined;
    if (napi_typeof(env, existing_console, &console_type) == napi_ok &&
        (console_type == napi_object || console_type == napi_function)) {
      napi_value existing_log = nullptr;
      if (GetNamedProperty(env, existing_console, "log", &existing_log) && IsFunction(env, existing_log)) {
        return napi_ok;
      }
    }
  }

  napi_value console_module = nullptr;
  if (RequireModule(env, "console", &console_module) && console_module != nullptr) {
    status = napi_set_named_property(env, global, "console", console_module);
    if (status == napi_ok) return napi_ok;
  }

  // Fallback for when no JS console builtin could be loaded.
  bool has_pending = false;
  if (napi_is_exception_pending(env, &has_pending) == napi_ok && has_pending) {
    napi_value exc = nullptr;
    napi_get_and_clear_last_exception(env, &exc);
  }

  napi_value console_obj = nullptr;
  if (napi_create_object(env, &console_obj) != napi_ok || console_obj == nullptr) {
    return napi_generic_failure;
  }
  napi_value log_fn = nullptr;
  if (napi_create_function(env, "log", NAPI_AUTO_LENGTH, ConsoleLogCallback, nullptr, &log_fn) != napi_ok ||
      log_fn == nullptr) {
    return napi_generic_failure;
  }
  if (napi_set_named_property(env, console_obj, "log", log_fn) != napi_ok ||
      napi_set_named_property(env, console_obj, "info", log_fn) != napi_ok ||
      napi_set_named_property(env, console_obj, "debug", log_fn) != napi_ok) {
    return napi_generic_failure;
  }
  napi_value err_fn = nullptr;
  if (napi_create_function(env, "error", NAPI_AUTO_LENGTH, ConsoleErrorCallback, nullptr, &err_fn) != napi_ok ||
      err_fn == nullptr) {
    return napi_generic_failure;
  }
  if (napi_set_named_property(env, console_obj, "error", err_fn) != napi_ok ||
      napi_set_named_property(env, console_obj, "warn", err_fn) != napi_ok) {
    return napi_generic_failure;
  }
  return napi_set_named_property(env, global, "console", console_obj);
}

int EdgeRunScriptSourceWithLoop(napi_env env,
                               const char* source_text,
                               std::string* error_out,
                               bool keep_event_loop_alive,
                               const char* native_main_builtin_id) {
  if (error_out != nullptr) {
    error_out->clear();
  }
  return RunScriptWithGlobals(
      env,
      source_text,
      nullptr,
      native_main_builtin_id,
      error_out,
      keep_event_loop_alive,
      EdgeBootstrapMode::kMainThread);
}

int EdgeRunScriptSource(napi_env env, const char* source_text, std::string* error_out) {
  return EdgeRunScriptSourceWithLoop(env, source_text, error_out, false);
}

int EdgeRunScriptFileWithLoop(napi_env env,
                               const char* script_path,
                               std::string* error_out,
                               bool keep_event_loop_alive,
                               const char* native_main_builtin_id) {
  if (error_out != nullptr) {
    error_out->clear();
  }
  const std::string normalized_script_path =
      script_path != nullptr ? edge_path::NormalizeFileURLOrPath(script_path) : std::string();
  const char* fs_script_path = normalized_script_path.empty() ? script_path : normalized_script_path.c_str();
  std::string source = ";";
  g_edge_current_script_path = script_path;
  std::string restore_cwd;
#if !defined(_WIN32)
  {
    const char* node_test_dir = std::getenv("NODE_TEST_DIR");
    if (node_test_dir != nullptr && fs_script_path != nullptr) {
      const std::string script_path_s(fs_script_path);
      const std::string test_dir_s(node_test_dir);
      if (!test_dir_s.empty() && script_path_s.rfind(test_dir_s, 0) == 0) {
        char cwd_buf[4096] = {'\0'};
        if (getcwd(cwd_buf, sizeof(cwd_buf)) != nullptr) {
          restore_cwd = cwd_buf;
        }
        const std::size_t pos = test_dir_s.find_last_of('/');
        if (pos != std::string::npos) {
          const std::string node_root = test_dir_s.substr(0, pos);
          chdir(node_root.c_str());
        }
      }
    }
    if (restore_cwd.empty() && fs_script_path != nullptr) {
      const std::string script_path_s(fs_script_path);
      constexpr std::string_view kVendoredTestAlias = "/.workspace/test/";
      constexpr std::string_view kVendoredWorkspaceDir = "/.workspace";
      const std::size_t alias_pos = script_path_s.find(kVendoredTestAlias);
      if (alias_pos != std::string::npos) {
        char cwd_buf[4096] = {'\0'};
        if (getcwd(cwd_buf, sizeof(cwd_buf)) != nullptr) {
          const std::string cwd(cwd_buf);
          const std::string alias_root =
              script_path_s.substr(0, alias_pos + kVendoredWorkspaceDir.size());
          if (cwd == alias_root) {
            restore_cwd = cwd;
            const std::string inferred_test_dir = script_path_s.substr(0, alias_pos);
            if (!inferred_test_dir.empty()) {
              chdir(inferred_test_dir.c_str());
            }
          }
        }
      }
    }
  }
#endif
  const int rc =
      RunScriptWithGlobals(
          env,
          source.c_str(),
          script_path,
          native_main_builtin_id,
          error_out,
          keep_event_loop_alive,
          EdgeBootstrapMode::kMainThread);
#if !defined(_WIN32)
  if (!restore_cwd.empty()) {
    chdir(restore_cwd.c_str());
  }
#endif
  g_edge_current_script_path.clear();
  return rc;
}

int EdgeRunScriptFile(napi_env env, const char* script_path, std::string* error_out) {
  return EdgeRunScriptFileWithLoop(env, script_path, error_out, false);
}

int EdgeRunWorkerThreadMain(napi_env env,
                           const std::vector<std::string>& exec_argv,
                           std::string* error_out) {
  if (error_out != nullptr) {
    error_out->clear();
  }

  g_edge_current_script_path.clear();
  g_edge_exec_argv = exec_argv;
  g_edge_effective_exec_argv = exec_argv;
  g_edge_process_title.clear();
  g_edge_script_argv.clear();

  return RunScriptWithGlobals(
      env,
      ";",
      nullptr,
      "internal/main/worker_thread",
      error_out,
      true,
      EdgeBootstrapMode::kWorkerThread);
}

bool EdgeInitializeOpenSslForExecArgv(const std::vector<std::string>& exec_argv,
                                      bool validate_csprng,
                                      std::string* error_out) {
  if (!ConfigureOpenSslFromExecArgv(exec_argv, error_out)) {
    return false;
  }
  if (!validate_csprng) {
    return true;
  }
  return EdgeValidateOpenSslCsprng();
}

bool EdgeValidateOpenSslCsprng() {
  // Match Node's startup behavior closely enough to fail fast when the loaded
  // provider configuration leaves no usable CSPRNG implementation.
  if (!ncrypto::CSPRNG(nullptr, 0)) {
    std::abort();
  }
  return true;
}

void EdgeSetCurrentScriptPath(const std::string& script_path) {
  g_edge_current_script_path = script_path;
}

void EdgeSetScriptArgv(const std::vector<std::string>& script_argv) {
  g_edge_script_argv = script_argv;
}

void EdgeSetExecArgv(const std::vector<std::string>& exec_argv) {
  g_edge_cli_exec_argv = exec_argv;
}
