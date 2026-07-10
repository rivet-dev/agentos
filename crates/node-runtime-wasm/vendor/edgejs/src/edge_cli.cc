#include "edge_cli.h"

#include "edge_trace.h"

#include <cctype>
#include <csignal>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <functional>
#include <iostream>
#include <mutex>
#include <optional>
#include <regex>
#include <sstream>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

#if defined(_WIN32)
#include <io.h>
#else
#include <unistd.h>
#endif

#include <uv.h>

#include "node_version.h"
#include "unofficial_napi.h"
#include "edge_environment_runtime.h"
#include "edge_env_loop.h"
#include "edge_handle_wrap.h"
#include "edge_napi_embedder_hooks.h"
#include "edge_option_helpers.h"
#include "edge_compat_exec.h"
#include "edge_process.h"
#include "edge_stream_base.h"
#include "edge_timers_host.h"
#include "edge_runtime_platform.h"
#include "edge_builtin_bytecode.h"
#include "edge_bytecode_cache.h"
#include "edge_precompile.h"
#include "edge_runtime.h"
#include "edge_worker_control.h"
#include "edge_worker_env.h"

#if !defined(_WIN32)
namespace node {
void InitializeStdio();
void RegisterSignalHandler(int signal,
                           void (*handler)(int signal, siginfo_t* info, void* ucontext),
                           bool reset_handler = false);
void SignalExit(int signal, siginfo_t* info, void* ucontext);
}  // namespace node
#endif

namespace {

constexpr const char kUsage[] = "Usage: edge <script.js>";
constexpr const char kNativeHelpText[] =
    "Usage: edge [options] [ script.js ] [arguments]\n"
    "\n"
    "Options:\n"
    "  -e, --eval=...              evaluate script\n"
    "  -p, --print[=...]           evaluate script and print result\n"
    "  -c, --check                 syntax check script without executing\n"
    "  -i, --interactive           always enter the REPL\n"
    "  --run <command>             run a package script command\n"
    "  --watch                     run in watch mode\n"
    "  --test                      run tests\n"
    "  --precompile <path>...      compile .js/.cjs files to bytecode sidecars\n"
    "  --bytecode-cache            enable per-file bytecode sidecars (off by default)\n"
    "  --no-bytecode-cache         disable all bytecode caching (incl. builtins)\n"
    "  --completion-bash           print source-able bash completion script\n"
    "  -v, --version               print Edge.js / Node.js version\n"
    "  -h, --help                  print this help\n";
constexpr unsigned kMaxSignal = 32;
std::once_flag g_cli_init_once;
#if defined(_WIN32)
constexpr const char kPathEnvSeparator[] = ";";
#else
constexpr const char kPathEnvSeparator[] = ":";
#endif

bool EdgeIsTruthyEnvVar(const char* value) {
  if (value == nullptr || value[0] == '\0') return false;

  std::string normalized(value);
  for (char& ch : normalized) {
    ch = static_cast<char>(std::tolower(static_cast<unsigned char>(ch)));
  }

  return normalized != "0" &&
         normalized != "false" &&
         normalized != "no" &&
         normalized != "off";
}

#if defined(ENABLE_TRACING)
class CliStartupTracer {
 public:
  CliStartupTracer()
      : enabled_(TracingEnabled()),
        last_ns_(enabled_ ? uv_hrtime() : 0) {}

  bool enabled() const { return enabled_; }

  void Mark(const char* phase) {
    if (!enabled_ || phase == nullptr || phase[0] == '\0') return;
    const uint64_t now = uv_hrtime();
    const double delta_ms = static_cast<double>(now - last_ns_) / 1e6;
    const double total_ms =
        static_cast<double>(now - EdgeGetProcessStartTimeNanoseconds()) / 1e6;
    std::cerr << "{\"edge_startup_trace\":\"" << phase << "\",\"delta_ms\":"
              << delta_ms << ",\"total_ms\":" << total_ms << "}\n";
    last_ns_ = now;
  }

 private:
  static bool TracingEnabled() {
    static const bool enabled =
        EdgeIsTruthyEnvVar(std::getenv("EDGE_STARTUP_TRACE"));
    return enabled;
  }

  bool enabled_;
  uint64_t last_ns_;
};

void CliStartupTraceForward(void* data, const char* phase) {
  auto* tracer = static_cast<CliStartupTracer*>(data);
  if (tracer != nullptr) tracer->Mark(phase);
}
#define EDGE_STARTUP_TRACE(tracer, phase) (tracer).Mark((phase))
#else
struct CliStartupTracer {};
#define EDGE_STARTUP_TRACE(tracer, phase) ((void)0)
#endif

struct RuntimeInitOptions {
  const std::vector<std::string>* raw_exec_argv = nullptr;
  const std::vector<std::string>* effective_exec_argv = nullptr;
  bool initialize_openssl = true;
  bool validate_openssl_csprng = true;
};

void ResetSignalHandlersLikeNode() {
#if !defined(_WIN32)
  struct sigaction act;
  std::memset(&act, 0, sizeof(act));

  for (unsigned nr = 1; nr < kMaxSignal; nr += 1) {
    if (nr == SIGKILL || nr == SIGSTOP) continue;

    bool ignore_signal = false;
#if defined(SIGPIPE)
    ignore_signal = ignore_signal || nr == SIGPIPE;
#endif
#if defined(SIGXFSZ)
    ignore_signal = ignore_signal || nr == SIGXFSZ;
#endif
    act.sa_handler = ignore_signal ? SIG_IGN : SIG_DFL;

    if (act.sa_handler == SIG_DFL) {
      struct sigaction old;
      if (sigaction(static_cast<int>(nr), nullptr, &old) != 0) continue;
#if defined(SA_SIGINFO)
      if ((old.sa_flags & SA_SIGINFO) || old.sa_handler != SIG_IGN) continue;
#else
      if (old.sa_handler != SIG_IGN) continue;
#endif
    }

    (void)sigaction(static_cast<int>(nr), &act, nullptr);
  }
#endif
}

int RunWithFreshEnv(const std::function<int(napi_env)>& runner,
                   std::string* error_out,
                   const RuntimeInitOptions& options = {}) {
  CliStartupTracer startup_trace;
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.begin");
  if (options.initialize_openssl) {
    const std::vector<std::string>* effective_exec_argv = options.effective_exec_argv;
    edge_options::EffectiveCliState derived_effective_state;
    if (effective_exec_argv == nullptr) {
      EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.build-effective-state.begin");
      derived_effective_state = edge_options::BuildEffectiveCliState(
          options.raw_exec_argv != nullptr ? *options.raw_exec_argv
                                           : std::vector<std::string>{});
      EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.build-effective-state.end");
      effective_exec_argv =
          (derived_effective_state.ok ? &derived_effective_state.effective_tokens
                                      : options.raw_exec_argv);
    } else {
      EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.use-precomputed-effective-state");
    }

    if (effective_exec_argv == nullptr) {
      if (error_out != nullptr) {
        *error_out = "Missing exec argv for OpenSSL initialization";
      }
      return 1;
    }

    EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.configure.begin");
    if (!EdgeInitializeOpenSslForExecArgv(
            *effective_exec_argv,
            false,
            error_out)) {
      return 1;
    }
    EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.configure.end");

    if (options.validate_openssl_csprng) {
      EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.csprng-check.begin");
      EdgeValidateOpenSslCsprng();
      EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.csprng-check.end");
    }
    EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl-init");
  } else {
    EDGE_STARTUP_TRACE(startup_trace, "cli.env.openssl.skip");
  }

  EdgeInstallNapiEmbedderHooks();
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.install-napi-hooks");

  napi_env env = nullptr;
  void* env_scope = nullptr;
  const napi_status create_status = unofficial_napi_create_env(8, &env, &env_scope);
  if (create_status != napi_ok || env == nullptr || env_scope == nullptr) {
    if (error_out != nullptr) {
      *error_out = "Failed to initialize runtime environment";
    }
    return 1;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.create-napi-env");

  if (!EdgeAttachEnvironmentForRuntime(env, nullptr
#if defined(ENABLE_TRACING)
                                       ,
                                       CliStartupTraceForward,
                                       &startup_trace
#endif
                                       )) {
    (void)unofficial_napi_release_env(env_scope);
    if (error_out != nullptr) {
      *error_out = "Failed to attach runtime environment";
    }
    return 1;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.attach-runtime");

  if (EdgeRuntimePlatformInstallHooks(env) != napi_ok) {
    (void)unofficial_napi_release_env(env_scope);
    if (error_out != nullptr) {
      *error_out = "Failed to attach runtime platform hooks";
    }
    return 1;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.install-runtime-hooks");

  const int exit_code = runner(env);
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.runner-returned");
  if (EDGE_TRACE_ENABLED("EDGE_TRACE_BOOTSTRAP")) {
    std::fprintf(stderr,
                 "EDGE_TRACE_BOOTSTRAP runner exit_code=%d error=%s\n",
                 exit_code,
                 (error_out != nullptr && !error_out->empty()) ? error_out->c_str() : "<none>");
  }
  EdgeEnvironmentRunCleanup(env);
  EdgeEnvironmentRunAtExitCallbacks(env);
  edge_builtin_bytecode::FlushIfDirty();
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.cleanup");
  const napi_status release_status = unofficial_napi_release_env(env_scope);
  if (release_status != napi_ok) {
    if (error_out != nullptr) {
      *error_out = "Failed to release runtime environment";
    }
    return 1;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.env.release-napi-env");
  return exit_code;
}

std::string ResolveCliScriptPath(const char* script_path) {
  if (script_path == nullptr || script_path[0] == '\0') {
    return "";
  }
  auto resolve_candidate = [](const std::filesystem::path& candidate) -> std::string {
    static constexpr const char* kExtensions[] = {
        "", ".js", ".json", ".node", "/index.js", "/index.json", "/index.node",
    };

    std::error_code ec;
    for (const char* suffix : kExtensions) {
      const std::filesystem::path resolved = candidate.string() + suffix;
      if (std::filesystem::exists(resolved, ec) && !ec) {
        return resolved.string();
      }
      ec.clear();
    }
    return "";
  };

  const std::filesystem::path direct(script_path);
  if (const std::string resolved = resolve_candidate(direct); !resolved.empty()) {
    return resolved;
  }

  // Allow running `./build/edge examples/foo.js` from repo root.
  const std::filesystem::path repo_fallback = std::filesystem::path("edge") / direct;
  if (const std::string resolved = resolve_candidate(repo_fallback); !resolved.empty()) {
    return resolved;
  }
  return direct.string();
}

std::string CliErrorPrefix() {
  std::string exec_path = EdgeGetProcessExecPath();
  if (exec_path.empty()) exec_path = "edge";
  return exec_path;
}

std::string FormatCliError(const std::string& message) {
  return CliErrorPrefix() + ": " + message;
}

std::string GetBashCompletion() {
  return R"(_node_complete() {
  local cur_word options
  cur_word="${COMP_WORDS[COMP_CWORD]}"
  if [[ "${cur_word}" == -* ]] ; then
    COMPREPLY=( $(compgen -W '' -- "${cur_word}") )
    return 0
  else
    COMPREPLY=( $(compgen -f "${cur_word}") )
    return 0
  fi
}
complete -o filenames -o nospace -o bashdefault -F _node_complete node node_g
)";
}

bool ApplyEnvUpdate(const std::string& key, const std::string& value) {
#if defined(_WIN32)
  return _putenv_s(key.c_str(), value.c_str()) == 0;
#else
  return setenv(key.c_str(), value.c_str(), 1) == 0;
#endif
}

void ApplyEnvUpdates(const std::unordered_map<std::string, std::string>& updates) {
  for (const auto& [key, value] : updates) {
    (void)ApplyEnvUpdate(key, value);
  }
}

bool IsBooleanOptionEnabled(const std::vector<std::string>& tokens, const char* option) {
  const std::string prefix = std::string(option) + "=";
  for (const auto& token : tokens) {
    if (token == option) return true;
    if (token.rfind(prefix, 0) != 0) continue;
    const std::string value = token.substr(prefix.size());
    if (value != "false" && value != "0") return true;
  }
  return false;
}

// --bytecode-cache / --no-bytecode-cache, last occurrence wins. Returns
// -1 (disable everything), +1 (opt user sidecars in), or 0 (neither given).
int BytecodeCacheCliOverride(const std::vector<std::string>& tokens) {
  int override_state = 0;
  for (const auto& token : tokens) {
    if (token == "--no-bytecode-cache") {
      override_state = -1;
    } else if (token == "--bytecode-cache") {
      override_state = 1;
    }
  }
  return override_state;
}

bool TokenHasInlineValue(const std::string& token) {
  return token.find('=') != std::string::npos;
}

bool IsSupportedV8ProfilerFlag(const std::string& token) {
  return token == "--prof" || token.rfind("--logfile=", 0) == 0 ||
         token.rfind("--prof-sampling-interval=", 0) == 0;
}

void ApplySupportedV8Flags(const std::vector<std::string>& raw_exec_argv) {
  std::string flags;
  bool has_js_source_phase_imports = false;
  bool has_no_js_source_phase_imports = false;
  bool has_import_attributes = false;
  bool has_no_import_attributes = false;
  for (const auto& token : raw_exec_argv) {
    if (IsSupportedV8ProfilerFlag(token)) {
      if (!flags.empty()) flags.push_back(' ');
      flags += token;
    }
    has_js_source_phase_imports = has_js_source_phase_imports || token == "--js-source-phase-imports";
    has_no_js_source_phase_imports = has_no_js_source_phase_imports || token == "--no-js-source-phase-imports";
    has_import_attributes = has_import_attributes || token == "--harmony-import-attributes";
    has_no_import_attributes = has_no_import_attributes || token == "--no-harmony-import-attributes";
  }
  if (!has_js_source_phase_imports && !has_no_js_source_phase_imports) {
    if (!flags.empty()) flags.push_back(' ');
    flags += "--js-source-phase-imports";
  }
  if (!has_import_attributes && !has_no_import_attributes) {
    if (!flags.empty()) flags.push_back(' ');
    flags += "--harmony-import-attributes";
  }
  if (!flags.empty()) {
    (void)unofficial_napi_set_flags_from_string(flags.c_str(), flags.size());
  }
}

bool OptionConsumesNextToken(const std::string& token) {
  static const std::unordered_set<std::string> kValueOptions = {
      "-e",
      "--eval",
      "-pe",
      "-ep",
      "-r",
      "--require",
      "--input-type",
      "--inspect-port",
      "--debug-port",
      "--stack-trace-limit",
      "--secure-heap",
      "--secure-heap-min",
      "--disable-warning",
      "--env-file",
      "--env-file-if-exists",
      "--experimental-config-file",
      "--experimental-loader",
      "--loader",
      "--import",
      "--conditions",
      "--trace-event-categories",
      "--trace-event-file-pattern",
      "--run",
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
      "--test-isolation",
      "--experimental-test-isolation",
      "--allow-fs-read",
      "--allow-fs-write",
      "--watch-kill-signal",
      "--watch-path",
  };
  return kValueOptions.find(token) != kValueOptions.end();
}

bool TokenRequiresNonEmptyInlineValue(const std::string& token) {
  static const std::unordered_set<std::string> kRequireValue = {
      "--debug-port=",
      "--inspect-port=",
  };
  for (const auto& prefix : kRequireValue) {
    if (token == prefix) return true;
  }
  return false;
}

bool IsBooleanOptionForNegation(const std::string& option) {
  static const std::unordered_set<std::string> kBooleanOptions = {
      "--abort-on-uncaught-exception",
      "--allow-addons",
      "--allow-child-process",
      "--allow-inspector",
      "--allow-wasi",
      "--allow-worker",
      "--async-context-frame",
      "--bytecode-cache",
      "--check",
      "--enable-source-maps",
      "--entry-url",
      "--experimental-addon-modules",
      "--experimental-detect-module",
      "--experimental-eventsource",
      "--experimental-fetch",
      "--experimental-global-customevent",
      "--experimental-global-webcrypto",
      "--experimental-import-meta-resolve",
      "--experimental-inspector-network-resource",
      "--experimental-network-inspection",
      "--experimental-print-required-tla",
      "--experimental-quic",
      "--experimental-repl-await",
      "--experimental-require-module",
      "--experimental-report",
      "--experimental-strip-types",
      "--experimental-sqlite",
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
      "--network-family-autoselection",
      "--no-addons",
      "--no-deprecation",
      "--no-experimental-global-navigator",
      "--no-experimental-websocket",
      "--no-node-snapshot",
      "--no-verify-base-objects",
      "--node-snapshot",
      "--openssl-legacy-provider",
      "--openssl-shared-config",
      "--pending-deprecation",
      "--permission",
      "--precompile",
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
      "--warnings",
      "--watch",
      "--watch-preserve-output",
  };
  return kBooleanOptions.find(option) != kBooleanOptions.end();
}

bool IsKnownNonBooleanOption(const std::string& option) {
  static const std::unordered_set<std::string> kNonBooleanOptions = {
      "--allow-fs-read",
      "--allow-fs-write",
      "--debug-port",
      "--diagnostic-dir",
      "--disable-warning",
      "--dns-result-order",
      "--env-file",
      "--env-file-if-exists",
      "--es-module-specifier-resolution",
      "--eval",
      "--experimental-config-file",
      "--heapsnapshot-signal",
      "--icu-data-dir",
      "--input-type",
      "--inspect-port",
      "--localstorage-file",
      "--max-http-header-size",
      "--openssl-config",
      "--redirect-warnings",
      "--require",
      "--run",
      "--secure-heap",
      "--secure-heap-min",
      "--stack-trace-limit",
      "--experimental-test-isolation",
      "--test-global-setup",
      "--test-isolation",
      "--test-rerun-failures",
      "--test-shard",
      "--tls-cipher-list",
      "--tls-keylog",
      "--trace-require-module",
      "--trace-event-categories",
      "--trace-event-file-pattern",
      "--unhandled-rejections",
      "--watch-kill-signal",
  };
  return kNonBooleanOptions.find(option) != kNonBooleanOptions.end();
}

bool ValidateNegatedOption(const std::string& token, std::string* error_out) {
  if (token.rfind("--no-", 0) != 0) return true;
  if (IsBooleanOptionForNegation(token)) return true;
  const std::string normalized = "--" + token.substr(5);
  if (IsBooleanOptionForNegation(normalized)) return true;
  if (IsKnownNonBooleanOption(normalized)) {
    if (error_out != nullptr) {
      *error_out = FormatCliError(token + " is an invalid negation because it is not a boolean option");
    }
    return false;
  }
  if (error_out != nullptr) {
    *error_out = "bad option: " + token;
  }
  return false;
}

bool HasDisallowedNodeOption(const std::string& token) {
  static const std::unordered_set<std::string> kDisallowedExact = {
      "--",
      "--check",
      "--eval",
      "--expose-internals",
      "--expose_internals",
      "--help",
      "--interactive",
      "--precompile",
      "--print",
      "--test",
      "--v8-options",
      "--version",
      "-c",
      "-e",
      "-h",
      "-i",
      "-p",
      "-pe",
      "-v",
  };
  const size_t eq = token.find('=');
  const std::string key = eq == std::string::npos ? token : token.substr(0, eq);
  return kDisallowedExact.find(key) != kDisallowedExact.end();
}

bool IsRecognizedCliOptionToken(const std::string& token) {
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
  if (TokenHasInlineValue(token)) {
    const std::string key = token.substr(0, token.find('='));
    return OptionConsumesNextToken(key) ||
           IsBooleanOptionForNegation(key) ||
           key == "--env-file" ||
           key == "--env-file-if-exists" ||
           key == "--experimental-config-file" ||
           key == "--input-type" ||
           key == "--trace-require-module" ||
           key == "--trace-event-categories" ||
           key == "--trace-event-file-pattern";
  }
  if (OptionConsumesNextToken(token)) return true;
  if (token == "--experimental-strip-types") return true;
  if (IsBooleanOptionForNegation(token)) return true;
  if (token.rfind("--no-", 0) == 0) return true;
  if (token.rfind("--env-file=", 0) == 0 ||
      token.rfind("--env-file-if-exists=", 0) == 0 ||
      token.rfind("--experimental-config-file=", 0) == 0 ||
      token.rfind("--input-type=", 0) == 0 ||
      token.rfind("--trace-require-module=", 0) == 0 ||
      token.rfind("--trace-event-categories=", 0) == 0 ||
      token.rfind("--trace-event-file-pattern=", 0) == 0) {
    return true;
  }
  return false;
}

bool ValidateNodeOptions(const std::vector<std::string>& node_options_tokens, std::string* error_out) {
  for (const auto& token : node_options_tokens) {
    if (HasDisallowedNodeOption(token)) {
      if (error_out != nullptr) {
        *error_out = FormatCliError(token + " is not allowed in NODE_OPTIONS");
      }
      return false;
    }
  }
  return true;
}

bool HasExactOptionToken(const std::vector<std::string>& tokens, const char* option) {
  for (const auto& token : tokens) {
    if (token == option) return true;
  }
  return false;
}

bool HasOptionTokenWithInlineValue(const std::vector<std::string>& tokens, const char* option) {
  const std::string prefix = std::string(option) + "=";
  for (const auto& token : tokens) {
    if (token == option || token.rfind(prefix, 0) == 0) return true;
  }
  return false;
}

bool ValidateCaOptions(const edge_options::EffectiveCliState& state, std::string* error_out) {
  const bool use_openssl_ca = HasExactOptionToken(state.effective_tokens, "--use-openssl-ca");
  const bool use_bundled_ca = HasExactOptionToken(state.effective_tokens, "--use-bundled-ca");
  if (use_openssl_ca && use_bundled_ca) {
    if (error_out != nullptr) {
      *error_out = FormatCliError("either --use-openssl-ca or --use-bundled-ca can be used, not both");
    }
    return false;
  }
  return true;
}

bool ValidateTlsProtocolBoundsOptions(const edge_options::EffectiveCliState& state, std::string* error_out) {
  const bool min_v13 = HasExactOptionToken(state.effective_tokens, "--tls-min-v1.3");
  const bool max_v12 = HasExactOptionToken(state.effective_tokens, "--tls-max-v1.2");
  if (min_v13 && max_v12) {
    if (error_out != nullptr) {
      *error_out = FormatCliError("either --tls-min-v1.3 or --tls-max-v1.2 can be used, not both");
    }
    return false;
  }
  return true;
}

bool ValidateTraceRequireModuleOption(const edge_options::EffectiveCliState& state,
                                      std::string* error_out) {
  for (const auto& token : state.effective_tokens) {
    constexpr std::string_view kPrefix = "--trace-require-module=";
    if (token.rfind(kPrefix, 0) != 0) continue;
    const std::string value = token.substr(kPrefix.size());
    if (value == "all" || value == "no-node-modules") {
      return true;
    }
    if (error_out != nullptr) {
      *error_out = "invalid value for --trace-require-module";
    }
    return false;
  }
  return true;
}

bool ValidateUnhandledRejectionsOption(const edge_options::EffectiveCliState& state,
                                       std::string* error_out) {
  constexpr std::string_view kPrefix = "--unhandled-rejections=";
  for (size_t i = 0; i < state.effective_tokens.size(); ++i) {
    const std::string& token = state.effective_tokens[i];
    std::string value;
    if (token == "--unhandled-rejections") {
      if (i + 1 >= state.effective_tokens.size()) break;
      value = state.effective_tokens[i + 1];
      i++;
    } else if (token.rfind(kPrefix, 0) == 0) {
      value = token.substr(kPrefix.size());
    } else {
      continue;
    }

    if (value == "none" || value == "strict" || value == "throw" ||
        value == "warn" || value == "warn-with-error-code") {
      continue;
    }
    if (error_out != nullptr) {
      *error_out = "invalid value for --unhandled-rejections";
    }
    return false;
  }
  return true;
}

bool GetSecurityRevertTokenValue(const std::vector<std::string>& tokens, std::string* value_out) {
  if (value_out != nullptr) value_out->clear();
  constexpr std::string_view kInlinePrefix = "--security-revert=";
  for (size_t i = 0; i < tokens.size(); ++i) {
    const std::string& token = tokens[i];
    if (token == "--security-reverts" || token == "--security-revert") {
      if (i + 1 < tokens.size() && value_out != nullptr) {
        *value_out = tokens[i + 1];
      }
      return i + 1 < tokens.size();
    }
    if (token.rfind(kInlinePrefix, 0) == 0) {
      if (value_out != nullptr) {
        *value_out = token.substr(kInlinePrefix.size());
      }
      return true;
    }
  }
  return false;
}

bool RawExecArgvHasInputType(const std::vector<std::string>& raw_exec_argv) {
  for (const auto& token : raw_exec_argv) {
    if (token == "--input-type" || token.rfind("--input-type=", 0) == 0) {
      return true;
    }
  }
  return false;
}

bool IsPermissionFlagToken(const std::string& token) {
  const size_t eq = token.find('=');
  const std::string key = eq == std::string::npos ? token : token.substr(0, eq);
  return key == "--permission" ||
         key == "--allow-fs-read" ||
         key == "--allow-fs-write" ||
         key == "--allow-addons" ||
         key == "--allow-child-process" ||
         key == "--allow-inspector" ||
         key == "--allow-worker" ||
         key == "--allow-wasi";
}

bool AreProcessWarningsSuppressed() {
  const char* value = std::getenv("NODE_NO_WARNINGS");
  if (value == nullptr || value[0] == '\0') return false;
  return std::strcmp(value, "0") != 0;
}

void WarnUnsupportedPermissionsIfNeeded(const edge_options::EffectiveCliState& state) {
  if (AreProcessWarningsSuppressed()) return;
  for (const auto& token : state.effective_tokens) {
    if (!IsPermissionFlagToken(token)) continue;
    std::cerr << "Warning: permissions are not supported in Edge; ignoring permission flags.\n";
    return;
  }
}

bool StringEqualsPathVariableName(const std::string& name) {
#if defined(_WIN32)
  return name.size() == 4 &&
         std::tolower(static_cast<unsigned char>(name[0])) == 'p' &&
         std::tolower(static_cast<unsigned char>(name[1])) == 'a' &&
         std::tolower(static_cast<unsigned char>(name[2])) == 't' &&
         std::tolower(static_cast<unsigned char>(name[3])) == 'h';
#else
  return name == "PATH";
#endif
}

bool StringEqualsNoCase(std::string_view lhs, std::string_view rhs) {
  if (lhs.size() != rhs.size()) return false;
  for (size_t i = 0; i < lhs.size(); ++i) {
    if (std::tolower(static_cast<unsigned char>(lhs[i])) !=
        std::tolower(static_cast<unsigned char>(rhs[i]))) {
      return false;
    }
  }
  return true;
}

std::string EscapeShellArgument(std::string_view input) {
  if (input.empty()) {
#if defined(_WIN32)
    return "\"\"";
#else
    return "''";
#endif
  }

  static constexpr std::string_view kForbiddenCharacters =
      "[\t\n\r \"#$&'()*;<>?\\\\`|~]";
  if (input.find_first_of(kForbiddenCharacters) == std::string::npos) {
    return std::string(input);
  }

  static const std::regex kLeadingQuotePairs("^(?:'')+(?!$)");
#if defined(_WIN32)
  std::string escaped =
      std::regex_replace(std::string(input), std::regex("\""), "\"\"");
  escaped = "\"" + escaped + "\"";
  static const std::regex kTripleSingleQuote("\\\\\"\"\"");
  escaped = std::regex_replace(escaped, kLeadingQuotePairs, "");
  escaped = std::regex_replace(escaped, kTripleSingleQuote, "\\\"");
#else
  std::string escaped =
      std::regex_replace(std::string(input), std::regex("'"), "\\'");
  escaped = "'" + escaped + "'";
  static const std::regex kTripleSingleQuote("\\\\'''");
  escaped = std::regex_replace(escaped, kLeadingQuotePairs, "");
  escaped = std::regex_replace(escaped, kTripleSingleQuote, "\\'");
#endif
  return escaped;
}

struct CliPackageJsonInfo {
  std::filesystem::path package_json_path;
  std::string raw_json;
  std::string path_env_prefix;
};

std::optional<CliPackageJsonInfo> FindCliPackageJson(const std::filesystem::path& cwd) {
  namespace fs = std::filesystem;
  CliPackageJsonInfo info;
  bool found_package_json = false;

  for (fs::path directory_path = cwd;; directory_path = directory_path.parent_path()) {
    std::error_code ec;
    const fs::path node_modules_bin = directory_path / "node_modules" / ".bin";
    if (fs::is_directory(node_modules_bin, ec) && !ec) {
      info.path_env_prefix += node_modules_bin.string();
      info.path_env_prefix += kPathEnvSeparator;
    }

    if (!found_package_json) {
      const fs::path package_json_path = directory_path / "package.json";
      ec.clear();
      if (fs::is_regular_file(package_json_path, ec) && !ec) {
        std::ifstream input(package_json_path, std::ios::in | std::ios::binary);
        std::ostringstream buffer;
        buffer << input.rdbuf();
        info.package_json_path = package_json_path;
        info.raw_json = buffer.str();
        found_package_json = true;
      }
    }

    if (directory_path == directory_path.root_path() ||
        directory_path == directory_path.parent_path()) {
      break;
    }
  }

  if (!found_package_json) {
    return std::nullopt;
  }
  return info;
}

class CliPackageScriptRunner {
 public:
  CliPackageScriptRunner(const std::filesystem::path& package_json_path,
                         std::string script_name,
                         std::string command,
                         std::string path_env_prefix,
                         std::vector<std::string> positional_args)
      : package_json_path_(package_json_path),
        script_name_(std::move(script_name)),
        command_(std::move(command)),
        path_env_prefix_(std::move(path_env_prefix)),
        positional_args_(std::move(positional_args)) {
    std::memset(&options_, 0, sizeof(options_));
    options_.stdio_count = 3;
    child_stdio_[0].flags = UV_INHERIT_FD;
    child_stdio_[0].data.fd = 0;
    child_stdio_[1].flags = UV_INHERIT_FD;
    child_stdio_[1].data.fd = 1;
    child_stdio_[2].flags = UV_INHERIT_FD;
    child_stdio_[2].data.fd = 2;
    options_.stdio = child_stdio_;
    options_.exit_cb = ExitCallback;
#ifdef _WIN32
    options_.flags |= UV_PROCESS_WINDOWS_VERBATIM_ARGUMENTS;
    file_ = "cmd.exe";
#else
    file_ = "/bin/sh";
#endif
    cwd_ = package_json_path_.parent_path().string();
    process_.data = this;
  }

  int Run(std::string* error_out) {
    SetEnvironmentVariables();
    std::string command_string(command_);
    for (const auto& arg : positional_args_) {
      command_string += " ";
      command_string += EscapeShellArgument(arg);
    }

#ifdef _WIN32
    if (file_.ends_with("cmd.exe")) {
      command_args_ = {
          file_, "/d", "/s", "/c", "\"" + command_string + "\""};
    } else {
      command_args_ = {file_, "-c", command_string};
    }
#else
    command_args_ = {file_, "-c", command_string};
#endif

    arg_ = std::make_unique<char*[]>(command_args_.size() + 1);
    for (size_t i = 0; i < command_args_.size(); ++i) {
      arg_[i] = const_cast<char*>(command_args_[i].c_str());
    }
    arg_[command_args_.size()] = nullptr;
    options_.file = file_.c_str();
    options_.args = arg_.get();

    options_.cwd = cwd_.c_str();

    if (const int rc = uv_spawn(uv_default_loop(), &process_, &options_); rc != 0) {
      if (error_out != nullptr) {
        *error_out = "Error: " + std::string(uv_strerror(rc));
      }
      return 1;
    }

    (void)uv_run(uv_default_loop(), UV_RUN_DEFAULT);
    return exit_code_;
  }

 private:
  static void ExitCallback(uv_process_t* handle, int64_t exit_status, int /*term_signal*/) {
    auto* self = static_cast<CliPackageScriptRunner*>(handle->data);
    uv_close(reinterpret_cast<uv_handle_t*>(handle), nullptr);
    self->exit_code_ = exit_status > 0 ? 1 : 0;
  }

  void SetEnvironmentVariables() {
    uv_env_item_t* env_items = nullptr;
    int env_count = 0;
    if (uv_os_environ(&env_items, &env_count) != 0) {
      return;
    }

    for (int i = 0; i < env_count; ++i) {
      std::string name = env_items[i].name;
      std::string value = env_items[i].value;
#if defined(_WIN32)
      if (StringEqualsNoCase(name, "comspec")) {
        file_ = value;
      }
#endif
      if (StringEqualsPathVariableName(name)) {
        value = path_env_prefix_ + value;
      }
      env_vars_.push_back(name + "=" + value);
    }
    uv_os_free_environ(env_items, env_count);

    env_vars_.push_back("NODE_RUN_SCRIPT_NAME=" + script_name_);
    env_vars_.push_back("NODE_RUN_PACKAGE_JSON_PATH=" + package_json_path_.string());

    env_ = std::make_unique<char*[]>(env_vars_.size() + 1);
    for (size_t i = 0; i < env_vars_.size(); ++i) {
      env_[i] = const_cast<char*>(env_vars_[i].c_str());
    }
    env_[env_vars_.size()] = nullptr;
    options_.env = env_.get();
  }

  uv_process_t process_{};
  uv_process_options_t options_{};
  uv_stdio_container_t child_stdio_[3]{};
  std::vector<std::string> command_args_;
  std::vector<std::string> env_vars_;
  std::unique_ptr<char*[]> env_;
  std::unique_ptr<char*[]> arg_;
  std::string file_;
  std::string cwd_;
  std::filesystem::path package_json_path_;
  std::string script_name_;
  std::string command_;
  std::string path_env_prefix_;
  std::vector<std::string> positional_args_;
  int exit_code_ = 1;
};

int RunCliPackageScript(const std::string& script_name,
                        const std::vector<std::string>& positional_args,
                        std::string* error_out) {
  std::error_code ec;
  const std::filesystem::path cwd = std::filesystem::current_path(ec);
  const std::string current_working_directory = ec ? std::string("<cwd unavailable>") : cwd.string();
  const auto package_json = ec ? std::optional<CliPackageJsonInfo>{} : FindCliPackageJson(cwd);
  if (!package_json.has_value()) {
    if (error_out != nullptr) {
      *error_out = "Can't find package.json for directory " + current_working_directory + "\n";
    }
    return 1;
  }

  const auto& package_json_path = package_json->package_json_path;
  simdjson::ondemand::parser parser;
  simdjson::padded_string padded(package_json->raw_json);
  simdjson::ondemand::document document;
  if (parser.iterate(padded).get(document) != simdjson::SUCCESS) {
    if (error_out != nullptr) {
      *error_out = "Can't parse " + package_json_path.string() + "\n";
    }
    return 1;
  }

  simdjson::ondemand::object root;
  if (const auto root_error = document.get_object().get(root); root_error != simdjson::SUCCESS) {
    if (error_out != nullptr) {
      if (root_error == simdjson::error_code::INCORRECT_TYPE) {
        *error_out = "Root value unexpected not an object for " +
                     package_json_path.string() + "\n\n";
      } else {
        *error_out = "Can't parse " + package_json_path.string() + "\n";
      }
    }
    return 1;
  }

  simdjson::ondemand::object scripts;
  if (root["scripts"].get_object().get(scripts) != simdjson::SUCCESS) {
    if (error_out != nullptr) {
      *error_out = "Can't find \"scripts\" field in " + package_json_path.string() + "\n";
    }
    return 1;
  }

  std::string_view command;
  if (const auto command_error = scripts[script_name].get_string().get(command);
      command_error != simdjson::SUCCESS) {
    if (error_out != nullptr) {
      if (command_error == simdjson::error_code::INCORRECT_TYPE) {
        *error_out = "Script \"" + script_name +
                     "\" is unexpectedly not a string for " +
                     package_json_path.string() + "\n\n";
      } else {
        *error_out = "Missing script: \"" + script_name + "\" for " +
                     package_json_path.string() + "\n\nAvailable scripts are:\n";
        scripts.reset();
        simdjson::ondemand::value value;
        for (auto field : scripts) {
          std::string_view key;
          std::string_view raw_value;
          if (field.unescaped_key().get(key) != simdjson::SUCCESS ||
              field.value().get(value) != simdjson::SUCCESS ||
              value.get_string().get(raw_value) != simdjson::SUCCESS) {
            continue;
          }
          *error_out += "  " + std::string(key) + ": " + std::string(raw_value) + "\n";
        }
      }
    }
    return 1;
  }

  CliPackageScriptRunner runner(package_json_path,
                                script_name,
                                std::string(command),
                                package_json->path_env_prefix,
                                positional_args);
  return runner.Run(error_out);
}

bool StdinIsTTY() {
#if defined(_WIN32)
  return uv_guess_handle(_fileno(stdin)) == UV_TTY;
#else
  return uv_guess_handle(STDIN_FILENO) == UV_TTY;
#endif
}

bool StdinSupportsRawMode() {
  if (!StdinIsTTY()) return false;
#if defined(__wasi__) || defined(__wasm32__)
  return false;
#else
  return true;
#endif
}

void PrepareEnvForRepl() {
  // WASIX stdio can present stdin as a TTY while still rejecting raw mode.
  // Force the plain line-based REPL in that case instead of crashing during
  // readline startup.
  if (StdinIsTTY() && !StdinSupportsRawMode()) {
    (void)ApplyEnvUpdate("NODE_NO_READLINE", "1");
  }
}

int RunCliBuiltin(const char* source_text,
                  const char* native_main_builtin_id,
                  const char* current_script_path,
                  std::string* error_out,
                  const RuntimeInitOptions& init_options = {}) {
  return RunWithFreshEnv(
      [&](napi_env env) {
        EdgeSetCurrentScriptPath(current_script_path != nullptr ? current_script_path : "");
        return EdgeRunScriptSourceWithLoop(env,
                                          source_text,
                                          error_out,
                                          true,
                                          native_main_builtin_id);
      },
      error_out,
      init_options);
}

int RunEnvCliWithOffset(int argc,
                        const char* const* argv,
                        int dispatch_offset,
                        std::string* error_out) {
  if (argv != nullptr && argc > 0 && argv[0] != nullptr) {
    EdgeSetProcessArgv0(argv[0]);
  }
  const int adjusted_argc = argc - dispatch_offset;
  const char* const* adjusted_argv = argv != nullptr ? argv + dispatch_offset : nullptr;
  return EdgeRunCompatCommand(adjusted_argc, adjusted_argv, error_out);
}

}  // namespace

void EdgeInitializeCliProcess() {
  std::call_once(g_cli_init_once, []() {
    ResetSignalHandlersLikeNode();
#if !defined(_WIN32)
    node::InitializeStdio();
    node::RegisterSignalHandler(SIGINT, node::SignalExit, true);
    node::RegisterSignalHandler(SIGTERM, node::SignalExit, true);
#endif
  });
}

int EdgeRunCliScript(const char* script_path,
                     const std::vector<std::string>* effective_exec_argv,
                     std::string* error_out) {
  EdgeInitializeCliProcess();
  if (error_out != nullptr) {
    error_out->clear();
  }
  if (script_path == nullptr || script_path[0] == '\0') {
    if (error_out != nullptr) {
      *error_out = kUsage;
    }
    return 1;
  }

  const std::string resolved_script_path = ResolveCliScriptPath(script_path);
  RuntimeInitOptions init_options;
  init_options.raw_exec_argv = effective_exec_argv;
  init_options.effective_exec_argv = effective_exec_argv;
  return RunWithFreshEnv(
      [&](napi_env env) {
        return EdgeRunScriptFileWithLoop(env, resolved_script_path.c_str(), error_out, true);
      },
      error_out,
      init_options);
}

int EdgeRunCli(int argc, const char* const* argv, std::string* error_out) {
  CliStartupTracer startup_trace;
  EDGE_STARTUP_TRACE(startup_trace, "cli.enter");
  EdgeInitializeCliProcess();
  EDGE_STARTUP_TRACE(startup_trace, "cli.initialize-process");
  if (error_out != nullptr) {
    error_out->clear();
  }
  if (argv != nullptr && argc > 0 && argv[0] != nullptr) {
    EdgeSetProcessArgv0(argv[0]);
  }
  if (argv == nullptr || argc < 1) {
    if (error_out != nullptr) *error_out = kUsage;
    return 1;
  }
  if (argc > 1 && argv[1] != nullptr &&
      std::strcmp(argv[1], kEdgeInternalEnvCliDispatchFlag) == 0) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.env-compat");
    return RunEnvCliWithOffset(argc, argv, 1, error_out);
  }
  if (argc > 1 && argv[1] != nullptr && EdgeShouldWrapCompatCommand(argv[1])) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.compat-command");
    return EdgeRunCompatCommand(argc, argv, error_out);
  }
  const bool env_safe_mode = EdgeIsTruthyEnvVar(std::getenv("EDGE_SAFE"));
  bool cli_safe_mode = false;
  for (int i = 1; i < argc; ++i) {
    if (argv[i] == nullptr) continue;
    if (std::strcmp(argv[i], "--safe") == 0) cli_safe_mode = true;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.scan-safe-mode");
  const bool safe_mode_requested = cli_safe_mode || env_safe_mode;
  if (safe_mode_requested) {
    bool has_cli_wasmer_bin = false;
    bool has_cli_wasmer_package = false;
    std::string wasmer_bin;
    std::string wasmer_package;
    for (int i = 1; i < argc; ++i) {
      if (argv[i] == nullptr) continue;
      const std::string token(argv[i]);
      if (token == "--wasmer-bin") {
        if (i + 1 >= argc || argv[i + 1] == nullptr) {
          if (error_out != nullptr) {
            *error_out = FormatCliError("--wasmer-bin requires an argument");
          }
          return 1;
        }
        has_cli_wasmer_bin = true;
        wasmer_bin = argv[i + 1];
        ++i;
        continue;
      }
      if (token.rfind("--wasmer-bin=", 0) == 0) {
        has_cli_wasmer_bin = true;
        wasmer_bin = token.substr(std::strlen("--wasmer-bin="));
        if (wasmer_bin.empty()) {
          if (error_out != nullptr) {
            *error_out = FormatCliError("--wasmer-bin requires an argument");
          }
          return 1;
        }
      }
      if (token == "--edgejs-wasmer-package" || token == "--wasmer-package") {
        if (i + 1 >= argc || argv[i + 1] == nullptr) {
          if (error_out != nullptr) {
            *error_out = FormatCliError(token + " requires an argument");
          }
          return 1;
        }
        has_cli_wasmer_package = true;
        wasmer_package = argv[i + 1];
        ++i;
        continue;
      }
      if (token.rfind("--edgejs-wasmer-package=", 0) == 0) {
        has_cli_wasmer_package = true;
        wasmer_package = token.substr(std::strlen("--edgejs-wasmer-package="));
        if (wasmer_package.empty()) {
          if (error_out != nullptr) {
            *error_out = FormatCliError("--edgejs-wasmer-package requires an argument");
          }
          return 1;
        }
        continue;
      }
      if (token.rfind("--wasmer-package=", 0) == 0) {
        has_cli_wasmer_package = true;
        wasmer_package = token.substr(std::strlen("--wasmer-package="));
        if (wasmer_package.empty()) {
          if (error_out != nullptr) {
            *error_out = FormatCliError("--wasmer-package requires an argument");
          }
          return 1;
        }
      }
    }
    if (!has_cli_wasmer_bin) {
      const char* env_wasmer_bin = std::getenv("WASMER_BIN");
      if (env_wasmer_bin != nullptr && env_wasmer_bin[0] != '\0') {
        wasmer_bin = env_wasmer_bin;
      }
    }
    if (!has_cli_wasmer_package) {
      const char* env_wasmer_package = std::getenv("EDGE_WASMER_PACKAGE");
      if (env_wasmer_package != nullptr && env_wasmer_package[0] != '\0') {
        wasmer_package = env_wasmer_package;
      }
    }
    std::vector<std::string> forwarded_args;
    forwarded_args.reserve(static_cast<size_t>(argc > 1 ? argc - 1 : 0));
    for (int i = 1; i < argc; ++i) {
      if (argv[i] == nullptr) continue;
      const std::string token(argv[i]);
      if (token == "--safe") continue;
      if (token == "--wasmer-bin") {
        ++i;
        continue;
      }
      if (token == "--edgejs-wasmer-package" || token == "--wasmer-package") {
        ++i;
        continue;
      }
      if (token.rfind("--wasmer-bin=", 0) == 0) continue;
      if (token.rfind("--edgejs-wasmer-package=", 0) == 0) continue;
      if (token.rfind("--wasmer-package=", 0) == 0) continue;
      forwarded_args.emplace_back(argv[i]);
    }
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.safe-mode");
    return EdgeRunSafeModeCommand(forwarded_args, wasmer_bin, wasmer_package, error_out);
  }
  if (argc > 1 && argv[1] != nullptr &&
      (std::strcmp(argv[1], "-v") == 0 || std::strcmp(argv[1], "--version") == 0)) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.version");
    std::cout << NODE_VERSION << "\n";
    return 0;
  }
  if (argc > 1 && argv[1] != nullptr &&
      (std::strcmp(argv[1], "-h") == 0 || std::strcmp(argv[1], "--help") == 0)) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.help");
    std::cout << kNativeHelpText;
    return 0;
  }
  enum class CliMode {
    kNone,
    kInteractive,
    kEval,
    kPrint,
    kCheck,
    kRun,
    kPrecompile,
  };

  CliMode mode = CliMode::kNone;
  std::vector<std::string> raw_exec_argv;
  std::vector<std::string> run_positional_argv;
  std::vector<std::string> script_argv;
  raw_exec_argv.reserve(static_cast<size_t>(argc));
  int script_index = argc;
  std::string run_target;
  bool saw_check = false;
  bool saw_precompile = false;
  bool print_flag = false;
  bool has_eval_string = false;
  bool force_repl = false;

  auto set_requires_argument_error = [&](const std::string& token) {
    if (error_out != nullptr) {
      *error_out = FormatCliError(token + " requires an argument");
    }
  };

  auto finalize_effective_state = [&](edge_options::EffectiveCliState* out_state) -> bool {
    if (out_state == nullptr) return false;
    *out_state = edge_options::BuildEffectiveCliState(raw_exec_argv);
    if (!out_state->ok) {
      if (error_out != nullptr) {
        *error_out = FormatCliError(out_state->error);
      }
      return false;
    }
    if (!ValidateNodeOptions(out_state->node_options_tokens, error_out)) {
      return false;
    }
    if (!ValidateCaOptions(*out_state, error_out)) {
      return false;
    }
    if (!ValidateTlsProtocolBoundsOptions(*out_state, error_out)) {
      return false;
    }
    if (!ValidateTraceRequireModuleOption(*out_state, error_out)) {
      return false;
    }
    if (!ValidateUnhandledRejectionsOption(*out_state, error_out)) {
      return false;
    }
    for (const auto& warning : out_state->warnings) {
      std::cerr << warning << "\n";
    }
    WarnUnsupportedPermissionsIfNeeded(*out_state);
    return true;
  };

  int i = 1;
  for (; i < argc; ++i) {
    if (argv[i] == nullptr) continue;
    std::string token(argv[i]);

    if (token == "--pending_deprecation") {
      token = "--pending-deprecation";
    } else if (token == "--security-reverts") {
      token = "--security-revert";
    }

    if (token == "--") {
      script_index = i + 1;
      break;
    }
    if (token == "-") {
      script_index = i;
      break;
    }
    if (token.empty() || token[0] != '-') {
      script_index = i;
      break;
    }
    if (token.rfind("--env-file-", 0) == 0 &&
        token != "--env-file" &&
        token.rfind("--env-file=", 0) != 0 &&
        token != "--env-file-if-exists" &&
        token.rfind("--env-file-if-exists=", 0) != 0) {
      if (error_out != nullptr) {
        *error_out = "bad option: " + token;
      }
      return 9;
    }
    if (!ValidateNegatedOption(token, error_out)) {
      return 9;
    }
    if (TokenRequiresNonEmptyInlineValue(token)) {
      set_requires_argument_error(token.substr(0, token.size() - 1));
      return 9;
    }
    if (token == "-c" || token == "--check") {
      raw_exec_argv.push_back(token);
      saw_check = true;
      mode = CliMode::kCheck;
      continue;
    }
    if (token == "--precompile") {
      raw_exec_argv.push_back(token);
      saw_precompile = true;
      mode = CliMode::kPrecompile;
      continue;
    }
    if (token == "-i" || token == "--interactive") {
      raw_exec_argv.push_back(token);
      force_repl = true;
      mode = CliMode::kInteractive;
      continue;
    }
    if (token == "-e" || token == "--eval" ||
        token == "-pe" || token == "-ep") {
      if (saw_check) {
        if (error_out != nullptr) {
          *error_out = FormatCliError("either --check or --eval can be used, not both");
        }
        return 9;
      }
      if (i + 1 >= argc || argv[i + 1] == nullptr) {
        set_requires_argument_error(token);
        return 9;
      }
      raw_exec_argv.push_back(token);
      raw_exec_argv.emplace_back(argv[++i]);
      has_eval_string = true;
      if (token == "-p" || token == "--print" || token == "-pe" || token == "-ep") {
        print_flag = true;
      }
      mode = print_flag ? CliMode::kPrint : CliMode::kEval;
      continue;
    }
    if (token.rfind("--eval=", 0) == 0) {
      if (saw_check) {
        if (error_out != nullptr) {
          *error_out = FormatCliError("either --check or --eval can be used, not both");
        }
        return 9;
      }
      raw_exec_argv.push_back(token);
      has_eval_string = true;
      mode = print_flag ? CliMode::kPrint : CliMode::kEval;
      continue;
    }
    if (token == "-p" || token == "--print") {
      raw_exec_argv.push_back(token);
      print_flag = true;
      mode = CliMode::kPrint;
      if (i + 1 < argc && argv[i + 1] != nullptr) {
        const std::string next(argv[i + 1]);
        if (!IsRecognizedCliOptionToken(next)) {
          raw_exec_argv.emplace_back(argv[++i]);
          has_eval_string = true;
          continue;
        }
      }
      continue;
    }
    if (token.rfind("--print=", 0) == 0) {
      raw_exec_argv.push_back(token);
      print_flag = true;
      has_eval_string = true;
      mode = CliMode::kPrint;
      continue;
    }
    if (token == "--run") {
      if (i + 1 >= argc || argv[i + 1] == nullptr) {
        set_requires_argument_error(token);
        return 9;
      }
      raw_exec_argv.push_back(token);
      run_target = argv[++i];
      raw_exec_argv.push_back(run_target);
      mode = CliMode::kRun;
      continue;
    }

    raw_exec_argv.push_back(token);
    if (TokenHasInlineValue(token)) continue;
    if (OptionConsumesNextToken(token)) {
      if (i + 1 >= argc || argv[i + 1] == nullptr) {
        set_requires_argument_error(token);
        return 9;
      }
      raw_exec_argv.emplace_back(argv[++i]);
    }
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.parse-argv");

  if (script_index == argc) script_index = i;

  edge_options::EffectiveCliState effective_state;
  if (!finalize_effective_state(&effective_state)) {
    return 9;
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.build-effective-state");

  EdgeSetExecArgv(raw_exec_argv);
  ApplySupportedV8Flags(raw_exec_argv);
  EDGE_STARTUP_TRACE(startup_trace, "cli.apply-v8-flags");

  // Bytecode cache defaults: builtins ON, per-file user sidecars OFF.
  //   --no-bytecode-cache / --check  -> kill switch (builtins + sidecars off)
  //   --bytecode-cache / --precompile -> opt user sidecars in
  // --check must never touch sidecars: a syntax check should not write to the
  // checked tree.
  const int bytecode_cache_override =
      BytecodeCacheCliOverride(effective_state.effective_tokens);
  if (bytecode_cache_override == -1 || mode == CliMode::kCheck) {
    edge_bytecode_cache::SetCacheDisabledFromCli();
  } else if (bytecode_cache_override == 1 || saw_precompile) {
    edge_bytecode_cache::SetSidecarsEnabledFromCli();
  }
  if (HasExactOptionToken(effective_state.effective_tokens, "--completion-bash")) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.completion-bash");
    std::cout << GetBashCompletion();
    return 0;
  }
  std::string security_revert;
  if (GetSecurityRevertTokenValue(raw_exec_argv, &security_revert)) {
    std::cerr << CliErrorPrefix() << ": Error: Attempt to revert an unknown CVE ["
              << security_revert << "]\n";
    return 12;
  }
  const bool use_test_runner = HasExactOptionToken(effective_state.effective_tokens, "--test");
  const bool use_watch_mode =
      IsBooleanOptionEnabled(effective_state.effective_tokens, "--watch") ||
      HasOptionTokenWithInlineValue(effective_state.effective_tokens, "--watch-path");
  bool requested_test_flag = use_test_runner;
  if (!requested_test_flag) {
    for (int argi = script_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr && std::strcmp(argv[argi], "--test") == 0) {
        requested_test_flag = true;
        break;
      }
    }
  }

  if (requested_test_flag) {
    if (saw_check) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --test or --check can be used, not both");
      }
      return 9;
    }
    if (has_eval_string) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --test or --eval can be used, not both");
      }
      return 9;
    }
    if (force_repl) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --test or --interactive can be used, not both");
      }
      return 9;
    }
    if (HasOptionTokenWithInlineValue(effective_state.effective_tokens, "--watch-path")) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("--watch-path cannot be used in combination with --test");
      }
      return 9;
    }
  }

  if (saw_precompile) {
    const char* conflicting = nullptr;
    if (saw_check) {
      conflicting = "--check";
    } else if (has_eval_string) {
      conflicting = "--eval";
    } else if (force_repl) {
      conflicting = "--interactive";
    } else if (requested_test_flag) {
      conflicting = "--test";
    } else if (use_watch_mode) {
      conflicting = "--watch";
    } else if (!run_target.empty()) {
      conflicting = "--run";
    } else if (bytecode_cache_override == -1) {
      conflicting = "--no-bytecode-cache";
    }
    if (conflicting != nullptr) {
      if (error_out != nullptr) {
        *error_out = FormatCliError(std::string("either --precompile or ") + conflicting +
                                    " can be used, not both");
      }
      return 9;
    }
  }

  if (use_watch_mode) {
    if (saw_check) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --watch or --check can be used, not both");
      }
      return 9;
    }
    if (has_eval_string) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --watch or --eval can be used, not both");
      }
      return 9;
    }
    if (force_repl) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --watch or --interactive can be used, not both");
      }
      return 9;
    }
    if (HasExactOptionToken(effective_state.effective_tokens, "--test-force-exit")) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("either --watch or --test-force-exit can be used, not both");
      }
      return 9;
    }
    if (!use_test_runner &&
        !HasOptionTokenWithInlineValue(effective_state.effective_tokens, "--watch-path") &&
        (script_index >= argc || argv[script_index] == nullptr)) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("--watch requires specifying a file");
      }
      return 9;
    }
  }

  if (!use_watch_mode) {
    ApplyEnvUpdates(effective_state.env_updates);
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.apply-env-updates");

  RuntimeInitOptions init_options;
  init_options.raw_exec_argv = &raw_exec_argv;
  init_options.effective_exec_argv = &effective_state.effective_tokens;

  if (force_repl) {
    if (RawExecArgvHasInputType(raw_exec_argv)) {
      if (error_out != nullptr) {
        *error_out = "Cannot specify --input-type for REPL";
      }
      return 1;
    }
    PrepareEnvForRepl();
    EdgeSetScriptArgv({});
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.repl");
    return RunCliBuiltin(";", "internal/main/repl", nullptr, error_out, init_options);
  }

  if (has_eval_string || (print_flag && mode == CliMode::kPrint)) {
    if (script_index < argc && argv[script_index] != nullptr && std::strcmp(argv[script_index], "--") == 0) {
      script_index++;
    }
    script_argv.reserve(static_cast<size_t>(argc - script_index));
    for (int argi = script_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr) script_argv.emplace_back(argv[argi]);
    }
    EdgeSetScriptArgv(script_argv);
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.eval");
    return RunCliBuiltin(";", "internal/main/eval_string", nullptr, error_out, init_options);
  }

  if (mode == CliMode::kRun) {
    int positional_index = script_index;
    if (positional_index < argc &&
        argv[positional_index] != nullptr &&
        std::strcmp(argv[positional_index], "--") == 0) {
      positional_index++;
    }
    run_positional_argv.reserve(static_cast<size_t>(argc - positional_index));
    for (int argi = positional_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr) run_positional_argv.emplace_back(argv[argi]);
    }
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.run");
    return RunCliPackageScript(run_target, run_positional_argv, error_out);
  }

  if (use_test_runner) {
    int pattern_index = script_index;
    if (pattern_index < argc &&
        argv[pattern_index] != nullptr &&
        std::strcmp(argv[pattern_index], "--") == 0) {
      pattern_index++;
    }
    script_argv.reserve(static_cast<size_t>(argc - pattern_index));
    for (int argi = pattern_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr) script_argv.emplace_back(argv[argi]);
    }
    EdgeSetScriptArgv(script_argv);
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.test");
    return RunCliBuiltin(";", "internal/main/test_runner", nullptr, error_out, init_options);
  }

  if (use_watch_mode) {
    int command_index = script_index;
    if (command_index < argc &&
        argv[command_index] != nullptr &&
        std::strcmp(argv[command_index], "--") == 0) {
      command_index++;
    }
    script_argv.reserve(static_cast<size_t>(argc - command_index));
    for (int argi = command_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr) script_argv.emplace_back(argv[argi]);
    }
    EdgeSetScriptArgv(script_argv);
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.watch");
    return RunCliBuiltin(";", "internal/main/watch_mode", nullptr, error_out, init_options);
  }

  if (mode == CliMode::kPrecompile) {
    int path_index = script_index;
    if (path_index < argc && argv[path_index] != nullptr && std::strcmp(argv[path_index], "--") == 0) {
      path_index++;
    }
    std::vector<std::string> precompile_paths;
    precompile_paths.reserve(static_cast<size_t>(argc - path_index));
    for (int argi = path_index; argi < argc; ++argi) {
      if (argv[argi] != nullptr) precompile_paths.emplace_back(argv[argi]);
    }
    if (precompile_paths.empty()) {
      if (error_out != nullptr) {
        *error_out = FormatCliError("--precompile requires at least one file or directory");
      }
      return 9;
    }
    EdgeSetScriptArgv(precompile_paths);
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.precompile");
    return RunWithFreshEnv(
        [&](napi_env env) {
          return edge_precompile::RunPrecompile(env, precompile_paths, error_out);
        },
        error_out,
        init_options);
  }

  const bool use_stdin_entry =
      script_index >= argc || argv[script_index] == nullptr || std::strcmp(argv[script_index], "-") == 0;
  if (use_stdin_entry) {
    if (script_index < argc && argv[script_index] != nullptr && std::strcmp(argv[script_index], "-") == 0) {
      script_argv.reserve(static_cast<size_t>(argc - (script_index + 1)));
      for (int argi = script_index + 1; argi < argc; ++argi) {
        if (argv[argi] != nullptr) script_argv.emplace_back(argv[argi]);
      }
    }
    EdgeSetScriptArgv(script_argv);
    if (mode == CliMode::kCheck) {
      EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.stdin-check");
      return RunCliBuiltin(";", "internal/main/check_syntax", "-", error_out, init_options);
    }
    PrepareEnvForRepl();
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.stdin");
    return RunCliBuiltin(";", StdinIsTTY() ? "internal/main/repl" : "internal/main/eval_stdin",
                         "-", error_out, init_options);
  }

  script_argv.reserve(static_cast<size_t>(argc - (mode == CliMode::kCheck ? script_index : (script_index + 1))));
  for (int argi = (mode == CliMode::kCheck ? script_index : (script_index + 1)); argi < argc; ++argi) {
    if (argv[argi] != nullptr) script_argv.emplace_back(argv[argi]);
  }
  EdgeSetScriptArgv(script_argv);
  if (mode == CliMode::kCheck) {
    EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.check");
    return RunCliBuiltin(";", "internal/main/check_syntax", nullptr, error_out, init_options);
  }
  EDGE_STARTUP_TRACE(startup_trace, "cli.dispatch.script");
  return EdgeRunCliScript(argv[script_index], &effective_state.effective_tokens, error_out);
}

int EdgeRunEnvCli(int argc, const char* const* argv, std::string* error_out) {
  EdgeInitializeCliProcess();
  if (error_out != nullptr) {
    error_out->clear();
  }
  if (argv != nullptr && argc > 0 && argv[0] != nullptr) {
    EdgeSetProcessArgv0(argv[0]);
  }
  return RunEnvCliWithOffset(argc, argv, 0, error_out);
}
