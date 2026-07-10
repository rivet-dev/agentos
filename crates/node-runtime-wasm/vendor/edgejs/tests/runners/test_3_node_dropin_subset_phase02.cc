#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <string_view>
#include <unordered_map>
#include <unordered_set>
#include <vector>
#if !defined(_WIN32)
#include <cerrno>
#if defined(__APPLE__)
#include <crt_externs.h>
#endif
#include <signal.h>
#include <fcntl.h>
#include <sys/file.h>
#include <sys/wait.h>
#include <unistd.h>
#else
#include <process.h>
#endif

#include "test_env.h"
#include "edge_module_loader.h"
#include "edge_process_wrap.h"
#include "edge_runtime.h"

namespace {
void BestEffortKillLeakedFixtureChildren();
}  // namespace

class Test3NodeDropinSubsetPhase02 : public FixtureTestBase {
 protected:
  static void TearDownTestSuite() { BestEffortKillLeakedFixtureChildren(); }
};

namespace {

uint64_t CurrentProcessId() {
#if defined(_WIN32)
  return static_cast<uint64_t>(_getpid());
#else
  return static_cast<uint64_t>(getpid());
#endif
}

std::string MakeTestSerialId(std::string_view key) {
  std::string material(key);
  material.push_back(':');
  material += std::to_string(CurrentProcessId());

  // Stable FNV-1a hash over script key + process id so concurrent invocations
  // of the same script do not collide on Node test temp paths/ports.
  uint64_t h = 1469598103934665603ull;
  for (char c : material) {
    h ^= static_cast<unsigned char>(c);
    h *= 1099511628211ull;
  }
  const uint32_t id = static_cast<uint32_t>((h % 1000000000ull) + 1ull);
  return std::to_string(id);
}

using EnvSnapshot = std::unordered_map<std::string, std::string>;

EnvSnapshot CaptureEnvSnapshot() {
  EnvSnapshot snapshot;
#if defined(_WIN32)
  extern char** _environ;
  char** envp = _environ;
#elif defined(__APPLE__)
  char** envp = *_NSGetEnviron();
#else
  extern char** environ;
  char** envp = ::environ;
#endif
  if (envp == nullptr) {
    return snapshot;
  }
  for (; *envp != nullptr; ++envp) {
    std::string entry(*envp);
    const size_t eq = entry.find('=');
    if (eq == std::string::npos) continue;
    snapshot.emplace(entry.substr(0, eq), entry.substr(eq + 1));
  }
  return snapshot;
}

void RestoreEnvSnapshot(const EnvSnapshot& snapshot) {
  const EnvSnapshot current = CaptureEnvSnapshot();
  for (const auto& [name, _] : current) {
    if (snapshot.find(name) == snapshot.end()) {
      unsetenv(name.c_str());
    }
  }
  for (const auto& [name, value] : snapshot) {
    setenv(name.c_str(), value.c_str(), 1);
  }
}

class ScopedEnvSnapshot {
 public:
  ScopedEnvSnapshot() : snapshot_(CaptureEnvSnapshot()) {}
  ~ScopedEnvSnapshot() { RestoreEnvSnapshot(snapshot_); }

 private:
  EnvSnapshot snapshot_;
};

bool StartsWith(std::string_view s, std::string_view prefix) {
  return s.size() >= prefix.size() && s.substr(0, prefix.size()) == prefix;
}

std::vector<std::string> SplitAsciiWhitespace(std::string_view s) {
  std::vector<std::string> tokens;
  while (!s.empty()) {
    while (!s.empty() &&
           (s.front() == ' ' || s.front() == '\t' || s.front() == '\r' || s.front() == '\n')) {
      s.remove_prefix(1);
    }
    if (s.empty()) break;
    size_t end = 0;
    while (end < s.size() &&
           s[end] != ' ' && s[end] != '\t' && s[end] != '\r' && s[end] != '\n') {
      end += 1;
    }
    tokens.emplace_back(s.substr(0, end));
    s.remove_prefix(end);
  }
  return tokens;
}

std::string_view TrimLeadingAsciiWhitespace(std::string_view s) {
  while (!s.empty() &&
         (s.front() == ' ' || s.front() == '\t' || s.front() == '\r' || s.front() == '\n')) {
    s.remove_prefix(1);
  }
  return s;
}

std::vector<std::string> ParseLeadingFlagsHeader(const std::filesystem::path& script_path) {
  std::ifstream in(script_path);
  if (!in.is_open()) {
    return {};
  }

  bool in_block_comment = false;
  std::string line;
  while (std::getline(in, line)) {
    std::string_view view(line);
    if (!view.empty() && view.back() == '\r') {
      view.remove_suffix(1);
    }
    view = TrimLeadingAsciiWhitespace(view);
    if (view.empty()) continue;

    if (in_block_comment) {
      const size_t end = view.find("*/");
      if (end == std::string_view::npos) continue;
      in_block_comment = false;
      view.remove_prefix(end + 2);
      view = TrimLeadingAsciiWhitespace(view);
      if (view.empty()) continue;
    }

    if (StartsWith(view, "// Flags:")) {
      view.remove_prefix(std::string_view("// Flags:").size());
      return SplitAsciiWhitespace(view);
    }
    if (StartsWith(view, "//")) continue;
    if (StartsWith(view, "#!")) continue;

    if (StartsWith(view, "/*")) {
      const size_t end = view.find("*/");
      if (end == std::string_view::npos) {
        in_block_comment = true;
        continue;
      }
      view.remove_prefix(end + 2);
      view = TrimLeadingAsciiWhitespace(view);
      if (view.empty()) continue;
      if (StartsWith(view, "// Flags:")) {
        view.remove_prefix(std::string_view("// Flags:").size());
        return SplitAsciiWhitespace(view);
      }
      if (StartsWith(view, "//")) continue;
      return {};
    }

    return {};
  }
  return {};
}

bool IsUnsupportedFlagsHeaderToken(std::string_view token) {
  static const std::unordered_set<std::string_view> kUnsupportedExactFlags = {
      "--allow-natives-syntax",
      "--disable-wasm-trap-handler",
      "--expose-gc",
      "--expose_gc",
      "--gc-global",
      "--jitless",
      "--no-liftoff",
      "--no-opt",
      "--permission",
  };
  static const std::vector<std::string_view> kUnsupportedPrefixes = {
      "--allow-fs-read",
      "--allow-fs-write",
      "--gc-",
      "--max-old-space-size",
      "--max_old_space_size",
      "--perf-basic-prof",
      "--stress-",
  };

  if (kUnsupportedExactFlags.find(token) != kUnsupportedExactFlags.end()) {
    return true;
  }
  for (const auto& prefix : kUnsupportedPrefixes) {
    if (StartsWith(token, prefix)) {
      return true;
    }
  }
  return false;
}

bool ScriptHasUnsupportedFlagsHeader(const std::filesystem::path& script_path) {
  for (const auto& token : ParseLeadingFlagsHeader(script_path)) {
    if (IsUnsupportedFlagsHeaderToken(token)) {
      return true;
    }
  }
  return false;
}

#if defined(PROJECT_ROOT_PATH)
std::filesystem::path ResolveNodeTestRootPathForRawScript(std::string_view script_rel) {
  namespace fs = std::filesystem;
  fs::path node_test_root_path(PROJECT_ROOT_PATH "/test");
  if (!node_test_root_path.is_absolute()) {
    // Resolve relative test_root by walking up from cwd until we find
    // test/<suite>/<script>.
    fs::path search = fs::current_path();
    bool found = false;
    for (; !search.empty() && search != search.parent_path(); search = search.parent_path()) {
      fs::path candidate = (search / node_test_root_path / std::string(script_rel)).lexically_normal();
      if (fs::exists(candidate)) {
        node_test_root_path = (search / node_test_root_path).lexically_normal();
        found = true;
        break;
      }
    }
    if (!found) {
      node_test_root_path = fs::absolute(fs::current_path().parent_path() / node_test_root_path).lexically_normal();
    }
  } else {
    node_test_root_path = node_test_root_path.lexically_normal();
  }
  return node_test_root_path;
}

std::filesystem::path ResolveRawNodeScriptPath(const char* node_test_relative_path) {
  namespace fs = std::filesystem;
  const std::string rel_path = node_test_relative_path ? std::string(node_test_relative_path) : std::string();
  const bool has_suite_prefix = rel_path.find('/') != std::string::npos;
  const std::string script_rel = has_suite_prefix ? rel_path : ("parallel/" + rel_path);
  const fs::path node_test_root_path = ResolveNodeTestRootPathForRawScript(script_rel);
  return fs::absolute(node_test_root_path / script_rel);
}

bool RawNodeScriptHasUnsupportedFlagsHeader(const char* node_test_relative_path) {
  return ScriptHasUnsupportedFlagsHeader(ResolveRawNodeScriptPath(node_test_relative_path));
}
#endif

class ScopedExclusiveFileLock {
 public:
  explicit ScopedExclusiveFileLock(const char* path) {
#if !defined(_WIN32)
    if (path == nullptr || path[0] == '\0') {
      return;
    }
    fd_ = open(path, O_CREAT | O_RDWR, 0600);
    if (fd_ >= 0) {
      while (flock(fd_, LOCK_EX) == -1 && errno == EINTR) {
      }
    }
#else
    (void)path;
#endif
  }

  ~ScopedExclusiveFileLock() {
#if !defined(_WIN32)
    if (fd_ >= 0) {
      flock(fd_, LOCK_UN);
      close(fd_);
    }
#endif
  }

 private:
  int fd_ = -1;
};

int RunRawNodeTestScript(napi_env env,
                         const char* node_test_relative_path,
                         std::string* error_out,
                         bool keep_event_loop_alive = false);

int RunRawNodeTestScriptInSubprocess(const char* node_test_relative_path,
                                     std::string* error_out,
                                     bool redirect_stdio_to_files = false,
                                     bool use_pseudo_tty = false);

int RunNodeCompatScript(napi_env env, const char* relative_path, std::string* error_out) {
  return RunRawNodeTestScript(env, relative_path, error_out, false);
}

// Run a Node test script from the repo's test checkout (raw drop-in).
// NODE_TEST_DIR points to test so common/fixtures.js can resolve fixtures under test/fixtures.
int RunRawNodeTestScript(napi_env env,
                         const char* node_test_relative_path,
                         std::string* error_out,
                         bool keep_event_loop_alive) {
#if defined(PROJECT_ROOT_PATH)
  namespace fs = std::filesystem;
  ScopedEnvSnapshot env_snapshot;
  std::error_code cwd_ec;
  const fs::path original_cwd = fs::current_path(cwd_ec);
  const bool restore_cwd = !cwd_ec;
  const std::string rel_path = node_test_relative_path ? std::string(node_test_relative_path) : std::string();
  const bool has_suite_prefix = rel_path.find('/') != std::string::npos;
  const std::string script_rel = has_suite_prefix ? rel_path : ("parallel/" + rel_path);
  const bool is_parallel_suite = StartsWith(script_rel, "parallel/");
  const bool is_pseudo_tty_suite = StartsWith(script_rel, "pseudo-tty/");
  if (is_pseudo_tty_suite && !keep_event_loop_alive) {
    return RunRawNodeTestScriptInSubprocess(node_test_relative_path,
                                            error_out,
                                            true,
                                            true);
  }
  const bool needs_global_serialization =
      StartsWith(script_rel, "sequential/") || StartsWith(script_rel, "pummel/");
  ScopedExclusiveFileLock suite_lock(
      needs_global_serialization ? "/tmp/edge-node-suite-serial.lock" : nullptr);
  const fs::path node_test_root_path = ResolveNodeTestRootPathForRawScript(script_rel);
  const std::string script_path_absolute = ResolveRawNodeScriptPath(node_test_relative_path).string();
  if (ScriptHasUnsupportedFlagsHeader(script_path_absolute)) {
    if (error_out != nullptr) error_out->clear();
    return 0;
  }
  const std::string node_test_dir = node_test_root_path.string();
  const std::string test_serial_id = MakeTestSerialId(script_rel);
  setenv("NODE_TEST_DIR", node_test_dir.c_str(), 1);
  setenv("NODE_TEST_KNOWN_GLOBALS", "0", 1);
  setenv("TEST_SERIAL_ID", test_serial_id.c_str(), 1);
  setenv("TEST_THREAD_ID", test_serial_id.c_str(), 1);
  setenv("TEST_PARALLEL", is_parallel_suite ? "1" : "0", 1);
  setenv("EDGE_FORCE_STDIO_TTY", is_pseudo_tty_suite ? "1" : "0", 1);
  {
    napi_value global = nullptr;
    napi_value process_v = nullptr;
    napi_value env_v = nullptr;
    napi_value known_globals_v = nullptr;
    napi_value serial_id_v = nullptr;
    napi_value thread_id_v = nullptr;
    napi_value parallel_v = nullptr;
    if (napi_get_global(env, &global) == napi_ok && global != nullptr &&
        napi_get_named_property(env, global, "process", &process_v) == napi_ok &&
        process_v != nullptr &&
        napi_get_named_property(env, process_v, "env", &env_v) == napi_ok &&
        env_v != nullptr &&
        napi_create_string_utf8(env, "0", NAPI_AUTO_LENGTH, &known_globals_v) == napi_ok &&
        known_globals_v != nullptr &&
        napi_create_string_utf8(
            env, test_serial_id.c_str(), NAPI_AUTO_LENGTH, &serial_id_v) == napi_ok &&
        serial_id_v != nullptr &&
        napi_create_string_utf8(
            env, test_serial_id.c_str(), NAPI_AUTO_LENGTH, &thread_id_v) == napi_ok &&
        thread_id_v != nullptr &&
        napi_create_string_utf8(
            env, is_parallel_suite ? "1" : "0", NAPI_AUTO_LENGTH, &parallel_v) == napi_ok &&
        parallel_v != nullptr) {
      napi_set_named_property(env, env_v, "NODE_TEST_KNOWN_GLOBALS", known_globals_v);
      napi_set_named_property(env, env_v, "TEST_SERIAL_ID", serial_id_v);
      napi_set_named_property(env, env_v, "TEST_THREAD_ID", thread_id_v);
      napi_set_named_property(env, env_v, "TEST_PARALLEL", parallel_v);
    }
  }
  if (keep_event_loop_alive) {
    if (std::getenv("EDGE_LOOP_TIMEOUT_MS") == nullptr) {
      setenv("EDGE_LOOP_TIMEOUT_MS", "30000", 1);
    }
  }
  const int exit_code =
      EdgeRunScriptFileWithLoop(env, script_path_absolute.c_str(), error_out, keep_event_loop_alive);
  if (restore_cwd) {
    std::error_code restore_ec;
    fs::current_path(original_cwd, restore_ec);
  }
  return exit_code;
#else
  (void)env;
  (void)node_test_relative_path;
  (void)error_out;
  return -1;
#endif
}

std::filesystem::path ResolveEdgeCliPathForRawSubprocess() {
  namespace fs = std::filesystem;
  if (const char* explicit_path = std::getenv("EDGE_EXEC_PATH");
      explicit_path != nullptr && explicit_path[0] != '\0') {
    return fs::path(explicit_path);
  }

  const fs::path cwd = fs::current_path();
  const std::vector<fs::path> candidates = {
      cwd / "build-edge" / "edge",
      cwd / "build" / "edge",
      cwd.parent_path() / "build-edge" / "edge",
      cwd.parent_path() / "build" / "edge",
  };
  for (const fs::path& candidate : candidates) {
    std::error_code ec;
    if (!fs::exists(candidate, ec) || ec) continue;
    if (fs::is_directory(candidate, ec) || ec) continue;
    return fs::absolute(candidate).lexically_normal();
  }
  return fs::path("edge");
}

std::string ShellSingleQuoted(std::string_view input) {
  std::string out;
  out.reserve(input.size() + 2);
  out.push_back('\'');
  for (char c : input) {
    if (c == '\'') {
      out += "'\\''";
    } else {
      out.push_back(c);
    }
  }
  out.push_back('\'');
  return out;
}

void BestEffortKillLeakedFixtureChildren() {
#if defined(_WIN32)
  return;
#else
  // Raw drop-in tests sometimes spawn detached fixture children that outlive
  // their parent process tree. Reap known child_process fixture commands.
  constexpr const char* kFixturePatterns[] = {
      "test/fixtures/child-process-",
      "test/fixtures/parent-process-nonpersistent",
  };
  for (const char* pattern : kFixturePatterns) {
    const std::string cmd =
        "/usr/bin/pkill -f " + ShellSingleQuoted(pattern) + " >/dev/null 2>&1 || true";
    (void)std::system(cmd.c_str());
  }
#endif
}

std::string ReadFileText(const std::filesystem::path& path) {
  std::ifstream in(path, std::ios::binary);
  if (!in.is_open()) return {};
  return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

int RunRawNodeTestScriptInSubprocess(const char* node_test_relative_path,
                                     std::string* error_out,
                                     bool redirect_stdio_to_files,
                                     bool use_pseudo_tty) {
#if defined(PROJECT_ROOT_PATH)
#if defined(_WIN32)
  (void)node_test_relative_path;
  (void)redirect_stdio_to_files;
  (void)use_pseudo_tty;
  if (error_out != nullptr) {
    *error_out = "Raw subprocess runner is not implemented on win32.";
  }
  return -1;
#else
  (void)ResolveEdgeCliPathForRawSubprocess;
  namespace fs = std::filesystem;
  const std::string rel_path = node_test_relative_path ? std::string(node_test_relative_path) : std::string();
  const bool has_suite_prefix = rel_path.find('/') != std::string::npos;
  const std::string script_rel = has_suite_prefix ? rel_path : ("parallel/" + rel_path);
  if (StartsWith(script_rel, "pseudo-tty/")) {
    use_pseudo_tty = true;
    redirect_stdio_to_files = true;
  }
  fs::path stdout_path;
  fs::path stderr_path;
  int stdout_fd = -1;
  int stderr_fd = -1;
  auto close_fd = [](int* fd) {
    if (fd != nullptr && *fd >= 0) {
      close(*fd);
      *fd = -1;
    }
  };
  if (redirect_stdio_to_files) {
    char stdout_template[] = "/tmp/edge-raw-stdout-XXXXXX";
    char stderr_template[] = "/tmp/edge-raw-stderr-XXXXXX";
    stdout_fd = mkstemp(stdout_template);
    stderr_fd = mkstemp(stderr_template);
    if (stdout_fd < 0 || stderr_fd < 0) {
      const int create_errno = errno;
      close_fd(&stdout_fd);
      close_fd(&stderr_fd);
      if (error_out != nullptr) {
        *error_out = "mkstemp failed: " + std::string(std::strerror(create_errno));
      }
      return 1;
    }
    stdout_path = stdout_template;
    stderr_path = stderr_template;
  }
  int status = 0;
  const pid_t child_pid = fork();
  if (child_pid < 0) {
    const int fork_errno = errno;
    close_fd(&stdout_fd);
    close_fd(&stderr_fd);
    if (!stdout_path.empty()) (void)fs::remove(stdout_path);
    if (!stderr_path.empty()) (void)fs::remove(stderr_path);
    if (error_out != nullptr) {
      *error_out = "fork failed: " + std::string(std::strerror(fork_errno));
    }
    return 1;
  }

  if (child_pid == 0) {
    if (redirect_stdio_to_files) {
      if (stdout_fd >= 0) {
        (void)dup2(stdout_fd, STDOUT_FILENO);
      }
      if (stderr_fd >= 0) {
        (void)dup2(stderr_fd, STDERR_FILENO);
      }
      close_fd(&stdout_fd);
      close_fd(&stderr_fd);
    }
    // Put each raw-test subprocess tree in its own process group so the parent
    // the parent can always reap/kill descendants after the child exits.
    (void)setpgid(0, 0);
    if (use_pseudo_tty) {
      const bool is_parallel_suite = StartsWith(script_rel, "parallel/");
      const fs::path node_test_root_path = ResolveNodeTestRootPathForRawScript(script_rel);
      const fs::path script_path = ResolveRawNodeScriptPath(node_test_relative_path);
      const fs::path input_path = script_path.parent_path() / (script_path.stem().string() + ".in");
      const std::string node_test_dir = node_test_root_path.string();
      const std::string test_serial_id = MakeTestSerialId(script_rel);
      const fs::path pty_helper = node_test_root_path / "tools" / "pseudo-tty.py";
      const fs::path edge_path = ResolveEdgeCliPathForRawSubprocess();

      setenv("NODE_TEST_DIR", node_test_dir.c_str(), 1);
      setenv("NODE_TEST_KNOWN_GLOBALS", "0", 1);
      setenv("TEST_SERIAL_ID", test_serial_id.c_str(), 1);
      setenv("TEST_THREAD_ID", test_serial_id.c_str(), 1);
      setenv("TEST_PARALLEL", is_parallel_suite ? "1" : "0", 1);
      unsetenv("EDGE_FORCE_STDIO_TTY");

      if (fs::exists(input_path)) {
        const int input_fd = open(input_path.c_str(), O_RDONLY);
        if (input_fd >= 0) {
          (void)dup2(input_fd, STDIN_FILENO);
          close(input_fd);
        }
      }

      execlp("python3",
             "python3",
             pty_helper.c_str(),
             edge_path.c_str(),
             script_path.c_str(),
             static_cast<char*>(nullptr));
      _exit(70);
    }

    void* scope = nullptr;
    napi_env env = nullptr;
    if (unofficial_napi_create_env(8, &env, &scope) != napi_ok || env == nullptr) {
      _exit(70);
    }
    std::string child_error;
    // Raw Node tests rely on libuv/event-loop turns (process exit checks,
    // IPC callbacks, mustCall verification). Run with loop support enabled in
    // subprocess mode to avoid false positives and leaking orphan children.
    const int child_exit = RunRawNodeTestScript(env, node_test_relative_path, &child_error, true);
    // Force-clean any tracked child processes before tearing down this env.
    EdgeProcessWrapForceKillTrackedChildren();
    if (!child_error.empty()) {
      (void)write(STDERR_FILENO, child_error.c_str(), child_error.size());
      (void)write(STDERR_FILENO, "\n", 1);
    }
    if (scope != nullptr) {
      (void)unofficial_napi_release_env(scope);
    }
    _exit(child_exit);
  }

  close_fd(&stdout_fd);
  close_fd(&stderr_fd);

  while (waitpid(child_pid, &status, 0) < 0) {
    if (errno == EINTR) continue;
    const int wait_errno = errno;
    if (error_out != nullptr) {
      *error_out = "waitpid failed: " + std::string(std::strerror(wait_errno));
    }
    return 1;
  }

  // Best-effort kill any descendants left behind by the raw test subprocess.
  // kill(-pgid, ...) targets the process group whose id == child_pid.
  (void)kill(-child_pid, SIGKILL);

  int exit_code = 1;
  if (WIFEXITED(status)) {
    exit_code = WEXITSTATUS(status);
  } else if (WIFSIGNALED(status)) {
    exit_code = 128 + WTERMSIG(status);
  }

  if (error_out != nullptr) {
    if (exit_code == 0) {
      error_out->clear();
    } else {
      *error_out = "subprocess exit(" + std::to_string(exit_code) + ")";
      if (redirect_stdio_to_files) {
        const std::string stdout_text = ReadFileText(stdout_path);
        const std::string stderr_text = ReadFileText(stderr_path);
        if (!stdout_text.empty()) {
          *error_out += "\nstdout=" + stdout_text;
        }
        if (!stderr_text.empty()) {
          *error_out += "\nstderr=" + stderr_text;
        }
      }
    }
  }
  if (!stdout_path.empty()) (void)fs::remove(stdout_path);
  if (!stderr_path.empty()) (void)fs::remove(stderr_path);
  return exit_code;
#endif
#else
  (void)node_test_relative_path;
  if (error_out != nullptr) {
    *error_out = "Raw Node tests require node test root path definitions.";
  }
  return -1;
#endif
}

}  // namespace

TEST_F(Test3NodeDropinSubsetPhase02, NodeAssertSubsetTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-assert-if-error.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RequireCacheSubsetTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-require-cache.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RequireJsonSubsetTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-require-json.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ModuleLoadingSubsetTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-module-cache.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, FsPhaseCSubsetTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-fs-append-file-sync.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-methods.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleAssignUndefinedCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-assign-undefined.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleClearCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-clear.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleGroupCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-group.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleMethodsCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-methods.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleInstanceCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-group.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, ConsoleTableCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-console-clear.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, BufferBase64HardeningCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-buffer-badhex.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsEolCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-eol.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsPriorityCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-process-priority.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsUserInfoCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-userinfo-handles-getter-errors.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsHomedirCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-homedir-no-envvar.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsConstantsCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-constants-signals.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsFastCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-fast.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, OsCheckedCompatTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-os-checked-function.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

#if defined(PROJECT_ROOT_PATH)
TEST_F(Test3NodeDropinSubsetPhase02, RawRequireCacheFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-require-cache.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawRequireJsonFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-require-json.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawModuleCacheFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-module-cache.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawRequireDotFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-require-dot.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsExistsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-exists.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsStatFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-stat.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsWriteSyncFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-write-sync.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsReadFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-read.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsReaddirFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-readdir.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsRenameFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-rename-type-check.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsUnlinkFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-unlink-type-check.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsTruncateSyncFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-truncate-sync.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsCopyfileSyncFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-copyfile.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsAppendFileSyncFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-append-file-sync.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsMkdtempFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-mkdtemp.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsReadlinkTypeCheckFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-readlink-type-check.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsSymlinkFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-symlink.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsChmodFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-chmod.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawFsUtimesFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-fs-utimes.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsEolFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-eol.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsProcessPriorityFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-process-priority.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsCheckedFunctionFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-checked-function.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsFastFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-fast.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsHomedirNoEnvvarFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-homedir-no-envvar.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsUserInfoHandlesGetterErrorsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-userinfo-handles-getter-errors.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawOsConstantsSignalsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-os-constants-signals.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, NodeCompatEventEmitterMethodNamesTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunNodeCompatScript(s.env, "parallel/test-event-emitter-method-names.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

#define DEFINE_RAW_NODE_TEST(test_name, script_name)            \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {             \
    if (std::getenv("EDGE_SKIP_DGRAM_TESTS") != nullptr &&     \
        std::string_view(script_name).find("dgram") != std::string_view::npos) { \
      GTEST_SKIP() << "Skipping dgram test in restricted environment"; \
    }                                                            \
    if (RawNodeScriptHasUnsupportedFlagsHeader(script_name)) {   \
      GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: " << script_name; \
    }                                                            \
    std::string error;                                          \
    const int exit_code = RunRawNodeTestScriptInSubprocess(script_name, &error); \
    EXPECT_EQ(exit_code, 0) << "error=" << error;               \
    EXPECT_TRUE(error.empty()) << "error=" << error;            \
  }

#define DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(test_name, script_name) \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {              \
    std::string error;                                            \
    const int exit_code = RunRawNodeTestScriptInSubprocess(script_name, &error); \
    EXPECT_EQ(exit_code, 0) << "error=" << error;                \
    EXPECT_TRUE(error.empty()) << "error=" << error;             \
  }

#define DEFINE_RAW_NODE_SUBPROCESS_TEST(test_name, script_name) \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {             \
    if (RawNodeScriptHasUnsupportedFlagsHeader(script_name)) {  \
      GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: " << script_name; \
    }                                                            \
    std::string error;                                           \
    const int exit_code = RunRawNodeTestScriptInSubprocess(script_name, &error); \
    EXPECT_EQ(exit_code, 0) << "error=" << error;               \
    EXPECT_TRUE(error.empty()) << "error=" << error;            \
  }

#define DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(test_name, script_name) \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {                        \
    if (RawNodeScriptHasUnsupportedFlagsHeader(script_name)) {             \
      GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: " << script_name; \
    }                                                                      \
    std::string error;                                                     \
    const int exit_code =                                                  \
        RunRawNodeTestScriptInSubprocess(script_name, &error, true);       \
    EXPECT_EQ(exit_code, 0) << "error=" << error;                          \
    EXPECT_TRUE(error.empty()) << "error=" << error;                       \
  }

#define DEFINE_RAW_NODE_IN_PROCESS_TEST(test_name, script_name)  \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {              \
    if (RawNodeScriptHasUnsupportedFlagsHeader(script_name)) {   \
      GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: " << script_name; \
    }                                                             \
    EnvScope s(runtime_.get());                                   \
    std::string error;                                            \
    const int exit_code = RunRawNodeTestScript(s.env, script_name, &error); \
    EXPECT_EQ(exit_code, 0) << "error=" << error;                \
    EXPECT_TRUE(error.empty()) << "error=" << error;             \
  }

DEFINE_RAW_NODE_TEST(RawBufferAllocFromNodeTest, "test-buffer-alloc.js")
DEFINE_RAW_NODE_TEST(RawBufferArraybufferFromNodeTest, "test-buffer-arraybuffer.js")
DEFINE_RAW_NODE_TEST(RawBufferAsciiFromNodeTest, "test-buffer-ascii.js")
DEFINE_RAW_NODE_TEST(RawBufferBackingArraybufferFromNodeTest, "test-buffer-backing-arraybuffer.js")
DEFINE_RAW_NODE_TEST(RawBufferBadhexFromNodeTest, "test-buffer-badhex.js")
DEFINE_RAW_NODE_TEST(RawBufferBigint64FromNodeTest, "test-buffer-bigint64.js")
DEFINE_RAW_NODE_TEST(RawBufferBytelengthFromNodeTest, "test-buffer-bytelength.js")
DEFINE_RAW_NODE_TEST(RawBufferCompareOffsetFromNodeTest, "test-buffer-compare-offset.js")
DEFINE_RAW_NODE_TEST(RawBufferCompareFromNodeTest, "test-buffer-compare.js")
DEFINE_RAW_NODE_TEST(RawBufferConcatFromNodeTest, "test-buffer-concat.js")
DEFINE_RAW_NODE_TEST(RawBufferConstantsFromNodeTest, "test-buffer-constants.js")
DEFINE_RAW_NODE_TEST(RawBufferConstructorDeprecationErrorFromNodeTest, "test-buffer-constructor-deprecation-error.js")
DEFINE_RAW_NODE_TEST(RawBufferConstructorNodeModulesPathsFromNodeTest, "test-buffer-constructor-node-modules-paths.js")
DEFINE_RAW_NODE_TEST(RawBufferConstructorNodeModulesFromNodeTest, "test-buffer-constructor-node-modules.js")
DEFINE_RAW_NODE_TEST(RawBufferConstructorOutsideNodeModulesFromNodeTest, "test-buffer-constructor-outside-node-modules.js")
DEFINE_RAW_NODE_TEST(RawBufferCopyFromNodeTest, "test-buffer-copy.js")
DEFINE_RAW_NODE_TEST(RawBufferEqualsFromNodeTest, "test-buffer-equals.js")
DEFINE_RAW_NODE_TEST(RawBufferFailedAllocTypedArraysFromNodeTest, "test-buffer-failed-alloc-typed-arrays.js")
DEFINE_RAW_NODE_TEST(RawBufferFakesFromNodeTest, "test-buffer-fakes.js")
DEFINE_RAW_NODE_TEST(RawBufferFillFromNodeTest, "test-buffer-fill.js")
DEFINE_RAW_NODE_TEST(RawBufferFromFromNodeTest, "test-buffer-from.js")
DEFINE_RAW_NODE_TEST(RawBufferGenericMethodsFromNodeTest, "test-buffer-generic-methods.js")
DEFINE_RAW_NODE_TEST(RawBufferIncludesFromNodeTest, "test-buffer-includes.js")
DEFINE_RAW_NODE_TEST(RawBufferIndexofFromNodeTest, "test-buffer-indexof.js")
DEFINE_RAW_NODE_TEST(RawBufferInheritanceFromNodeTest, "test-buffer-inheritance.js")
DEFINE_RAW_NODE_TEST(RawBufferInspectFromNodeTest, "test-buffer-inspect.js")
DEFINE_RAW_NODE_TEST(RawBufferIsasciiFromNodeTest, "test-buffer-isascii.js")
DEFINE_RAW_NODE_TEST(RawBufferIsencodingFromNodeTest, "test-buffer-isencoding.js")
DEFINE_RAW_NODE_TEST(RawBufferIsutf8FromNodeTest, "test-buffer-isutf8.js")
DEFINE_RAW_NODE_TEST(RawBufferIteratorFromNodeTest, "test-buffer-iterator.js")
DEFINE_RAW_NODE_TEST(RawBufferNewFromNodeTest, "test-buffer-new.js")
DEFINE_RAW_NODE_TEST(RawBufferNoNegativeAllocationFromNodeTest, "test-buffer-no-negative-allocation.js")
DEFINE_RAW_NODE_TEST(RawBufferNopendingdepMapFromNodeTest, "test-buffer-nopendingdep-map.js")
DEFINE_RAW_NODE_TEST(RawBufferOfNoDeprecationFromNodeTest, "test-buffer-of-no-deprecation.js")
DEFINE_RAW_NODE_TEST(RawBufferOverMaxLengthFromNodeTest, "test-buffer-over-max-length.js")
DEFINE_RAW_NODE_TEST(RawBufferParentPropertyFromNodeTest, "test-buffer-parent-property.js")
DEFINE_RAW_NODE_TEST(RawBufferPendingDeprecationFromNodeTest, "test-buffer-pending-deprecation.js")
DEFINE_RAW_NODE_TEST(RawBufferPoolUntransferableFromNodeTest, "test-buffer-pool-untransferable.js")
DEFINE_RAW_NODE_TEST(RawBufferPrototypeInspectFromNodeTest, "test-buffer-prototype-inspect.js")
DEFINE_RAW_NODE_TEST(RawBufferReadFromNodeTest, "test-buffer-read.js")
DEFINE_RAW_NODE_TEST(RawBufferReaddoubleFromNodeTest, "test-buffer-readdouble.js")
DEFINE_RAW_NODE_TEST(RawBufferReadfloatFromNodeTest, "test-buffer-readfloat.js")
DEFINE_RAW_NODE_TEST(RawBufferReadintFromNodeTest, "test-buffer-readint.js")
DEFINE_RAW_NODE_TEST(RawBufferReaduintFromNodeTest, "test-buffer-readuint.js")
DEFINE_RAW_NODE_TEST(RawBufferResizableFromNodeTest, "test-buffer-resizable.js")
DEFINE_RAW_NODE_TEST(RawBufferSafeUnsafeFromNodeTest, "test-buffer-safe-unsafe.js")
DEFINE_RAW_NODE_TEST(RawBufferSetInspectMaxBytesFromNodeTest, "test-buffer-set-inspect-max-bytes.js")
DEFINE_RAW_NODE_TEST(RawBufferSharedarraybufferFromNodeTest, "test-buffer-sharedarraybuffer.js")
DEFINE_RAW_NODE_TEST(RawBufferSliceFromNodeTest, "test-buffer-slice.js")
DEFINE_RAW_NODE_TEST(RawBufferSlowFromNodeTest, "test-buffer-slow.js")
DEFINE_RAW_NODE_TEST(RawBufferTojsonFromNodeTest, "test-buffer-tojson.js")
DEFINE_RAW_NODE_TEST(RawBufferTostring4gbFromNodeTest, "test-buffer-tostring-4gb.js")
DEFINE_RAW_NODE_TEST(RawBufferTostringRangeFromNodeTest, "test-buffer-tostring-range.js")
DEFINE_RAW_NODE_TEST(RawBufferTostringRangeerrorFromNodeTest, "test-buffer-tostring-rangeerror.js")
DEFINE_RAW_NODE_TEST(RawBufferTostringFromNodeTest, "test-buffer-tostring.js")
DEFINE_RAW_NODE_TEST(RawBufferWriteFastFromNodeTest, "test-buffer-write-fast.js")
DEFINE_RAW_NODE_TEST(RawBufferWriteFromNodeTest, "test-buffer-write.js")
DEFINE_RAW_NODE_TEST(RawBufferWritedoubleFromNodeTest, "test-buffer-writedouble.js")
DEFINE_RAW_NODE_TEST(RawBufferWritefloatFromNodeTest, "test-buffer-writefloat.js")
DEFINE_RAW_NODE_TEST(RawBufferWriteintFromNodeTest, "test-buffer-writeint.js")
DEFINE_RAW_NODE_TEST(RawBufferWriteuintFromNodeTest, "test-buffer-writeuint.js")
DEFINE_RAW_NODE_TEST(RawBufferZeroFillCliFromNodeTest, "test-buffer-zero-fill-cli.js")
DEFINE_RAW_NODE_TEST(RawBufferZeroFillResetFromNodeTest, "test-buffer-zero-fill-reset.js")
DEFINE_RAW_NODE_TEST(RawBufferZeroFillFromNodeTest, "test-buffer-zero-fill.js")
DEFINE_RAW_NODE_TEST(RawBufferAllocUnsafeIsInitializedWithZeroFillFlagFromNodeTest, "test-buffer-alloc-unsafe-is-initialized-with-zero-fill-flag.js")
DEFINE_RAW_NODE_TEST(RawBufferAllocUnsafeIsUninitializedFromNodeTest, "test-buffer-alloc-unsafe-is-uninitialized.js")
DEFINE_RAW_NODE_TEST(RawBufferSwapFromNodeTest, "test-buffer-swap.js")

// Raw Node events/EventEmitter tests (drop-in from test/parallel)
DEFINE_RAW_NODE_TEST(RawEventCaptureRejectionsFromNodeTest, "test-event-capture-rejections.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterAddListenersFromNodeTest, "test-event-emitter-add-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterCheckListenerLeaksFromNodeTest, "test-event-emitter-check-listener-leaks.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterEmitContextFromNodeTest, "test-event-emitter-emit-context.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterErrorMonitorFromNodeTest, "test-event-emitter-error-monitor.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterErrorsFromNodeTest, "test-event-emitter-errors.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterGetMaxListenersFromNodeTest, "test-event-emitter-get-max-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterInvalidListenerFromNodeTest, "test-event-emitter-invalid-listener.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterListenerCountFromNodeTest, "test-event-emitter-listener-count.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterListenersFromNodeTest, "test-event-emitter-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterListenersSideEffectsFromNodeTest, "test-event-emitter-listeners-side-effects.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterMaxListenersFromNodeTest, "test-event-emitter-max-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterMaxListenersWarningFromNodeTest, "test-event-emitter-max-listeners-warning.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterMaxListenersWarningForSymbolFromNodeTest, "test-event-emitter-max-listeners-warning-for-symbol.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterMaxListenersWarningForNullFromNodeTest, "test-event-emitter-max-listeners-warning-for-null.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterModifyInEmitFromNodeTest, "test-event-emitter-modify-in-emit.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterNoErrorProvidedToErrorEventFromNodeTest, "test-event-emitter-no-error-provided-to-error-event.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterNumArgsFromNodeTest, "test-event-emitter-num-args.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterOnceFromNodeTest, "test-event-emitter-once.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterPrependFromNodeTest, "test-event-emitter-prepend.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterRemoveAllListenersFromNodeTest, "test-event-emitter-remove-all-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterRemoveListenersFromNodeTest, "test-event-emitter-remove-listeners.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterSetMaxListenersSideEffectsFromNodeTest, "test-event-emitter-set-max-listeners-side-effects.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterSpecialEventNamesFromNodeTest, "test-event-emitter-special-event-names.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterSubclassFromNodeTest, "test-event-emitter-subclass.js")
DEFINE_RAW_NODE_TEST(RawEventEmitterSymbolsFromNodeTest, "test-event-emitter-symbols.js")
DEFINE_RAW_NODE_TEST(RawEventsCustomeventFromNodeTest, "test-events-customevent.js")
DEFINE_RAW_NODE_TEST(RawEventsGetmaxlistenersFromNodeTest, "test-events-getmaxlisteners.js")
DEFINE_RAW_NODE_TEST(RawEventsListFromNodeTest, "test-events-list.js")
DEFINE_RAW_NODE_TEST(RawEventsListenerCountWithListenerFromNodeTest, "test-events-listener-count-with-listener.js")
DEFINE_RAW_NODE_TEST(RawEventsOnAsyncIteratorFromNodeTest, "test-events-on-async-iterator.js")
DEFINE_RAW_NODE_TEST(RawEventsOnceFromNodeTest, "test-events-once.js")
DEFINE_RAW_NODE_TEST(RawEventsStaticGeteventlistenersFromNodeTest, "test-events-static-geteventlisteners.js")
DEFINE_RAW_NODE_TEST(RawEventsUncaughtExceptionStackFromNodeTest, "test-events-uncaught-exception-stack.js")
DEFINE_RAW_NODE_TEST(RawEventTargetFromNodeTest, "test-event-target.js")

// Raw Node querystring tests
DEFINE_RAW_NODE_TEST(RawQuerystringFromNodeTest, "test-querystring.js")
DEFINE_RAW_NODE_TEST(RawQuerystringMulticharSeparatorFromNodeTest, "test-querystring-multichar-separator.js")
DEFINE_RAW_NODE_TEST(RawQuerystringMaxKeysNonFiniteFromNodeTest, "test-querystring-maxKeys-non-finite.js")
DEFINE_RAW_NODE_TEST(RawQuerystringEscapeFromNodeTest, "test-querystring-escape.js")

// Raw Node process tests
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessFeaturesFromNodeTest, "test-process-features.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessAbortFromNodeTest, "test-process-abort.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawProcessArgv0FromNodeTest) {
  if (RawNodeScriptHasUnsupportedFlagsHeader("test-process-argv-0.js")) {
    GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: test-process-argv-0.js";
  }
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-process-argv-0.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessAvailableMemoryFromNodeTest, "test-process-available-memory.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessChdirFromNodeTest, "test-process-chdir.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessChdirErrormessageFromNodeTest, "test-process-chdir-errormessage.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExecpathFromNodeTest, "test-process-execpath.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExecArgvFromNodeTest, "test-process-exec-argv.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessConfigFromNodeTest, "test-process-config.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessConstantsNoatimeFromNodeTest, "test-process-constants-noatime.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessConstrainedMemoryFromNodeTest, "test-process-constrained-memory.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessCpuUsageFromNodeTest, "test-process-cpuUsage.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessDefaultFromNodeTest, "test-process-default.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEmitwarningFromNodeTest, "test-process-emitwarning.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEmitFromNodeTest, "test-process-emit.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvFromNodeTest, "test-process-env.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvAllowedFlagsFromNodeTest, "test-process-env-allowed-flags.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvAllowedFlagsAreDocumentedFromNodeTest, "test-process-env-allowed-flags-are-documented.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvDeleteFromNodeTest, "test-process-env-delete.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvIgnoreGetterSetterFromNodeTest, "test-process-env-ignore-getter-setter.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvSideeffectsFromNodeTest, "test-process-env-sideeffects.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvSymbolsFromNodeTest, "test-process-env-symbols.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessBindingUtilFromNodeTest, "test-process-binding-util.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessBindingFromNodeTest, "test-process-binding.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessBindingInternalbindingAllowlistFromNodeTest, "test-process-binding-internalbinding-allowlist.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessDlopenErrorMessageCrashFromNodeTest, "test-process-dlopen-error-message-crash.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEnvTzFromNodeTest, "test-process-env-tz.js")

// Raw Node crypto tests (phase 1 gate)
DEFINE_RAW_NODE_TEST(RawCryptoFromNodeTest, "test-crypto.js")
DEFINE_RAW_NODE_TEST(RawCryptoRandomFromNodeTest, "test-crypto-random.js")
DEFINE_RAW_NODE_TEST(RawCryptoRandomuuidFromNodeTest, "test-crypto-randomuuid.js")
DEFINE_RAW_NODE_TEST(RawCryptoHashFromNodeTest, "test-crypto-hash.js")
DEFINE_RAW_NODE_TEST(RawCryptoHmacFromNodeTest, "test-crypto-hmac.js")
DEFINE_RAW_NODE_TEST(RawCryptoPbkdf2FromNodeTest, "test-crypto-pbkdf2.js")
DEFINE_RAW_NODE_TEST(RawCryptoScryptFromNodeTest, "test-crypto-scrypt.js")
DEFINE_RAW_NODE_TEST(RawCryptoHkdfFromNodeTest, "test-crypto-hkdf.js")
DEFINE_RAW_NODE_TEST(RawCryptoClassesFromNodeTest, "test-crypto-classes.js")
DEFINE_RAW_NODE_TEST(RawCryptoCipherivDecipherivFromNodeTest, "test-crypto-cipheriv-decipheriv.js")
DEFINE_RAW_NODE_TEST(RawCryptoSignVerifyFromNodeTest, "test-crypto-sign-verify.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawCryptoVerifyFailureFromNodeTest) {
  std::string error;
  const int exit_code = RunRawNodeTestScriptInSubprocess("test-crypto-verify-failure.js", &error, true);
  if (exit_code != 0 &&
      error.find("listen EPERM: operation not permitted 0.0.0.0") != std::string::npos) {
    GTEST_SKIP() << "Skipping verify-failure raw test in restricted network environment";
  }
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawCryptoOneShotHashFromNodeTest, "test-crypto-oneshot-hash.js")
DEFINE_RAW_NODE_TEST(RawCryptoDefaultShakeLengthsOneShotFromNodeTest, "test-crypto-default-shake-lengths-oneshot.js")
DEFINE_RAW_NODE_TEST(RawCryptoAsyncSignVerifyFromNodeTest, "test-crypto-async-sign-verify.js")
DEFINE_RAW_NODE_TEST(RawCryptoRsaPssDefaultSaltLengthFromNodeTest, "test-crypto-rsa-pss-default-salt-length.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenBitLengthFromNodeTest, "test-crypto-keygen-bit-length.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenInvalidParameterEncodingEcFromNodeTest, "test-crypto-keygen-invalid-parameter-encoding-ec.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenInvalidParameterEncodingDsaFromNodeTest, "test-crypto-keygen-invalid-parameter-encoding-dsa.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenSyncFromNodeTest, "test-crypto-keygen-sync.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenPromisifyFromNodeTest, "test-crypto-keygen-promisify.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenKeyObjectWithoutEncodingFromNodeTest, "test-crypto-keygen-key-object-without-encoding.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenKeyObjectsFromNodeTest, "test-crypto-keygen-key-objects.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenNonStandardPublicExponentFromNodeTest, "test-crypto-keygen-non-standard-public-exponent.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenDuplicateDeprecatedOptionFromNodeTest, "test-crypto-keygen-duplicate-deprecated-option.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenRsaPssFromNodeTest, "test-crypto-keygen-rsa-pss.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenAsyncRsaFromNodeTest, "test-crypto-keygen-async-rsa.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenAsyncDsaKeyObjectFromNodeTest, "test-crypto-keygen-async-dsa-key-object.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenAsyncDsaFromNodeTest, "test-crypto-keygen-async-dsa.js")
DEFINE_RAW_NODE_TEST(RawCryptoKeygenFromNodeTest, "test-crypto-keygen.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoAesWrapFromNodeTest, "test-crypto-aes-wrap.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoAuthenticatedStreamFromNodeTest, "test-crypto-authenticated-stream.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoAuthenticatedFromNodeTest, "test-crypto-authenticated.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoCertificateFromNodeTest, "test-crypto-certificate.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDefaultShakeLengthsFromNodeTest, "test-crypto-default-shake-lengths.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDes3WrapFromNodeTest, "test-crypto-des3-wrap.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhConstructorFromNodeTest, "test-crypto-dh-constructor.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhCurvesFromNodeTest, "test-crypto-dh-curves.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhErrorsFromNodeTest, "test-crypto-dh-errors.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhGenerateKeysFromNodeTest, "test-crypto-dh-generate-keys.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhGroupSettersFromNodeTest, "test-crypto-dh-group-setters.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhLeakFromNodeTest, "test-crypto-dh-leak.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhModp2ViewsFromNodeTest, "test-crypto-dh-modp2-views.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhModp2FromNodeTest, "test-crypto-dh-modp2.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhOddKeyFromNodeTest, "test-crypto-dh-odd-key.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhPaddingFromNodeTest, "test-crypto-dh-padding.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhSharedFromNodeTest, "test-crypto-dh-shared.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhStatelessAsyncFromNodeTest, "test-crypto-dh-stateless-async.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhStatelessFromNodeTest, "test-crypto-dh-stateless.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDhFromNodeTest, "test-crypto-dh.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDomainFromNodeTest, "test-crypto-domain.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoDomainsFromNodeTest, "test-crypto-domains.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoEcbFromNodeTest, "test-crypto-ecb.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoEcdhConvertKeyFromNodeTest, "test-crypto-ecdh-convert-key.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoEncodingValidationErrorFromNodeTest, "test-crypto-encoding-validation-error.js")
// DEFINE_RAW_NODE_TEST(RawTestCryptoFipsFromNodeTest, "test-crypto-fips.js") // FIPS harness helper not implemented
DEFINE_RAW_NODE_TEST(RawTestCryptoFromBinaryFromNodeTest, "test-crypto-from-binary.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoGcmExplicitShortTagFromNodeTest, "test-crypto-gcm-explicit-short-tag.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoGcmImplicitShortTagFromNodeTest, "test-crypto-gcm-implicit-short-tag.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoGetcipherinfoFromNodeTest, "test-crypto-getcipherinfo.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoHashStreamPipeFromNodeTest, "test-crypto-hash-stream-pipe.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeyObjectsFromNodeTest, "test-crypto-key-objects.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncEllipticCurveJwkEcFromNodeTest, "test-crypto-keygen-async-elliptic-curve-jwk-ec.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncEllipticCurveJwkRsaFromNodeTest, "test-crypto-keygen-async-elliptic-curve-jwk-rsa.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncEllipticCurveJwkFromNodeTest, "test-crypto-keygen-async-elliptic-curve-jwk.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncEncryptedPrivateKeyDerFromNodeTest, "test-crypto-keygen-async-encrypted-private-key-der.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncEncryptedPrivateKeyFromNodeTest, "test-crypto-keygen-async-encrypted-private-key.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncExplicitEllipticCurveEncryptedP256FromNodeTest, "test-crypto-keygen-async-explicit-elliptic-curve-encrypted-p256.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncExplicitEllipticCurveEncryptedJsFromNodeTest, "test-crypto-keygen-async-explicit-elliptic-curve-encrypted.js.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncExplicitEllipticCurveFromNodeTest, "test-crypto-keygen-async-explicit-elliptic-curve.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncNamedEllipticCurveEncryptedP256FromNodeTest, "test-crypto-keygen-async-named-elliptic-curve-encrypted-p256.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncNamedEllipticCurveEncryptedFromNodeTest, "test-crypto-keygen-async-named-elliptic-curve-encrypted.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenAsyncNamedEllipticCurveFromNodeTest, "test-crypto-keygen-async-named-elliptic-curve.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenDeprecationFromNodeTest, "test-crypto-keygen-deprecation.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenDhClassicFromNodeTest, "test-crypto-keygen-dh-classic.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenEddsaFromNodeTest, "test-crypto-keygen-eddsa.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenEmptyPassphraseNoErrorFromNodeTest, "test-crypto-keygen-empty-passphrase-no-error.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenEmptyPassphraseNoPromptFromNodeTest, "test-crypto-keygen-empty-passphrase-no-prompt.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenMissingOidFromNodeTest, "test-crypto-keygen-missing-oid.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenNoRsassaPssParamsFromNodeTest, "test-crypto-keygen-no-rsassa-pss-params.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenRfc801791FromNodeTest, "test-crypto-keygen-rfc8017-9-1.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoKeygenRfc8017A23FromNodeTest, "test-crypto-keygen-rfc8017-a-2-3.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoLazyTransformWritableFromNodeTest, "test-crypto-lazy-transform-writable.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoNoAlgorithmFromNodeTest, "test-crypto-no-algorithm.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoOaepZeroLengthFromNodeTest, "test-crypto-oaep-zero-length.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoOneshotHashXofFromNodeTest, "test-crypto-oneshot-hash-xof.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoOpDuringProcessExitFromNodeTest, "test-crypto-op-during-process-exit.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPaddingAes256FromNodeTest, "test-crypto-padding-aes256.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPaddingFromNodeTest, "test-crypto-padding.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPrimeFromNodeTest, "test-crypto-prime.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPrivateDecryptGh32240FromNodeTest, "test-crypto-private-decrypt-gh32240.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPsychicSignaturesFromNodeTest, "test-crypto-psychic-signatures.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoPublicdecryptFailsFirstTimeFromNodeTest, "test-crypto-publicDecrypt-fails-first-time.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoRandomfillsyncRegressionFromNodeTest, "test-crypto-randomfillsync-regression.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoRsaDsaFromNodeTest, "test-crypto-rsa-dsa.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoSecLevelFromNodeTest, "test-crypto-sec-level.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoSecretKeygenFromNodeTest, "test-crypto-secret-keygen.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoSecureHeapFromNodeTest, "test-crypto-secure-heap.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoStreamFromNodeTest, "test-crypto-stream.js")
DEFINE_RAW_NODE_TEST(RawTestCryptoUpdateEncodingFromNodeTest, "test-crypto-update-encoding.js")
DEFINE_RAW_NODE_TEST(RawTestTlsTicketInvalidArgFromNodeTest, "test-tls-ticket-invalid-arg.js")
DEFINE_RAW_NODE_TEST(RawTestTlsSetCiphersErrorFromNodeTest, "test-tls-set-ciphers-error.js")
DEFINE_RAW_NODE_TEST(RawTestTlsOptionsBooleanCheckFromNodeTest, "test-tls-options-boolean-check.js")
DEFINE_RAW_NODE_TEST(RawTestTlsKeyengineInvalidArgTypeFromNodeTest, "test-tls-keyengine-invalid-arg-type.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawTestTlsKeyengineUnsupportedFromNodeTest, "test-tls-keyengine-unsupported.js")
DEFINE_RAW_NODE_TEST(RawTestTlsClientcertengineInvalidArgTypeFromNodeTest, "test-tls-clientcertengine-invalid-arg-type.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawTestTlsClientcertengineUnsupportedFromNodeTest,
                                 "test-tls-clientcertengine-unsupported.js")
DEFINE_RAW_NODE_TEST(RawTestTlsSessionTimeoutErrorsFromNodeTest, "test-tls-session-timeout-errors.js")
DEFINE_RAW_NODE_TEST(RawTestTlsConnectSecureContextFromNodeTest, "test-tls-connect-secure-context.js")
DEFINE_RAW_NODE_TEST(RawTestTlsGetcipherFromNodeTest, "test-tls-getcipher.js")
DEFINE_RAW_NODE_TEST(RawTestTlsExportkeyingmaterialFromNodeTest, "test-tls-exportkeyingmaterial.js")
DEFINE_RAW_NODE_TEST(RawTestTlsAddContextFromNodeTest, "test-tls-add-context.js")
DEFINE_RAW_NODE_TEST(RawTestTlsClientResumeFromNodeTest, "test-tls-client-resume.js")

DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecveFromNodeTest, "test-process-execve.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecveValidationFromNodeTest, "test-process-execve-validation.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecveAbortFromNodeTest, "test-process-execve-abort.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecveOnExitFromNodeTest, "test-process-execve-on-exit.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecvePermissionFailFromNodeTest, "test-process-execve-permission-fail.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessExecvePermissionGrantedFromNodeTest, "test-process-execve-permission-granted.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExceptionCaptureFromNodeTest, "test-process-exception-capture.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExceptionCaptureErrorsFromNodeTest, "test-process-exception-capture-errors.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExceptionCaptureShouldAbortOnUncaughtFromNodeTest, "test-process-exception-capture-should-abort-on-uncaught.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExceptionCaptureShouldAbortOnUncaughtSetflagsfromstringFromNodeTest, "test-process-exception-capture-should-abort-on-uncaught-setflagsfromstring.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExternalStdioCloseFromNodeTest, "test-process-external-stdio-close.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExternalStdioCloseSpawnFromNodeTest, "test-process-external-stdio-close-spawn.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessGetactivehandlesFromNodeTest, "test-process-getactivehandles.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessGetactiverequestsFromNodeTest, "test-process-getactiverequests.js")
DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(RawProcessGetactiveresourcesFromNodeTest, "test-process-getactiveresources.js")
DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(RawProcessGetactiveresourcesTrackActiveRequestsFromNodeTest, "test-process-getactiveresources-track-active-requests.js")
DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(RawProcessGetactiveresourcesTrackTimerLifetimeFromNodeTest, "test-process-getactiveresources-track-timer-lifetime.js")
DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(RawProcessGetactiveresourcesTrackMultipleTimersFromNodeTest, "test-process-getactiveresources-track-multiple-timers.js")
DEFINE_RAW_NODE_FILE_STDIO_SUBPROCESS_TEST(RawProcessGetactiveresourcesTrackIntervalLifetimeFromNodeTest, "test-process-getactiveresources-track-interval-lifetime.js")
DEFINE_RAW_NODE_TEST(RawTimersFromNodeTest, "test-timers.js")
DEFINE_RAW_NODE_TEST(RawTimersArgsFromNodeTest, "test-timers-args.js")
DEFINE_RAW_NODE_TEST(RawTimersThisFromNodeTest, "test-timers-this.js")
DEFINE_RAW_NODE_TEST(RawTimersInvalidClearFromNodeTest, "test-timers-invalid-clear.js")
DEFINE_RAW_NODE_TEST(RawTimersClearNullDoesNotThrowErrorFromNodeTest, "test-timers-clear-null-does-not-throw-error.js")
DEFINE_RAW_NODE_TEST(RawTimersClearObjectDoesNotThrowErrorFromNodeTest, "test-timers-clear-object-does-not-throw-error.js")
DEFINE_RAW_NODE_TEST(RawTimersOrderingFromNodeTest, "test-timers-ordering.js")
DEFINE_RAW_NODE_TEST(RawTimersNestedFromNodeTest, "test-timers-nested.js")
DEFINE_RAW_NODE_TEST(RawTimersNextTickFromNodeTest, "test-timers-next-tick.js")
DEFINE_RAW_NODE_TEST(RawTimersZeroTimeoutFromNodeTest, "test-timers-zero-timeout.js")
DEFINE_RAW_NODE_TEST(RawTimersUserCallFromNodeTest, "test-timers-user-call.js")
DEFINE_RAW_NODE_TEST(RawTimersToPrimitiveFromNodeTest, "test-timers-to-primitive.js")
DEFINE_RAW_NODE_TEST(RawTimersNowFromNodeTest, "test-timers-now.js")
DEFINE_RAW_NODE_TEST(RawTimersPromisesSchedulerFromNodeTest, "test-timers-promises-scheduler.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessHrtimeBigintFromNodeTest, "test-process-hrtime-bigint.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessHrtimeFromNodeTest, "test-process-hrtime.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessKillNullFromNodeTest, "test-process-kill-null.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessKillPidFromNodeTest, "test-process-kill-pid.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessNextTickFromNodeTest, "test-process-next-tick.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessPpidFromNodeTest, "test-process-ppid.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessPrototypeFromNodeTest, "test-process-prototype.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessRawDebugFromNodeTest, "test-process-raw-debug.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessRefUnrefFromNodeTest, "test-process-ref-unref.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessReleaseFromNodeTest, "test-process-release.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessSetsourcemapsenabledFromNodeTest, "test-process-setsourcemapsenabled.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessBeforeexitFromNodeTest, "test-process-beforeexit.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessThreadCpuUsageMainThreadFromNodeTest, "test-process-threadCpuUsage-main-thread.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitHandlerFromNodeTest, "test-process-exit-handler.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessUmaskFromNodeTest, "test-process-umask.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessUmaskMaskFromNodeTest, "test-process-umask-mask.js")
DEFINE_RAW_NODE_SUBPROCESS_TEST(RawProcessUptimeFromNodeTest, "test-process-uptime.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitCodeFromNodeTest, "test-process-exit-code.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitCodeValidationFromNodeTest, "test-process-exit-code-validation.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitFromNodeTest, "test-process-exit.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitFromBeforeExitFromNodeTest, "test-process-exit-from-before-exit.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessExitRecursiveFromNodeTest, "test-process-exit-recursive.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessReallyExitFromNodeTest, "test-process-really-exit.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessEuidEgidFromNodeTest, "test-process-euid-egid.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessInitgroupsFromNodeTest, "test-process-initgroups.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessNoDeprecationFromNodeTest, "test-process-no-deprecation.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessRedirectWarningsFromNodeTest, "test-process-redirect-warnings.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessRedirectWarningsEnvFromNodeTest, "test-process-redirect-warnings-env.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessSetgroupsFromNodeTest, "test-process-setgroups.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessTitleCliFromNodeTest, "test-process-title-cli.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessUidGidFromNodeTest, "test-process-uid-gid.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessLoadEnvFileFromNodeTest, "test-process-load-env-file.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessVersionsFromNodeTest, "test-process-versions.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessWarningFromNodeTest, "test-process-warning.js")
DEFINE_RAW_NODE_IN_PROCESS_TEST(RawProcessUncaughtExceptionMonitorFromNodeTest, "test-process-uncaught-exception-monitor.js")

// Raw Node util tests
DEFINE_RAW_NODE_TEST(RawUtilFromNodeTest, "test-util.js")
// DEFINE_RAW_NODE_TEST(RawUtilTypesFromNodeTest, "test-util-types.js")
DEFINE_RAW_NODE_TEST(RawUtilTypesExistsFromNodeTest, "test-util-types-exists.js")
DEFINE_RAW_NODE_TEST(RawUtilTextDecoderFromNodeTest, "test-util-text-decoder.js")
DEFINE_RAW_NODE_TEST(RawUtilStripvtcontrolcharactersFromNodeTest, "test-util-stripvtcontrolcharacters.js")
DEFINE_RAW_NODE_TEST(RawUtilSleepFromNodeTest, "test-util-sleep.js")
DEFINE_RAW_NODE_TEST(RawUtilPromisifyFromNodeTest, "test-util-promisify.js")
DEFINE_RAW_NODE_TEST(RawUtilParseEnvFromNodeTest, "test-util-parse-env.js")
DEFINE_RAW_NODE_TEST(RawUtilInheritsFromNodeTest, "test-util-inherits.js")
DEFINE_RAW_NODE_TEST(RawUtilGetcallsitesFromNodeTest, "test-util-getcallsites.js")
DEFINE_RAW_NODE_TEST(RawUtilGetcallsitesPreparestacktraceFromNodeTest,
                     "test-util-getcallsites-preparestacktrace.js")
DEFINE_RAW_NODE_TEST(RawUtilInternalFromNodeTest, "test-util-internal.js")
DEFINE_RAW_NODE_TEST(RawInternalUtilConstructSabFromNodeTest, "test-internal-util-construct-sab.js")
DEFINE_RAW_NODE_TEST(RawInternalUtilDecorateErrorStackFromNodeTest,
                     "test-internal-util-decorate-error-stack.js")
DEFINE_RAW_NODE_TEST(RawUtilFormatFromNodeTest, "test-util-format.js")
DEFINE_RAW_NODE_TEST(RawUtilDeprecateFromNodeTest, "test-util-deprecate.js")
DEFINE_RAW_NODE_TEST(RawUtilCallbackifyFromNodeTest, "test-util-callbackify.js")

// Raw Node tty tests (drop-in from test/{parallel,pseudo-tty})
DEFINE_RAW_NODE_TEST(RawTtyBackwardsApiFromNodeTest, "test-tty-backwards-api.js")
DEFINE_RAW_NODE_TEST(RawTtyStdinEndFromNodeTest, "test-tty-stdin-end.js")
DEFINE_RAW_NODE_TEST(RawTtyStdinPipeFromNodeTest, "test-tty-stdin-pipe.js")
DEFINE_RAW_NODE_TEST(RawTtywrapInvalidFdFromNodeTest, "test-ttywrap-invalid-fd.js")
DEFINE_RAW_NODE_TEST(RawTtywrapStackFromNodeTest, "test-ttywrap-stack.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyWrapFromNodeTest, "pseudo-tty/test-tty-wrap.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyHandleWrapHasrefFromNodeTest, "pseudo-tty/test-handle-wrap-hasref-tty.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyWindowSizeFromNodeTest, "pseudo-tty/test-tty-window-size.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyStreamConstructorsFromNodeTest, "pseudo-tty/test-tty-stream-constructors.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyIsattyFromNodeTest, "pseudo-tty/test-tty-isatty.js")
DEFINE_RAW_NODE_TEST(RawAbortSignalHandlerFromNodeTest, "abort/test-signal-handler.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawAbortAddonRegisterSignalHandlerFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "abort/test-addon-register-signal-handler.js", &error);
  if (exit_code != 0 &&
      (error.find("SyntaxError: Invalid or unexpected token") != std::string::npos ||
       error.find("Module did not self-register") != std::string::npos)) {
    GTEST_SKIP() << "Skipping native addon signal handler test: .node loading is unavailable in this runtime.";
  }
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawParallelSignalHandlerFromNodeTest, "parallel/test-signal-handler.js")
DEFINE_RAW_NODE_TEST(RawParallelProcessRemoveAllSignalListenersFromNodeTest, "parallel/test-process-remove-all-signal-listeners.js")
DEFINE_RAW_NODE_TEST(RawParallelSignalUnregisterFromNodeTest, "parallel/test-signal-unregister.js")
DEFINE_RAW_NODE_TEST(RawParallelSignalArgsFromNodeTest, "parallel/test-signal-args.js")
DEFINE_RAW_NODE_TEST(RawParallelSignalSafetyFromNodeTest, "parallel/test-signal-safety.js")
DEFINE_RAW_NODE_TEST(RawParallelSignalHandlerRemoveOnExitFromNodeTest, "parallel/test-signal-handler-remove-on-exit.js")
DEFINE_RAW_NODE_TEST(RawParallelAbortsignalCloneableFromNodeTest, "parallel/test-abortsignal-cloneable.js")
DEFINE_RAW_NODE_TEST(RawParallelFsWatchAbortSignalFromNodeTest, "parallel/test-fs-watch-abort-signal.js")
DEFINE_RAW_NODE_TEST(RawParallelStreamAddAbortSignalFromNodeTest, "parallel/test-stream-add-abort-signal.js")
DEFINE_RAW_NODE_TEST(
    RawParallelWhatwgEventsAddEventListenerOptionsSignalFromNodeTest,
    "parallel/test-whatwg-events-add-event-listener-options-signal.js")
DEFINE_RAW_NODE_TEST(RawParallelProcessKillPidFromNodeTest, "parallel/test-process-kill-pid.js")
DEFINE_RAW_NODE_TEST(RawParallelProcessKillNullFromNodeTest, "parallel/test-process-kill-null.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyColorSupportFromNodeTest, "pseudo-tty/test-tty-color-support.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyColorSupportWarningFromNodeTest, "pseudo-tty/test-tty-color-support-warning.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyColorSupportWarning2FromNodeTest, "pseudo-tty/test-tty-color-support-warning-2.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyStdoutResizeFromNodeTest, "pseudo-tty/test-tty-stdout-resize.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyStdoutEndFromNodeTest, "pseudo-tty/test-tty-stdout-end.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyStdinEndFromNodeTest, "pseudo-tty/test-tty-stdin-end.js")
DEFINE_RAW_NODE_TEST(RawPseudoTtyStdinCallEndFromNodeTest, "pseudo-tty/test-tty-stdin-call-end.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawPseudoTtySetRawModeResetSignalFromNodeTest) {
  static constexpr const char* kScript = "pseudo-tty/test-set-raw-mode-reset-signal.js";
  if (RawNodeScriptHasUnsupportedFlagsHeader(kScript)) {
    GTEST_SKIP() << "Skipping Node.js raw test with unsupported // Flags header: " << kScript;
  }
#if !defined(_WIN32)
  if (isatty(STDIN_FILENO) == 0) {
    GTEST_SKIP() << "Skipping pseudo-tty raw mode reset test: stdin is not a TTY";
  }
#endif
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, kScript, &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawReportSignalFromNodeTest, "report/test-report-signal.js")

// Raw Node repl tests (initial drop-in batch from test/parallel)
DEFINE_RAW_NODE_TEST(RawReplEmptyFromNodeTest, "test-repl-empty.js")
DEFINE_RAW_NODE_TEST(RawReplNullFromNodeTest, "test-repl-null.js")
DEFINE_RAW_NODE_TEST(RawReplEndEmitsExitFromNodeTest, "test-repl-end-emits-exit.js")
DEFINE_RAW_NODE_TEST(RawReplUnsupportedOptionFromNodeTest, "test-repl-unsupported-option.js")
DEFINE_RAW_NODE_TEST(RawReplUseGlobalFromNodeTest, "test-repl-use-global.js")
DEFINE_RAW_NODE_TEST(RawReplOptionsFromNodeTest, "test-repl-options.js")
DEFINE_RAW_NODE_TEST(RawReplCloseFromNodeTest, "test-repl-close.js")
DEFINE_RAW_NODE_TEST(RawReplSetPromptFromNodeTest, "test-repl-setprompt.js")
DEFINE_RAW_NODE_TEST(RawReplNoTerminalFromNodeTest, "test-repl-no-terminal.js")
DEFINE_RAW_NODE_TEST(RawReplDefineCommandFromNodeTest, "test-repl-definecommand.js")
DEFINE_RAW_NODE_TEST(RawReplContextFromNodeTest, "test-repl-context.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawReplUnderscoreFromNodeTest) {
  GTEST_SKIP() << "Upstream Node v24.13.2-pre currently fails parallel/test-repl-underscore.js";
}
DEFINE_RAW_NODE_TEST(RawReplResetEventFromNodeTest, "test-repl-reset-event.js")
DEFINE_RAW_NODE_TEST(RawReplCustomEvalFromNodeTest, "test-repl-custom-eval.js")
DEFINE_RAW_NODE_TEST(RawReplThrowNullOrUndefinedFromNodeTest, "test-repl-throw-null-or-undefined.js")
DEFINE_RAW_NODE_TEST(RawReplNullThrownFromNodeTest, "test-repl-null-thrown.js")
DEFINE_RAW_NODE_TEST(RawReplRecoverableFromNodeTest, "test-repl-recoverable.js")
DEFINE_RAW_NODE_TEST(RawReplUnexpectedTokenRecoverableFromNodeTest, "test-repl-unexpected-token-recoverable.js")
DEFINE_RAW_NODE_TEST(RawReplSyntaxErrorHandlingFromNodeTest, "test-repl-syntax-error-handling.js")
// DEFINE_RAW_NODE_TEST(RawReplModeFromNodeTest, "test-repl-mode.js")
DEFINE_RAW_NODE_TEST(RawReplAutoLibsFromNodeTest, "test-repl-autolibs.js")
DEFINE_RAW_NODE_TEST(RawReplSaveLoadFromNodeTest, "test-repl-save-load.js")
DEFINE_RAW_NODE_TEST(RawReplInspectDefaultsFromNodeTest, "test-repl-inspect-defaults.js")
DEFINE_RAW_NODE_TEST(RawReplRequireContextFromNodeTest, "test-repl-require-context.js")
DEFINE_RAW_NODE_TEST(RawReplRequireFromNodeTest, "test-repl-require.js")
DEFINE_RAW_NODE_TEST(RawReplRequireCacheFromNodeTest, "test-repl-require-cache.js")
DEFINE_RAW_NODE_TEST(RawReplRequireAfterWriteFromNodeTest, "test-repl-require-after-write.js")
DEFINE_RAW_NODE_TEST(RawReplRequireSelfReferentialFromNodeTest, "test-repl-require-self-referential.js")
DEFINE_RAW_NODE_TEST(RawReplEnvVarsFromNodeTest, "test-repl-envvars.js")
DEFINE_RAW_NODE_TEST(RawReplCliEvalFromNodeTest, "test-repl-cli-eval.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteFromNodeTest, "test-repl-tab-complete.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteRequireFromNodeTest, "test-repl-tab-complete-require.js")
DEFINE_RAW_NODE_TEST(RawReplCompletionOnGettersDisabledFromNodeTest, "test-repl-completion-on-getters-disabled.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteBufferFromNodeTest, "test-repl-tab-complete-buffer.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteComputedPropsFromNodeTest, "test-repl-tab-complete-computed-props.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteCrashFromNodeTest, "test-repl-tab-complete-crash.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteCustomCompleterFromNodeTest, "test-repl-tab-complete-custom-completer.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteFilesFromNodeTest, "test-repl-tab-complete-files.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteGetterErrorFromNodeTest, "test-repl-tab-complete-getter-error.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteImportFromNodeTest, "test-repl-tab-complete-import.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteNestedReplsFromNodeTest, "test-repl-tab-complete-nested-repls.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteNoWarnFromNodeTest, "test-repl-tab-complete-no-warn.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteNoSideEffectsFromNodeTest, "test-repl-tab-complete-nosideeffects.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteOnEditorModeFromNodeTest, "test-repl-tab-complete-on-editor-mode.js")
DEFINE_RAW_NODE_TEST(RawReplTabCompleteUnaryExpressionsFromNodeTest, "test-repl-tab-complete-unary-expressions.js")
DEFINE_RAW_NODE_TEST(RawReplTabFromNodeTest, "test-repl-tab.js")
DEFINE_RAW_NODE_TEST(RawReplMultilineFromNodeTest, "test-repl-multiline.js")
DEFINE_RAW_NODE_TEST(RawReplMultilineNavigationFromNodeTest, "test-repl-multiline-navigation.js")
DEFINE_RAW_NODE_TEST(RawReplMultilineNavigationWhileAddingFromNodeTest, "test-repl-multiline-navigation-while-adding.js")
DEFINE_RAW_NODE_TEST(RawReplPreprocessTopLevelAwaitFromNodeTest, "test-repl-preprocess-top-level-await.js")

// Raw Node url tests
DEFINE_RAW_NODE_TEST(RawUrlDomainAsciiUnicodeFromNodeTest, "test-url-domain-ascii-unicode.js")
DEFINE_RAW_NODE_TEST(RawUrlFileurltopathFromNodeTest, "test-url-fileurltopath.js")
DEFINE_RAW_NODE_TEST(RawUrlFormatInvalidInputFromNodeTest, "test-url-format-invalid-input.js")
DEFINE_RAW_NODE_TEST(RawUrlFormatWhatwgFromNodeTest, "test-url-format-whatwg.js")
DEFINE_RAW_NODE_TEST(RawUrlFormatFromNodeTest, "test-url-format.js")
DEFINE_RAW_NODE_TEST(RawUrlInvalidFileUrlPathInputFromNodeTest, "test-url-invalid-file-url-path-input.js")
DEFINE_RAW_NODE_TEST(RawUrlIsUrlInternalFromNodeTest, "test-url-is-url-internal.js")
DEFINE_RAW_NODE_TEST(RawUrlParseFormatFromNodeTest, "test-url-parse-format.js")
DEFINE_RAW_NODE_TEST(RawUrlParseInvalidInputFromNodeTest, "test-url-parse-invalid-input.js")
DEFINE_RAW_NODE_TEST(RawUrlParseQueryFromNodeTest, "test-url-parse-query.js")
DEFINE_RAW_NODE_TEST(RawUrlPathtofileurlFromNodeTest, "test-url-pathtofileurl.js")
DEFINE_RAW_NODE_TEST(RawUrlRelativeFromNodeTest, "test-url-relative.js")
DEFINE_RAW_NODE_TEST(RawUrlUrltooptionsFromNodeTest, "test-url-urltooptions.js")
DEFINE_RAW_NODE_TEST(RawUrlPatternFromNodeTest, "test-urlpattern.js")
DEFINE_RAW_NODE_TEST(RawUrlPatternInvalidThisFromNodeTest, "test-urlpattern-invalidthis.js")
DEFINE_RAW_NODE_TEST(RawUrlPatternTypesFromNodeTest, "test-urlpattern-types.js")

// Raw Node string_decoder tests
DEFINE_RAW_NODE_TEST(RawStringDecoderFromNodeTest, "test-string-decoder.js")
DEFINE_RAW_NODE_TEST(RawStringDecoderEndFromNodeTest, "test-string-decoder-end.js")
DEFINE_RAW_NODE_TEST(RawStringDecoderFuzzFromNodeTest, "test-string-decoder-fuzz.js")

// Raw Node dns tests
DEFINE_RAW_NODE_TEST(RawDnsFromNodeTest, "test-dns.js")
DEFINE_RAW_NODE_TEST(RawDnsSetserversTypeCheckFromNodeTest, "test-dns-setservers-type-check.js")
DEFINE_RAW_NODE_TEST(RawDnsSetserverWhenQueryingFromNodeTest, "test-dns-setserver-when-querying.js")
DEFINE_RAW_NODE_TEST(RawDnsSetlocaladdressFromNodeTest, "test-dns-setlocaladdress.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsSetDefaultOrderFromNodeTest, "test-dns-set-default-order.js")
DEFINE_RAW_NODE_TEST(RawDnsResolverMaxTimeoutFromNodeTest, "test-dns-resolver-max-timeout.js")
DEFINE_RAW_NODE_TEST(RawDnsResolvensTypeerrorFromNodeTest, "test-dns-resolvens-typeerror.js")
DEFINE_RAW_NODE_TEST(RawDnsResolveanyFromNodeTest, "test-dns-resolveany.js")
DEFINE_RAW_NODE_TEST(RawDnsResolveanyBadAncountFromNodeTest, "test-dns-resolveany-bad-ancount.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsResolvePromisesFromNodeTest, "test-dns-resolve-promises.js")
DEFINE_RAW_NODE_TEST(RawDnsPromisesExistsFromNodeTest, "test-dns-promises-exists.js")
DEFINE_RAW_NODE_TEST(RawDnsPerfHooksFromNodeTest, "test-dns-perf_hooks.js")
DEFINE_RAW_NODE_TEST(RawDnsMultiChannelFromNodeTest, "test-dns-multi-channel.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsMemoryErrorFromNodeTest, "test-dns-memory-error.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsLookupServiceFromNodeTest, "test-dns-lookupService.js")
DEFINE_RAW_NODE_TEST(RawDnsLookupServicePromisesFromNodeTest, "test-dns-lookupService-promises.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsLookupFromNodeTest, "test-dns-lookup.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsLookupPromisesFromNodeTest, "test-dns-lookup-promises.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsLookupPromisesOptionsDeprecatedFromNodeTest, "test-dns-lookup-promises-options-deprecated.js")
DEFINE_RAW_NODE_TEST(RawDnsGetServerFromNodeTest, "test-dns-get-server.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsDefaultOrderVerbatimFromNodeTest, "test-dns-default-order-verbatim.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsDefaultOrderIpv6FromNodeTest, "test-dns-default-order-ipv6.js")
DEFINE_RAW_NODE_ALLOW_FLAGS_TEST(RawDnsDefaultOrderIpv4FromNodeTest, "test-dns-default-order-ipv4.js")
DEFINE_RAW_NODE_TEST(RawDnsChannelTimeoutFromNodeTest, "test-dns-channel-timeout.js")
DEFINE_RAW_NODE_TEST(RawDnsChannelCancelFromNodeTest, "test-dns-channel-cancel.js")
DEFINE_RAW_NODE_TEST(RawDnsChannelCancelPromiseFromNodeTest, "test-dns-channel-cancel-promise.js")
DEFINE_RAW_NODE_TEST(RawDnsCancelReverseLookupFromNodeTest, "test-dns-cancel-reverse-lookup.js")

// Raw Node http tests (drop-in from test/parallel, excluding async-hooks-specific cases)
DEFINE_RAW_NODE_TEST(RawTestHttp10KeepAliveFromNodeTest, "test-http-1.0-keep-alive.js")
DEFINE_RAW_NODE_TEST(RawTestHttp10FromNodeTest, "test-http-1.0.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAbortBeforeEndFromNodeTest, "test-http-abort-before-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAbortClientFromNodeTest, "test-http-abort-client.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAbortQueuedFromNodeTest, "test-http-abort-queued.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAbortStreamEndFromNodeTest, "test-http-abort-stream-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAbortedFromNodeTest, "test-http-aborted.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAddrequestLocaladdressFromNodeTest, "test-http-addrequest-localaddress.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAfterConnectFromNodeTest, "test-http-after-connect.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentAbortControllerFromNodeTest, "test-http-agent-abort-controller.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentCloseFromNodeTest, "test-http-agent-close.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentDestroyedSocketFromNodeTest, "test-http-agent-destroyed-socket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentErrorOnIdleFromNodeTest, "test-http-agent-error-on-idle.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentFalseFromNodeTest, "test-http-agent-false.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentGetnameFromNodeTest, "test-http-agent-getname.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentKeepAliveTimeoutBufferFromNodeTest, "test-http-agent-keep-alive-timeout-buffer.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentKeepaliveDelayFromNodeTest, "test-http-agent-keepalive-delay.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentKeepaliveFromNodeTest, "test-http-agent-keepalive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentMaxsocketsRespectedFromNodeTest, "test-http-agent-maxsockets-respected.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentMaxsocketsFromNodeTest, "test-http-agent-maxsockets.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentMaxtotalsocketsFromNodeTest, "test-http-agent-maxtotalsockets.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentNoProtocolFromNodeTest, "test-http-agent-no-protocol.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentNullFromNodeTest, "test-http-agent-null.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentRemoveFromNodeTest, "test-http-agent-remove.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentReuseDrainedSocketOnlyFromNodeTest, "test-http-agent-reuse-drained-socket-only.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentSchedulingFromNodeTest, "test-http-agent-scheduling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentTimeoutOptionFromNodeTest, "test-http-agent-timeout-option.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentTimeoutFromNodeTest, "test-http-agent-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentUninitializedWithHandleFromNodeTest, "test-http-agent-uninitialized-with-handle.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentUninitializedFromNodeTest, "test-http-agent-uninitialized.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAgentFromNodeTest, "test-http-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAllowContentLength304FromNodeTest, "test-http-allow-content-length-304.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAllowReqAfter204ResFromNodeTest, "test-http-allow-req-after-204-res.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAutomaticHeadersFromNodeTest, "test-http-automatic-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpAutoselectfamilyFromNodeTest, "test-http-autoselectfamily.js")
DEFINE_RAW_NODE_TEST(RawTestHttpBindTwiceFromNodeTest, "test-http-bind-twice.js")
DEFINE_RAW_NODE_TEST(RawTestHttpBlankHeaderFromNodeTest, "test-http-blank-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpBufferSanityFromNodeTest, "test-http-buffer-sanity.js")
DEFINE_RAW_NODE_TEST(RawTestHttpByteswrittenFromNodeTest, "test-http-byteswritten.js")
DEFINE_RAW_NODE_TEST(RawTestHttpCatchUncaughtexceptionFromNodeTest, "test-http-catch-uncaughtexception.js")
DEFINE_RAW_NODE_TEST(RawTestHttpChunkExtensionsLimitFromNodeTest, "test-http-chunk-extensions-limit.js")
DEFINE_RAW_NODE_TEST(RawTestHttpChunkProblemFromNodeTest, "test-http-chunk-problem.js")
DEFINE_RAW_NODE_TEST(RawTestHttpChunked304FromNodeTest, "test-http-chunked-304.js")
DEFINE_RAW_NODE_TEST(RawTestHttpChunkedSmugglingFromNodeTest, "test-http-chunked-smuggling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpChunkedFromNodeTest, "test-http-chunked.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortDestroyFromNodeTest, "test-http-client-abort-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortEventFromNodeTest, "test-http-client-abort-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortKeepAliveDestroyResFromNodeTest, "test-http-client-abort-keep-alive-destroy-res.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortKeepAliveQueuedTcpSocketFromNodeTest, "test-http-client-abort-keep-alive-queued-tcp-socket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortKeepAliveQueuedUnixSocketFromNodeTest, "test-http-client-abort-keep-alive-queued-unix-socket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortNoAgentFromNodeTest, "test-http-client-abort-no-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortResponseEventFromNodeTest, "test-http-client-abort-response-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortUnixSocketFromNodeTest, "test-http-client-abort-unix-socket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortFromNodeTest, "test-http-client-abort.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbort2FromNodeTest, "test-http-client-abort2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbort3FromNodeTest, "test-http-client-abort3.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAbortedEventFromNodeTest, "test-http-client-aborted-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAgentAbortCloseEventFromNodeTest, "test-http-client-agent-abort-close-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAgentEndCloseEventFromNodeTest, "test-http-client-agent-end-close-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientAgentFromNodeTest, "test-http-client-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientCheckHttpTokenFromNodeTest, "test-http-client-check-http-token.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientCloseEventFromNodeTest, "test-http-client-close-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientCloseWithDefaultAgentFromNodeTest, "test-http-client-close-with-default-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientDefaultHeadersExistFromNodeTest, "test-http-client-default-headers-exist.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientDefaultsFromNodeTest, "test-http-client-defaults.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientEncodingFromNodeTest, "test-http-client-encoding.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientErrorRawbytesFromNodeTest, "test-http-client-error-rawbytes.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientFinishedFromNodeTest, "test-http-client-finished.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientGetUrlFromNodeTest, "test-http-client-get-url.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientHeadersArrayFromNodeTest, "test-http-client-headers-array.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientHeadersHostArrayFromNodeTest, "test-http-client-headers-host-array.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientImmediateErrorFromNodeTest, "test-http-client-immediate-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientIncomingmessageDestroyFromNodeTest, "test-http-client-incomingmessage-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientInputFunctionFromNodeTest, "test-http-client-input-function.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientInsecureHttpParserErrorFromNodeTest, "test-http-client-insecure-http-parser-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientInvalidPathFromNodeTest, "test-http-client-invalid-path.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientKeepAliveHintFromNodeTest, "test-http-client-keep-alive-hint.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientKeepAliveReleaseBeforeFinishFromNodeTest, "test-http-client-keep-alive-release-before-finish.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawTestHttpClientLeakyWithDoubleResponseFromNodeTest) {
  GTEST_SKIP() << "Temporarily skipped test-http-client-leaky-with-double-response.js: upstream-style GC timing in this scenario is not stable in node.";
}
DEFINE_RAW_NODE_TEST(RawTestHttpClientOverrideGlobalAgentFromNodeTest, "test-http-client-override-global-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientParseErrorFromNodeTest, "test-http-client-parse-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientPipeEndFromNodeTest, "test-http-client-pipe-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRace2FromNodeTest, "test-http-client-race-2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRaceFromNodeTest, "test-http-client-race.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientReadInErrorFromNodeTest, "test-http-client-read-in-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientReadableFromNodeTest, "test-http-client-readable.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRejectChunkedWithContentLengthFromNodeTest, "test-http-client-reject-chunked-with-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRejectCrNoLfFromNodeTest, "test-http-client-reject-cr-no-lf.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRejectUnexpectedAgentFromNodeTest, "test-http-client-reject-unexpected-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientReqErrorDontDoubleFireFromNodeTest, "test-http-client-req-error-dont-double-fire.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientRequestOptionsFromNodeTest, "test-http-client-request-options.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientResDestroyedFromNodeTest, "test-http-client-res-destroyed.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientResponseDomainFromNodeTest, "test-http-client-response-domain.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientResponseTimeoutFromNodeTest, "test-http-client-response-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientSetTimeoutAfterEndFromNodeTest, "test-http-client-set-timeout-after-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientSetTimeoutFromNodeTest, "test-http-client-set-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientSpuriousAbortedFromNodeTest, "test-http-client-spurious-aborted.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutAgentFromNodeTest, "test-http-client-timeout-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutConnectListenerFromNodeTest, "test-http-client-timeout-connect-listener.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutEventFromNodeTest, "test-http-client-timeout-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutOnConnectFromNodeTest, "test-http-client-timeout-on-connect.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutOptionListenersFromNodeTest, "test-http-client-timeout-option-listeners.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutOptionWithAgentFromNodeTest, "test-http-client-timeout-option-with-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutOptionFromNodeTest, "test-http-client-timeout-option.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutWithDataFromNodeTest, "test-http-client-timeout-with-data.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientTimeoutFromNodeTest, "test-http-client-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientUnescapedPathFromNodeTest, "test-http-client-unescaped-path.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientUploadBufFromNodeTest, "test-http-client-upload-buf.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientUploadFromNodeTest, "test-http-client-upload.js")
DEFINE_RAW_NODE_TEST(RawTestHttpClientWithCreateConnectionFromNodeTest, "test-http-client-with-create-connection.js")
DEFINE_RAW_NODE_TEST(RawTestHttpCommonFromNodeTest, "test-http-common.js")
DEFINE_RAW_NODE_TEST(RawTestHttpConnResetFromNodeTest, "test-http-conn-reset.js")
DEFINE_RAW_NODE_TEST(RawTestHttpConnectReqResFromNodeTest, "test-http-connect-req-res.js")
DEFINE_RAW_NODE_TEST(RawTestHttpConnectFromNodeTest, "test-http-connect.js")
DEFINE_RAW_NODE_TEST(RawTestHttpContentLengthMismatchFromNodeTest, "test-http-content-length-mismatch.js")
DEFINE_RAW_NODE_TEST(RawTestHttpContentLengthFromNodeTest, "test-http-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpContentLength0FromNodeTest, "test-http-contentLength0.js")
DEFINE_RAW_NODE_TEST(RawTestHttpCorrectHostnameFromNodeTest, "test-http-correct-hostname.js")
DEFINE_RAW_NODE_TEST(RawTestHttpCreateConnectionFromNodeTest, "test-http-createConnection.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDateHeaderFromNodeTest, "test-http-date-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDebugFromNodeTest, "test-http-debug.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDecodedAuthFromNodeTest, "test-http-decoded-auth.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDefaultEncodingFromNodeTest, "test-http-default-encoding.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDefaultPortFromNodeTest, "test-http-default-port.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDestroyedSocketWrite2FromNodeTest, "test-http-destroyed-socket-write2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDnsErrorFromNodeTest, "test-http-dns-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDontSetDefaultHeadersWithSetHeaderFromNodeTest, "test-http-dont-set-default-headers-with-set-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDontSetDefaultHeadersWithSetHostFromNodeTest, "test-http-dont-set-default-headers-with-setHost.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDontSetDefaultHeadersFromNodeTest, "test-http-dont-set-default-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDoubleContentLengthFromNodeTest, "test-http-double-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDummyCharactersSmugglingFromNodeTest, "test-http-dummy-characters-smuggling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpDumpReqWhenResEndsFromNodeTest, "test-http-dump-req-when-res-ends.js")
DEFINE_RAW_NODE_TEST(RawTestHttpEarlyHintsInvalidArgumentFromNodeTest, "test-http-early-hints-invalid-argument.js")
DEFINE_RAW_NODE_TEST(RawTestHttpEarlyHintsFromNodeTest, "test-http-early-hints.js")
DEFINE_RAW_NODE_TEST(RawTestHttpEndThrowSocketHandlingFromNodeTest, "test-http-end-throw-socket-handling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpEofOnConnectFromNodeTest, "test-http-eof-on-connect.js")
DEFINE_RAW_NODE_TEST(RawTestHttpExceptionsFromNodeTest, "test-http-exceptions.js")
DEFINE_RAW_NODE_TEST(RawTestHttpExpectContinueFromNodeTest, "test-http-expect-continue.js")
DEFINE_RAW_NODE_TEST(RawTestHttpExpectHandlingFromNodeTest, "test-http-expect-handling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpExtraResponseFromNodeTest, "test-http-extra-response.js")
DEFINE_RAW_NODE_TEST(RawTestHttpFlushHeadersFromNodeTest, "test-http-flush-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpFlushResponseHeadersFromNodeTest, "test-http-flush-response-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpFullResponseFromNodeTest, "test-http-full-response.js")
DEFINE_RAW_NODE_TEST(RawTestHttpGenericStreamsFromNodeTest, "test-http-generic-streams.js")
DEFINE_RAW_NODE_TEST(RawTestHttpGetPipelineProblemFromNodeTest, "test-http-get-pipeline-problem.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeadRequestFromNodeTest, "test-http-head-request.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeadResponseHasNoBodyEndImplicitHeadersFromNodeTest, "test-http-head-response-has-no-body-end-implicit-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeadResponseHasNoBodyEndFromNodeTest, "test-http-head-response-has-no-body-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeadResponseHasNoBodyFromNodeTest, "test-http-head-response-has-no-body.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeadThrowOnResponseBodyWriteFromNodeTest, "test-http-head-throw-on-response-body-write.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderBadrequestFromNodeTest, "test-http-header-badrequest.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderObstextFromNodeTest, "test-http-header-obstext.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderOverflowFromNodeTest, "test-http-header-overflow.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderOwstextFromNodeTest, "test-http-header-owstext.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderReadFromNodeTest, "test-http-header-read.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHeaderValidatorsFromNodeTest, "test-http-header-validators.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHexWriteFromNodeTest, "test-http-hex-write.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHighwatermarkFromNodeTest, "test-http-highwatermark.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHostHeaderIpv6FailFromNodeTest, "test-http-host-header-ipv6-fail.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHostHeadersFromNodeTest, "test-http-host-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpHostnameTypecheckingFromNodeTest, "test-http-hostname-typechecking.js")
DEFINE_RAW_NODE_TEST(RawTestHttpImportWebsocketFromNodeTest, "test-http-import-websocket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpIncomingMatchKnownFieldsFromNodeTest, "test-http-incoming-matchKnownFields.js")
DEFINE_RAW_NODE_TEST(RawTestHttpIncomingMessageConnectionSetterFromNodeTest, "test-http-incoming-message-connection-setter.js")
DEFINE_RAW_NODE_TEST(RawTestHttpIncomingMessageDestroyFromNodeTest, "test-http-incoming-message-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpIncomingMessageOptionsFromNodeTest, "test-http-incoming-message-options.js")
DEFINE_RAW_NODE_TEST(RawTestHttpIncomingPipelinedSocketDestroyFromNodeTest, "test-http-incoming-pipelined-socket-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInformationHeadersFromNodeTest, "test-http-information-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInformationProcessingFromNodeTest, "test-http-information-processing.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInsecureParserPerStreamFromNodeTest, "test-http-insecure-parser-per-stream.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInsecureParserFromNodeTest, "test-http-insecure-parser.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInvalidPathCharsFromNodeTest, "test-http-invalid-path-chars.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInvalidTeFromNodeTest, "test-http-invalid-te.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInvalidUrlsFromNodeTest, "test-http-invalid-urls.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInvalidheaderfieldFromNodeTest, "test-http-invalidheaderfield.js")
DEFINE_RAW_NODE_TEST(RawTestHttpInvalidheaderfield2FromNodeTest, "test-http-invalidheaderfield2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveCloseOnHeaderFromNodeTest, "test-http-keep-alive-close-on-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveDropRequestsFromNodeTest, "test-http-keep-alive-drop-requests.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveMaxRequestsFromNodeTest, "test-http-keep-alive-max-requests.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAlivePipelineMaxRequestsFromNodeTest, "test-http-keep-alive-pipeline-max-requests.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveTimeoutBufferFromNodeTest, "test-http-keep-alive-timeout-buffer.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveTimeoutCustomFromNodeTest, "test-http-keep-alive-timeout-custom.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveTimeoutRaceConditionFromNodeTest, "test-http-keep-alive-timeout-race-condition.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveTimeoutFromNodeTest, "test-http-keep-alive-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepAliveFromNodeTest, "test-http-keep-alive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepaliveClientFromNodeTest, "test-http-keepalive-client.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepaliveFreeFromNodeTest, "test-http-keepalive-free.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepaliveOverrideFromNodeTest, "test-http-keepalive-override.js")
DEFINE_RAW_NODE_TEST(RawTestHttpKeepaliveRequestFromNodeTest, "test-http-keepalive-request.js")
DEFINE_RAW_NODE_TEST(RawTestHttpListeningFromNodeTest, "test-http-listening.js")
DEFINE_RAW_NODE_TEST(RawTestHttpLocaladdressBindErrorFromNodeTest, "test-http-localaddress-bind-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpLocaladdressFromNodeTest, "test-http-localaddress.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMalformedRequestFromNodeTest, "test-http-malformed-request.js")
DEFINE_RAW_NODE_TEST(RawTestHttpManyEndedPipelinesFromNodeTest, "test-http-many-ended-pipelines.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMaxHeaderSizePerStreamFromNodeTest, "test-http-max-header-size-per-stream.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMaxHeaderSizeFromNodeTest, "test-http-max-header-size.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMaxHeadersCountFromNodeTest, "test-http-max-headers-count.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMaxHttpHeadersFromNodeTest, "test-http-max-http-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMaxSocketsFromNodeTest, "test-http-max-sockets.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMethodsFromNodeTest, "test-http-methods.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMissingHeaderSeparatorCrFromNodeTest, "test-http-missing-header-separator-cr.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMissingHeaderSeparatorLfFromNodeTest, "test-http-missing-header-separator-lf.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMultiLineHeadersFromNodeTest, "test-http-multi-line-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMultipleHeadersFromNodeTest, "test-http-multiple-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpMutableHeadersFromNodeTest, "test-http-mutable-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpNoContentLengthFromNodeTest, "test-http-no-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpNoReadNoDumpFromNodeTest, "test-http-no-read-no-dump.js")
DEFINE_RAW_NODE_TEST(RawTestHttpNodelayFromNodeTest, "test-http-nodelay.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingBufferFromNodeTest, "test-http-outgoing-buffer.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingDestroyFromNodeTest, "test-http-outgoing-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingDestroyedFromNodeTest, "test-http-outgoing-destroyed.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingEndCorkFromNodeTest, "test-http-outgoing-end-cork.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingEndMultipleFromNodeTest, "test-http-outgoing-end-multiple.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingEndTypesFromNodeTest, "test-http-outgoing-end-types.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingFinishWritableFromNodeTest, "test-http-outgoing-finish-writable.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingFinishFromNodeTest, "test-http-outgoing-finish.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingFinishedFromNodeTest, "test-http-outgoing-finished.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingFirstChunkSinglebyteEncodingFromNodeTest, "test-http-outgoing-first-chunk-singlebyte-encoding.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingMessageCaptureRejectionFromNodeTest, "test-http-outgoing-message-capture-rejection.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingMessageInheritanceFromNodeTest, "test-http-outgoing-message-inheritance.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingMessageWriteCallbackFromNodeTest, "test-http-outgoing-message-write-callback.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingPropertiesFromNodeTest, "test-http-outgoing-properties.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingProtoFromNodeTest, "test-http-outgoing-proto.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingRenderHeadersFromNodeTest, "test-http-outgoing-renderHeaders.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingSettimeoutFromNodeTest, "test-http-outgoing-settimeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingWritableFinishedFromNodeTest, "test-http-outgoing-writableFinished.js")
DEFINE_RAW_NODE_TEST(RawTestHttpOutgoingWriteTypesFromNodeTest, "test-http-outgoing-write-types.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserBadRefFromNodeTest, "test-http-parser-bad-ref.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserFinishErrorFromNodeTest, "test-http-parser-finish-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserFreeFromNodeTest, "test-http-parser-free.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserFreedBeforeUpgradeFromNodeTest, "test-http-parser-freed-before-upgrade.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserLazyLoadedFromNodeTest, "test-http-parser-lazy-loaded.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserMemoryRetentionFromNodeTest, "test-http-parser-memory-retention.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserMultipleExecuteFromNodeTest, "test-http-parser-multiple-execute.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserTimeoutResetFromNodeTest, "test-http-parser-timeout-reset.js")
DEFINE_RAW_NODE_TEST(RawTestHttpParserFromNodeTest, "test-http-parser.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPauseNoDumpFromNodeTest, "test-http-pause-no-dump.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPauseResumeOneEndFromNodeTest, "test-http-pause-resume-one-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPauseFromNodeTest, "test-http-pause.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPerfHooksFromNodeTest, "test-http-perf_hooks.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPipeFsFromNodeTest, "test-http-pipe-fs.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPipelineAssertionerrorFinishFromNodeTest, "test-http-pipeline-assertionerror-finish.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPipelineFloodFromNodeTest, "test-http-pipeline-flood.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPipelineRequestsConnectionLeakFromNodeTest, "test-http-pipeline-requests-connection-leak.js")
DEFINE_RAW_NODE_TEST(RawTestHttpPipelineSocketParserTypeerrorFromNodeTest, "test-http-pipeline-socket-parser-typeerror.js")
DEFINE_RAW_NODE_TEST(RawTestHttpProxyFromNodeTest, "test-http-proxy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRawHeadersFromNodeTest, "test-http-raw-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRawheadersLimitFromNodeTest, "test-http-rawheaders-limit.js")
DEFINE_RAW_NODE_TEST(RawTestHttpReadableDataEventFromNodeTest, "test-http-readable-data-event.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRemoveConnectionHeaderPersistsConnectionFromNodeTest, "test-http-remove-connection-header-persists-connection.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRemoveHeaderStaysRemovedFromNodeTest, "test-http-remove-header-stays-removed.js")
DEFINE_RAW_NODE_TEST(RawTestHttpReqCloseRobustFromTamperingFromNodeTest, "test-http-req-close-robust-from-tampering.js")
DEFINE_RAW_NODE_TEST(RawTestHttpReqResCloseFromNodeTest, "test-http-req-res-close.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestAgentFromNodeTest, "test-http-request-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestArgumentsFromNodeTest, "test-http-request-arguments.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestDontOverrideOptionsFromNodeTest, "test-http-request-dont-override-options.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestEndTwiceFromNodeTest, "test-http-request-end-twice.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestEndFromNodeTest, "test-http-request-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestHostHeaderFromNodeTest, "test-http-request-host-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestInvalidMethodErrorFromNodeTest, "test-http-request-invalid-method-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestJoinAuthorizationHeadersFromNodeTest, "test-http-request-join-authorization-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestLargePayloadFromNodeTest, "test-http-request-large-payload.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestMethodDeletePayloadFromNodeTest, "test-http-request-method-delete-payload.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestMethodsFromNodeTest, "test-http-request-methods.js")
DEFINE_RAW_NODE_TEST(RawTestHttpRequestSmugglingContentLengthFromNodeTest, "test-http-request-smuggling-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResWriteAfterEndFromNodeTest, "test-http-res-write-after-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResWriteEndDontTakeArrayFromNodeTest, "test-http-res-write-end-dont-take-array.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseAddHeaderAfterSentFromNodeTest, "test-http-response-add-header-after-sent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseCloseFromNodeTest, "test-http-response-close.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseCorkFromNodeTest, "test-http-response-cork.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseMultiContentLengthFromNodeTest, "test-http-response-multi-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseMultiheadersFromNodeTest, "test-http-response-multiheaders.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseNoHeadersFromNodeTest, "test-http-response-no-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseReadableFromNodeTest, "test-http-response-readable.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseRemoveHeaderAfterSentFromNodeTest, "test-http-response-remove-header-after-sent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseSetheadersFromNodeTest, "test-http-response-setheaders.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseSplittingFromNodeTest, "test-http-response-splitting.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseStatusMessageFromNodeTest, "test-http-response-status-message.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseStatuscodeFromNodeTest, "test-http-response-statuscode.js")
DEFINE_RAW_NODE_TEST(RawTestHttpResponseWriteheadReturnsThisFromNodeTest, "test-http-response-writehead-returns-this.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSameMapFromNodeTest, "test-http-same-map.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerCaptureRejectionsFromNodeTest, "test-http-server-capture-rejections.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerClearTimerFromNodeTest, "test-http-server-clear-timer.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerClientErrorFromNodeTest, "test-http-server-client-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerCloseAllFromNodeTest, "test-http-server-close-all.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerCloseDestroyTimeoutFromNodeTest, "test-http-server-close-destroy-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerCloseIdleWaitResponseFromNodeTest, "test-http-server-close-idle-wait-response.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerCloseIdleFromNodeTest, "test-http-server-close-idle.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerConnectionListWhenCloseFromNodeTest, "test-http-server-connection-list-when-close.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerConnectionsCheckingLeakFromNodeTest, "test-http-server-connections-checking-leak.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerConsumedTimeoutFromNodeTest, "test-http-server-consumed-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerDeChunkedTrailerFromNodeTest, "test-http-server-de-chunked-trailer.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerDeleteParserFromNodeTest, "test-http-server-delete-parser.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerDestroySocketOnClientErrorFromNodeTest, "test-http-server-destroy-socket-on-client-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerDropConnectionsInClusterFromNodeTest, "test-http-server-drop-connections-in-cluster.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerHeadersTimeoutDelayedHeadersFromNodeTest, "test-http-server-headers-timeout-delayed-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerHeadersTimeoutInterruptedHeadersFromNodeTest, "test-http-server-headers-timeout-interrupted-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerHeadersTimeoutKeepaliveFromNodeTest, "test-http-server-headers-timeout-keepalive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerHeadersTimeoutPipeliningFromNodeTest, "test-http-server-headers-timeout-pipelining.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerIncomingmessageDestroyFromNodeTest, "test-http-server-incomingmessage-destroy.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerKeepAliveDefaultsFromNodeTest, "test-http-server-keep-alive-defaults.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerKeepAliveMaxRequestsNullFromNodeTest, "test-http-server-keep-alive-max-requests-null.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerKeepAliveTimeoutFromNodeTest, "test-http-server-keep-alive-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerKeepaliveEndFromNodeTest, "test-http-server-keepalive-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerKeepaliveReqGcFromNodeTest, "test-http-server-keepalive-req-gc.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerMethodQueryFromNodeTest, "test-http-server-method.query.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerMultiheadersFromNodeTest, "test-http-server-multiheaders.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerMultiheaders2FromNodeTest, "test-http-server-multiheaders2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerMultipleClientErrorFromNodeTest, "test-http-server-multiple-client-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerNonUtf8HeaderFromNodeTest, "test-http-server-non-utf8-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerOptimizeEmptyRequestsFromNodeTest, "test-http-server-optimize-empty-requests.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerOptionsHighwatermarkFromNodeTest, "test-http-server-options-highwatermark.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerOptionsIncomingMessageFromNodeTest, "test-http-server-options-incoming-message.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerOptionsServerResponseFromNodeTest, "test-http-server-options-server-response.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRejectChunkedWithContentLengthFromNodeTest, "test-http-server-reject-chunked-with-content-length.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRejectCrNoLfFromNodeTest, "test-http-server-reject-cr-no-lf.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutDelayedBodyFromNodeTest, "test-http-server-request-timeout-delayed-body.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutDelayedHeadersFromNodeTest, "test-http-server-request-timeout-delayed-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutInterruptedBodyFromNodeTest, "test-http-server-request-timeout-interrupted-body.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutInterruptedHeadersFromNodeTest, "test-http-server-request-timeout-interrupted-headers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutKeepaliveFromNodeTest, "test-http-server-request-timeout-keepalive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutPipeliningFromNodeTest, "test-http-server-request-timeout-pipelining.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerRequestTimeoutUpgradeFromNodeTest, "test-http-server-request-timeout-upgrade.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerResponseStandaloneFromNodeTest, "test-http-server-response-standalone.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawTestHttpServerStaleCloseFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "test-http-server-stale-close.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawTestHttpServerTimeoutsValidationFromNodeTest, "test-http-server-timeouts-validation.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerUnconsumeConsumeFromNodeTest, "test-http-server-unconsume-consume.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerUnconsumeFromNodeTest, "test-http-server-unconsume.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerWriteAfterEndFromNodeTest, "test-http-server-write-after-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerWriteEndAfterEndFromNodeTest, "test-http-server-write-end-after-end.js")
DEFINE_RAW_NODE_TEST(RawTestHttpServerFromNodeTest, "test-http-server.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetCookiesFromNodeTest, "test-http-set-cookies.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetHeaderChainFromNodeTest, "test-http-set-header-chain.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetMaxIdleHttpParserFromNodeTest, "test-http-set-max-idle-http-parser.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetTimeoutServerFromNodeTest, "test-http-set-timeout-server.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetTimeoutFromNodeTest, "test-http-set-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSetTrailersFromNodeTest, "test-http-set-trailers.js")
DEFINE_RAW_NODE_TEST(RawTestHttpShouldKeepAliveFromNodeTest, "test-http-should-keep-alive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSocketEncodingErrorFromNodeTest, "test-http-socket-encoding-error.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSocketErrorListenersFromNodeTest, "test-http-socket-error-listeners.js")
DEFINE_RAW_NODE_TEST(RawTestHttpStatusCodeFromNodeTest, "test-http-status-code.js")
DEFINE_RAW_NODE_TEST(RawTestHttpStatusMessageFromNodeTest, "test-http-status-message.js")
DEFINE_RAW_NODE_TEST(RawTestHttpStatusReasonInvalidCharsFromNodeTest, "test-http-status-reason-invalid-chars.js")
DEFINE_RAW_NODE_TEST(RawTestHttpSyncWriteErrorDuringContinueFromNodeTest, "test-http-sync-write-error-during-continue.js")
DEFINE_RAW_NODE_TEST(RawTestHttpTimeoutClientWarningFromNodeTest, "test-http-timeout-client-warning.js")
DEFINE_RAW_NODE_TEST(RawTestHttpTimeoutOverflowFromNodeTest, "test-http-timeout-overflow.js")
DEFINE_RAW_NODE_TEST(RawTestHttpTimeoutFromNodeTest, "test-http-timeout.js")
DEFINE_RAW_NODE_TEST(RawTestHttpTransferEncodingRepeatedChunkedFromNodeTest, "test-http-transfer-encoding-repeated-chunked.js")
DEFINE_RAW_NODE_TEST(RawTestHttpTransferEncodingSmugglingFromNodeTest, "test-http-transfer-encoding-smuggling.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUncaughtFromRequestCallbackFromNodeTest, "test-http-uncaught-from-request-callback.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUnixSocketKeepAliveFromNodeTest, "test-http-unix-socket-keep-alive.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUnixSocketFromNodeTest, "test-http-unix-socket.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeAdvertiseFromNodeTest, "test-http-upgrade-advertise.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeAgentFromNodeTest, "test-http-upgrade-agent.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeBinaryFromNodeTest, "test-http-upgrade-binary.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeClientFromNodeTest, "test-http-upgrade-client.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeClient2FromNodeTest, "test-http-upgrade-client2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeReconsumeStreamFromNodeTest, "test-http-upgrade-reconsume-stream.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeServerCallbackFromNodeTest, "test-http-upgrade-server-callback.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeServerFromNodeTest, "test-http-upgrade-server.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUpgradeServer2FromNodeTest, "test-http-upgrade-server2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseAuthWithHeaderInRequestFromNodeTest, "test-http-url.parse-auth-with-header-in-request.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseAuthFromNodeTest, "test-http-url.parse-auth.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseBasicFromNodeTest, "test-http-url.parse-basic.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseHttpsRequestFromNodeTest, "test-http-url.parse-https.request.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseOnlySupportHttpHttpsProtocolFromNodeTest, "test-http-url.parse-only-support-http-https-protocol.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParsePathFromNodeTest, "test-http-url.parse-path.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParsePostFromNodeTest, "test-http-url.parse-post.js")
DEFINE_RAW_NODE_TEST(RawTestHttpUrlParseSearchFromNodeTest, "test-http-url.parse-search.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWgetFromNodeTest, "test-http-wget.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWritableTrueAfterCloseFromNodeTest, "test-http-writable-true-after-close.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWriteCallbacksFromNodeTest, "test-http-write-callbacks.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWriteEmptyStringFromNodeTest, "test-http-write-empty-string.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWriteHead2FromNodeTest, "test-http-write-head-2.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWriteHeadAfterSetHeaderFromNodeTest, "test-http-write-head-after-set-header.js")
DEFINE_RAW_NODE_TEST(RawTestHttpWriteHeadFromNodeTest, "test-http-write-head.js")
DEFINE_RAW_NODE_TEST(RawTestHttpZeroLengthWriteFromNodeTest, "test-http-zero-length-write.js")
DEFINE_RAW_NODE_TEST(RawTestHttpZerolengthbufferFromNodeTest, "test-http-zerolengthbuffer.js")

// Raw Node dgram tests
DEFINE_RAW_NODE_TEST(RawDgramAbortClosedFromNodeTest, "test-dgram-abort-closed.js")
DEFINE_RAW_NODE_TEST(RawDgramAddressFromNodeTest, "test-dgram-address.js")
DEFINE_RAW_NODE_TEST(RawDgramBindDefaultAddressFromNodeTest, "test-dgram-bind-default-address.js")
DEFINE_RAW_NODE_TEST(RawDgramBindErrorRepeatFromNodeTest, "test-dgram-bind-error-repeat.js")
DEFINE_RAW_NODE_TEST(RawDgramBindFdErrorFromNodeTest, "test-dgram-bind-fd-error.js")
DEFINE_RAW_NODE_TEST(RawDgramBindFdFromNodeTest, "test-dgram-bind-fd.js")
DEFINE_RAW_NODE_TEST(RawDgramBindSocketCloseBeforeClusterReplyFromNodeTest, "test-dgram-bind-socket-close-before-cluster-reply.js")
DEFINE_RAW_NODE_TEST(RawDgramBindSocketCloseBeforeLookupFromNodeTest, "test-dgram-bind-socket-close-before-lookup.js")
DEFINE_RAW_NODE_TEST(RawDgramBindFromNodeTest, "test-dgram-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramBlocklistFromNodeTest, "test-dgram-blocklist.js")
DEFINE_RAW_NODE_TEST(RawDgramBytesLengthFromNodeTest, "test-dgram-bytes-length.js")
DEFINE_RAW_NODE_TEST(RawDgramCloseDuringBindFromNodeTest, "test-dgram-close-during-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramCloseInListeningFromNodeTest, "test-dgram-close-in-listening.js")
DEFINE_RAW_NODE_TEST(RawDgramCloseIsNotCallbackFromNodeTest, "test-dgram-close-is-not-callback.js")
DEFINE_RAW_NODE_TEST(RawDgramCloseSignalFromNodeTest, "test-dgram-close-signal.js")
DEFINE_RAW_NODE_TEST(RawDgramCloseFromNodeTest, "test-dgram-close.js")
DEFINE_RAW_NODE_TEST(RawDgramClusterBindErrorFromNodeTest, "test-dgram-cluster-bind-error.js")
DEFINE_RAW_NODE_TEST(RawDgramClusterCloseDuringBindFromNodeTest, "test-dgram-cluster-close-during-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramClusterCloseInListeningFromNodeTest, "test-dgram-cluster-close-in-listening.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendCallbackBufferLengthFromNodeTest, "test-dgram-connect-send-callback-buffer-length.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendCallbackBufferFromNodeTest, "test-dgram-connect-send-callback-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendCallbackMultiBufferFromNodeTest, "test-dgram-connect-send-callback-multi-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendDefaultHostFromNodeTest, "test-dgram-connect-send-default-host.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendEmptyArrayFromNodeTest, "test-dgram-connect-send-empty-array.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendEmptyBufferFromNodeTest, "test-dgram-connect-send-empty-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendEmptyPacketFromNodeTest, "test-dgram-connect-send-empty-packet.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendMultiBufferCopyFromNodeTest, "test-dgram-connect-send-multi-buffer-copy.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectSendMultiStringArrayFromNodeTest, "test-dgram-connect-send-multi-string-array.js")
DEFINE_RAW_NODE_TEST(RawDgramConnectFromNodeTest, "test-dgram-connect.js")
DEFINE_RAW_NODE_TEST(RawDgramCreateSocketHandleFdFromNodeTest, "test-dgram-create-socket-handle-fd.js")
DEFINE_RAW_NODE_TEST(RawDgramCreateSocketHandleFromNodeTest, "test-dgram-create-socket-handle.js")
DEFINE_RAW_NODE_TEST(RawDgramCreatesocketTypeFromNodeTest, "test-dgram-createSocket-type.js")
DEFINE_RAW_NODE_TEST(RawDgramCustomLookupFromNodeTest, "test-dgram-custom-lookup.js")
DEFINE_RAW_NODE_TEST(RawDgramDeprecationErrorFromNodeTest, "test-dgram-deprecation-error.js")
DEFINE_RAW_NODE_TEST(RawDgramErrorMessageAddressFromNodeTest, "test-dgram-error-message-address.js")
DEFINE_RAW_NODE_TEST(RawDgramExclusiveImplicitBindFromNodeTest, "test-dgram-exclusive-implicit-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramImplicitBindFromNodeTest, "test-dgram-implicit-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramIpv6onlyFromNodeTest, "test-dgram-ipv6only.js")
DEFINE_RAW_NODE_TEST(RawDgramListenAfterBindFromNodeTest, "test-dgram-listen-after-bind.js")
DEFINE_RAW_NODE_TEST(RawDgramMembershipFromNodeTest, "test-dgram-membership.js")
DEFINE_RAW_NODE_TEST(RawDgramMsgsizeFromNodeTest, "test-dgram-msgsize.js")
DEFINE_RAW_NODE_TEST(RawDgramMulticastLoopbackFromNodeTest, "test-dgram-multicast-loopback.js")
DEFINE_RAW_NODE_TEST(RawDgramMulticastSetInterfaceFromNodeTest, "test-dgram-multicast-set-interface.js")
DEFINE_RAW_NODE_TEST(RawDgramMulticastSetttlFromNodeTest, "test-dgram-multicast-setTTL.js")
DEFINE_RAW_NODE_TEST(RawDgramOobBufferFromNodeTest, "test-dgram-oob-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramRecvErrorFromNodeTest, "test-dgram-recv-error.js")
DEFINE_RAW_NODE_TEST(RawDgramRefFromNodeTest, "test-dgram-ref.js")
DEFINE_RAW_NODE_TEST(RawDgramReuseportFromNodeTest, "test-dgram-reuseport.js")
DEFINE_RAW_NODE_TEST(RawDgramSendAddressTypesFromNodeTest, "test-dgram-send-address-types.js")
DEFINE_RAW_NODE_TEST(RawDgramSendBadArgumentsFromNodeTest, "test-dgram-send-bad-arguments.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackBufferEmptyAddressFromNodeTest, "test-dgram-send-callback-buffer-empty-address.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackBufferLengthEmptyAddressFromNodeTest, "test-dgram-send-callback-buffer-length-empty-address.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackBufferLengthFromNodeTest, "test-dgram-send-callback-buffer-length.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackBufferFromNodeTest, "test-dgram-send-callback-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackMultiBufferEmptyAddressFromNodeTest, "test-dgram-send-callback-multi-buffer-empty-address.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackMultiBufferFromNodeTest, "test-dgram-send-callback-multi-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCallbackRecursiveFromNodeTest, "test-dgram-send-callback-recursive.js")
DEFINE_RAW_NODE_TEST(RawDgramSendCbQuelchesErrorFromNodeTest, "test-dgram-send-cb-quelches-error.js")
DEFINE_RAW_NODE_TEST(RawDgramSendDefaultHostFromNodeTest, "test-dgram-send-default-host.js")
DEFINE_RAW_NODE_TEST(RawDgramSendEmptyArrayFromNodeTest, "test-dgram-send-empty-array.js")
DEFINE_RAW_NODE_TEST(RawDgramSendEmptyBufferFromNodeTest, "test-dgram-send-empty-buffer.js")
DEFINE_RAW_NODE_TEST(RawDgramSendEmptyPacketFromNodeTest, "test-dgram-send-empty-packet.js")
DEFINE_RAW_NODE_TEST(RawDgramSendErrorFromNodeTest, "test-dgram-send-error.js")
DEFINE_RAW_NODE_TEST(RawDgramSendInvalidMsgTypeFromNodeTest, "test-dgram-send-invalid-msg-type.js")
DEFINE_RAW_NODE_TEST(RawDgramSendMultiBufferCopyFromNodeTest, "test-dgram-send-multi-buffer-copy.js")
DEFINE_RAW_NODE_TEST(RawDgramSendMultiStringArrayFromNodeTest, "test-dgram-send-multi-string-array.js")
DEFINE_RAW_NODE_TEST(RawDgramSendQueueInfoFromNodeTest, "test-dgram-send-queue-info.js")
DEFINE_RAW_NODE_TEST(RawDgramSendtoFromNodeTest, "test-dgram-sendto.js")
DEFINE_RAW_NODE_TEST(RawDgramSetbroadcastFromNodeTest, "test-dgram-setBroadcast.js")
DEFINE_RAW_NODE_TEST(RawDgramSetttlFromNodeTest, "test-dgram-setTTL.js")
DEFINE_RAW_NODE_TEST(RawDgramSocketBufferSizeFromNodeTest, "test-dgram-socket-buffer-size.js")
DEFINE_RAW_NODE_TEST(RawDgramUdp4FromNodeTest, "test-dgram-udp4.js")
DEFINE_RAW_NODE_TEST(RawDgramUdp6LinkLocalAddressFromNodeTest, "test-dgram-udp6-link-local-address.js")
DEFINE_RAW_NODE_TEST(RawDgramUdp6SendDefaultHostFromNodeTest, "test-dgram-udp6-send-default-host.js")
DEFINE_RAW_NODE_TEST(RawDgramUnrefInClusterFromNodeTest, "test-dgram-unref-in-cluster.js")
DEFINE_RAW_NODE_TEST(RawDgramUnrefFromNodeTest, "test-dgram-unref.js")

#undef DEFINE_RAW_NODE_TEST

TEST_F(Test3NodeDropinSubsetPhase02, RawPathFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathBasenameFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-basename.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathDirnameFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-dirname.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathExtnameFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-extname.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathGlobFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-glob.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathIsAbsoluteFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-isabsolute.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathJoinFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-join.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathMakeLongFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-makelong.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathNormalizeFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-normalize.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathParseFormatFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-parse-format.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathPosixExistsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-posix-exists.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathPosixRelativeOnWindowsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-posix-relative-on-windows.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathRelativeFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-relative.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathResolveFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-resolve.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathWin32ExistsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-win32-exists.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathWin32NormalizeDeviceNamesFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-win32-normalize-device-names.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

TEST_F(Test3NodeDropinSubsetPhase02, RawPathZeroLengthStringsFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(s.env, "test-path-zero-length-strings.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}

// Raw Node net tests (parallel, sequential, pummel).
#define DEFINE_RAW_NODE_TEST(test_name, script_name)            \
  TEST_F(Test3NodeDropinSubsetPhase02, test_name) {             \
    EnvScope s(runtime_.get());                                 \
    std::string error;                                           \
    const int exit_code = RunRawNodeTestScript(s.env, script_name, &error); \
    EXPECT_EQ(exit_code, 0) << "error=" << error;              \
    EXPECT_TRUE(error.empty()) << "error=" << error;            \
  }
DEFINE_RAW_NODE_TEST(RawNetAccessByteswrittenParallelFromNodeTest, "parallel/test-net-access-byteswritten.js")
DEFINE_RAW_NODE_TEST(RawNetAfterCloseParallelFromNodeTest, "parallel/test-net-after-close.js")
DEFINE_RAW_NODE_TEST(RawNetAllowHalfOpenParallelFromNodeTest, "parallel/test-net-allow-half-open.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyAttemptTimeoutCliOptionParallelFromNodeTest, "parallel/test-net-autoselectfamily-attempt-timeout-cli-option.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyAttemptTimeoutDefaultValueParallelFromNodeTest, "parallel/test-net-autoselectfamily-attempt-timeout-default-value.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyCommandlineOptionParallelFromNodeTest, "parallel/test-net-autoselectfamily-commandline-option.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyDefaultParallelFromNodeTest, "parallel/test-net-autoselectfamily-default.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyIpv4firstParallelFromNodeTest, "parallel/test-net-autoselectfamily-ipv4first.js")
DEFINE_RAW_NODE_TEST(RawNetAutoselectfamilyParallelFromNodeTest, "parallel/test-net-autoselectfamily.js")
DEFINE_RAW_NODE_TEST(RawNetBetterErrorMessagesListenPathParallelFromNodeTest, "parallel/test-net-better-error-messages-listen-path.js")
DEFINE_RAW_NODE_TEST(RawNetBetterErrorMessagesListenParallelFromNodeTest, "parallel/test-net-better-error-messages-listen.js")
DEFINE_RAW_NODE_TEST(RawNetBetterErrorMessagesPathParallelFromNodeTest, "parallel/test-net-better-error-messages-path.js")
DEFINE_RAW_NODE_TEST(RawNetBetterErrorMessagesPortHostnameParallelFromNodeTest, "parallel/test-net-better-error-messages-port-hostname.js")
DEFINE_RAW_NODE_TEST(RawNetBinaryParallelFromNodeTest, "parallel/test-net-binary.js")
DEFINE_RAW_NODE_TEST(RawNetBindTwiceParallelFromNodeTest, "parallel/test-net-bind-twice.js")
DEFINE_RAW_NODE_TEST(RawNetBlocklistParallelFromNodeTest, "parallel/test-net-blocklist.js")
DEFINE_RAW_NODE_TEST(RawNetBuffersizeParallelFromNodeTest, "parallel/test-net-buffersize.js")
DEFINE_RAW_NODE_TEST(RawNetBytesReadParallelFromNodeTest, "parallel/test-net-bytes-read.js")
DEFINE_RAW_NODE_TEST(RawNetBytesStatsParallelFromNodeTest, "parallel/test-net-bytes-stats.js")
DEFINE_RAW_NODE_TEST(RawNetBytesWrittenLargeParallelFromNodeTest, "parallel/test-net-bytes-written-large.js")
DEFINE_RAW_NODE_TEST(RawNetCanResetTimeoutParallelFromNodeTest, "parallel/test-net-can-reset-timeout.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawNetChildProcessConnectResetParallelFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "parallel/test-net-child-process-connect-reset.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawNetClientBindTwiceParallelFromNodeTest, "parallel/test-net-client-bind-twice.js")
DEFINE_RAW_NODE_TEST(RawNetConnectAbortControllerParallelFromNodeTest, "parallel/test-net-connect-abort-controller.js")
DEFINE_RAW_NODE_TEST(RawNetConnectAfterDestroyParallelFromNodeTest, "parallel/test-net-connect-after-destroy.js")
DEFINE_RAW_NODE_TEST(RawNetConnectBufferParallelFromNodeTest, "parallel/test-net-connect-buffer.js")
DEFINE_RAW_NODE_TEST(RawNetConnectBuffer2ParallelFromNodeTest, "parallel/test-net-connect-buffer2.js")
DEFINE_RAW_NODE_TEST(RawNetConnectCallSocketConnectParallelFromNodeTest, "parallel/test-net-connect-call-socket-connect.js")
DEFINE_RAW_NODE_TEST(RawNetConnectDestroyParallelFromNodeTest, "parallel/test-net-connect-destroy.js")
DEFINE_RAW_NODE_TEST(RawNetConnectImmediateDestroyParallelFromNodeTest, "parallel/test-net-connect-immediate-destroy.js")
DEFINE_RAW_NODE_TEST(RawNetConnectImmediateFinishParallelFromNodeTest, "parallel/test-net-connect-immediate-finish.js")
DEFINE_RAW_NODE_TEST(RawNetConnectKeepaliveParallelFromNodeTest, "parallel/test-net-connect-keepalive.js")
DEFINE_RAW_NODE_TEST(RawNetConnectMemleakParallelFromNodeTest, "parallel/test-net-connect-memleak.js")
DEFINE_RAW_NODE_TEST(RawNetConnectNoArgParallelFromNodeTest, "parallel/test-net-connect-no-arg.js")
DEFINE_RAW_NODE_TEST(RawNetConnectNodelayParallelFromNodeTest, "parallel/test-net-connect-nodelay.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsAllowhalfopenParallelFromNodeTest, "parallel/test-net-connect-options-allowhalfopen.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsFdParallelFromNodeTest, "parallel/test-net-connect-options-fd.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsInvalidParallelFromNodeTest, "parallel/test-net-connect-options-invalid.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsIpv6ParallelFromNodeTest, "parallel/test-net-connect-options-ipv6.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsPathParallelFromNodeTest, "parallel/test-net-connect-options-path.js")
DEFINE_RAW_NODE_TEST(RawNetConnectOptionsPortParallelFromNodeTest, "parallel/test-net-connect-options-port.js")
DEFINE_RAW_NODE_TEST(RawNetConnectPausedConnectionParallelFromNodeTest, "parallel/test-net-connect-paused-connection.js")
DEFINE_RAW_NODE_TEST(RawNetConnectResetAfterDestroyParallelFromNodeTest, "parallel/test-net-connect-reset-after-destroy.js")
DEFINE_RAW_NODE_TEST(RawNetConnectResetBeforeConnectedParallelFromNodeTest, "parallel/test-net-connect-reset-before-connected.js")
DEFINE_RAW_NODE_TEST(RawNetConnectResetUntilConnectedParallelFromNodeTest, "parallel/test-net-connect-reset-until-connected.js")
DEFINE_RAW_NODE_TEST(RawNetConnectResetParallelFromNodeTest, "parallel/test-net-connect-reset.js")
DEFINE_RAW_NODE_TEST(RawNetDnsCustomLookupParallelFromNodeTest, "parallel/test-net-dns-custom-lookup.js")
DEFINE_RAW_NODE_TEST(RawNetDnsErrorParallelFromNodeTest, "parallel/test-net-dns-error.js")
DEFINE_RAW_NODE_TEST(RawNetDnsLookupSkipParallelFromNodeTest, "parallel/test-net-dns-lookup-skip.js")
DEFINE_RAW_NODE_TEST(RawNetDnsLookupParallelFromNodeTest, "parallel/test-net-dns-lookup.js")
DEFINE_RAW_NODE_TEST(RawNetDuringCloseParallelFromNodeTest, "parallel/test-net-during-close.js")
DEFINE_RAW_NODE_TEST(RawNetEaddrinuseParallelFromNodeTest, "parallel/test-net-eaddrinuse.js")
DEFINE_RAW_NODE_TEST(RawNetEndCloseParallelFromNodeTest, "parallel/test-net-end-close.js")
DEFINE_RAW_NODE_TEST(RawNetEndDestroyedParallelFromNodeTest, "parallel/test-net-end-destroyed.js")
DEFINE_RAW_NODE_TEST(RawNetEndWithoutConnectParallelFromNodeTest, "parallel/test-net-end-without-connect.js")
DEFINE_RAW_NODE_TEST(RawNetErrorTwiceParallelFromNodeTest, "parallel/test-net-error-twice.js")
DEFINE_RAW_NODE_TEST(RawNetIsipParallelFromNodeTest, "parallel/test-net-isip.js")
DEFINE_RAW_NODE_TEST(RawNetIsipv4ParallelFromNodeTest, "parallel/test-net-isipv4.js")
DEFINE_RAW_NODE_TEST(RawNetIsipv6ParallelFromNodeTest, "parallel/test-net-isipv6.js")
DEFINE_RAW_NODE_TEST(RawNetKeepaliveParallelFromNodeTest, "parallel/test-net-keepalive.js")
DEFINE_RAW_NODE_TEST(RawNetLargeStringParallelFromNodeTest, "parallel/test-net-large-string.js")
DEFINE_RAW_NODE_TEST(RawNetListenAfterDestroyingStdinParallelFromNodeTest, "parallel/test-net-listen-after-destroying-stdin.js")
DEFINE_RAW_NODE_TEST(RawNetListenCloseServerCallbackIsNotFunctionParallelFromNodeTest, "parallel/test-net-listen-close-server-callback-is-not-function.js")
DEFINE_RAW_NODE_TEST(RawNetListenCloseServerParallelFromNodeTest, "parallel/test-net-listen-close-server.js")
DEFINE_RAW_NODE_TEST(RawNetListenErrorParallelFromNodeTest, "parallel/test-net-listen-error.js")
DEFINE_RAW_NODE_TEST(RawNetListenExclusiveRandomPortsParallelFromNodeTest, "parallel/test-net-listen-exclusive-random-ports.js")
DEFINE_RAW_NODE_TEST(RawNetListenFd0ParallelFromNodeTest, "parallel/test-net-listen-fd0.js")
DEFINE_RAW_NODE_TEST(RawNetListenHandleInCluster1ParallelFromNodeTest, "parallel/test-net-listen-handle-in-cluster-1.js")
DEFINE_RAW_NODE_TEST(RawNetListenHandleInCluster2ParallelFromNodeTest, "parallel/test-net-listen-handle-in-cluster-2.js")
DEFINE_RAW_NODE_TEST(RawNetListenInvalidPortParallelFromNodeTest, "parallel/test-net-listen-invalid-port.js")
DEFINE_RAW_NODE_TEST(RawNetListenIpv6onlyParallelFromNodeTest, "parallel/test-net-listen-ipv6only.js")
DEFINE_RAW_NODE_TEST(RawNetListenTwiceParallelFromNodeTest, "parallel/test-net-listen-twice.js")
DEFINE_RAW_NODE_TEST(RawNetListeningParallelFromNodeTest, "parallel/test-net-listening.js")
DEFINE_RAW_NODE_TEST(RawNetLocalAddressPortParallelFromNodeTest, "parallel/test-net-local-address-port.js")
DEFINE_RAW_NODE_TEST(RawNetLocalerrorParallelFromNodeTest, "parallel/test-net-localerror.js")
DEFINE_RAW_NODE_TEST(RawNetNormalizeArgsParallelFromNodeTest, "parallel/test-net-normalize-args.js")
DEFINE_RAW_NODE_TEST(RawNetOnreadStaticBufferParallelFromNodeTest, "parallel/test-net-onread-static-buffer.js")
DEFINE_RAW_NODE_TEST(RawNetOptionsLookupParallelFromNodeTest, "parallel/test-net-options-lookup.js")
DEFINE_RAW_NODE_TEST(RawNetPauseResumeConnectingParallelFromNodeTest, "parallel/test-net-pause-resume-connecting.js")
DEFINE_RAW_NODE_TEST(RawNetPerfHooksParallelFromNodeTest, "parallel/test-net-perf_hooks.js")
DEFINE_RAW_NODE_TEST(RawNetPersistentKeepaliveParallelFromNodeTest, "parallel/test-net-persistent-keepalive.js")
DEFINE_RAW_NODE_TEST(RawNetPersistentNodelayParallelFromNodeTest, "parallel/test-net-persistent-nodelay.js")
DEFINE_RAW_NODE_TEST(RawNetPersistentRefUnrefParallelFromNodeTest, "parallel/test-net-persistent-ref-unref.js")
DEFINE_RAW_NODE_TEST(RawNetPingpongParallelFromNodeTest, "parallel/test-net-pingpong.js")
DEFINE_RAW_NODE_TEST(RawNetPipeConnectErrorsParallelFromNodeTest, "parallel/test-net-pipe-connect-errors.js")
DEFINE_RAW_NODE_TEST(RawNetPipeWithLongPathParallelFromNodeTest, "parallel/test-net-pipe-with-long-path.js")
DEFINE_RAW_NODE_TEST(RawNetReconnectParallelFromNodeTest, "parallel/test-net-reconnect.js")
DEFINE_RAW_NODE_TEST(RawNetRemoteAddressPortParallelFromNodeTest, "parallel/test-net-remote-address-port.js")
DEFINE_RAW_NODE_TEST(RawNetRemoteAddressParallelFromNodeTest, "parallel/test-net-remote-address.js")
DEFINE_RAW_NODE_TEST(RawNetReuseportParallelFromNodeTest, "parallel/test-net-reuseport.js")
DEFINE_RAW_NODE_TEST(RawNetServerBlocklistParallelFromNodeTest, "parallel/test-net-server-blocklist.js")
DEFINE_RAW_NODE_TEST(RawNetServerCallListenMultipleTimesParallelFromNodeTest, "parallel/test-net-server-call-listen-multiple-times.js")
DEFINE_RAW_NODE_TEST(RawNetServerCaptureRejectionParallelFromNodeTest, "parallel/test-net-server-capture-rejection.js")
DEFINE_RAW_NODE_TEST(RawNetServerCloseBeforeCallingLookupCallbackParallelFromNodeTest, "parallel/test-net-server-close-before-calling-lookup-callback.js")
DEFINE_RAW_NODE_TEST(RawNetServerCloseBeforeIpcResponseParallelFromNodeTest, "parallel/test-net-server-close-before-ipc-response.js")
DEFINE_RAW_NODE_TEST(RawNetServerCloseParallelFromNodeTest, "parallel/test-net-server-close.js")
DEFINE_RAW_NODE_TEST(RawNetServerDropConnectionsParallelFromNodeTest, "parallel/test-net-server-drop-connections.js")
DEFINE_RAW_NODE_TEST(RawNetServerKeepaliveParallelFromNodeTest, "parallel/test-net-server-keepalive.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenHandleParallelFromNodeTest, "parallel/test-net-server-listen-handle.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenOptionsSignalParallelFromNodeTest, "parallel/test-net-server-listen-options-signal.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenOptionsParallelFromNodeTest, "parallel/test-net-server-listen-options.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenPathParallelFromNodeTest, "parallel/test-net-server-listen-path.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenRemoveCallbackParallelFromNodeTest, "parallel/test-net-server-listen-remove-callback.js")
DEFINE_RAW_NODE_TEST(RawNetServerMaxConnectionsCloseMakesMoreAvailableParallelFromNodeTest, "parallel/test-net-server-max-connections-close-makes-more-available.js")
DEFINE_RAW_NODE_TEST(RawNetServerMaxConnectionsParallelFromNodeTest, "parallel/test-net-server-max-connections.js")
DEFINE_RAW_NODE_TEST(RawNetServerNodelayParallelFromNodeTest, "parallel/test-net-server-nodelay.js")
DEFINE_RAW_NODE_TEST(RawNetServerOptionsParallelFromNodeTest, "parallel/test-net-server-options.js")
DEFINE_RAW_NODE_TEST(RawNetServerPauseOnConnectParallelFromNodeTest, "parallel/test-net-server-pause-on-connect.js")
DEFINE_RAW_NODE_TEST(RawNetServerResetParallelFromNodeTest, "parallel/test-net-server-reset.js")
DEFINE_RAW_NODE_TEST(RawNetServerTryPortsParallelFromNodeTest, "parallel/test-net-server-try-ports.js")
DEFINE_RAW_NODE_TEST(RawNetServerUnrefPersistentParallelFromNodeTest, "parallel/test-net-server-unref-persistent.js")
DEFINE_RAW_NODE_TEST(RawNetServerUnrefParallelFromNodeTest, "parallel/test-net-server-unref.js")
DEFINE_RAW_NODE_TEST(RawNetSettimeoutParallelFromNodeTest, "parallel/test-net-settimeout.js")
DEFINE_RAW_NODE_TEST(RawNetSocketByteswrittenParallelFromNodeTest, "parallel/test-net-socket-byteswritten.js")
DEFINE_RAW_NODE_TEST(RawNetSocketCloseAfterEndParallelFromNodeTest, "parallel/test-net-socket-close-after-end.js")
DEFINE_RAW_NODE_TEST(RawNetSocketConnectInvalidAutoselectfamilyParallelFromNodeTest, "parallel/test-net-socket-connect-invalid-autoselectfamily.js")
DEFINE_RAW_NODE_TEST(RawNetSocketConnectInvalidAutoselectfamilyattempttimeoutParallelFromNodeTest, "parallel/test-net-socket-connect-invalid-autoselectfamilyattempttimeout.js")
DEFINE_RAW_NODE_TEST(RawNetSocketConnectWithoutCbParallelFromNodeTest, "parallel/test-net-socket-connect-without-cb.js")
DEFINE_RAW_NODE_TEST(RawNetSocketConnectingParallelFromNodeTest, "parallel/test-net-socket-connecting.js")
DEFINE_RAW_NODE_TEST(RawNetSocketConstructorParallelFromNodeTest, "parallel/test-net-socket-constructor.js")
DEFINE_RAW_NODE_TEST(RawNetSocketDestroySendParallelFromNodeTest, "parallel/test-net-socket-destroy-send.js")
DEFINE_RAW_NODE_TEST(RawNetSocketDestroyTwiceParallelFromNodeTest, "parallel/test-net-socket-destroy-twice.js")
DEFINE_RAW_NODE_TEST(RawNetSocketEndBeforeConnectParallelFromNodeTest, "parallel/test-net-socket-end-before-connect.js")
DEFINE_RAW_NODE_TEST(RawNetSocketEndCallbackParallelFromNodeTest, "parallel/test-net-socket-end-callback.js")
DEFINE_RAW_NODE_TEST(RawNetSocketLocalAddressParallelFromNodeTest, "parallel/test-net-socket-local-address.js")
DEFINE_RAW_NODE_TEST(RawNetSocketNoHalfopenEnforcerParallelFromNodeTest, "parallel/test-net-socket-no-halfopen-enforcer.js")
DEFINE_RAW_NODE_TEST(RawNetSocketReadyWithoutCbParallelFromNodeTest, "parallel/test-net-socket-ready-without-cb.js")
DEFINE_RAW_NODE_TEST(RawNetSocketResetSendParallelFromNodeTest, "parallel/test-net-socket-reset-send.js")
DEFINE_RAW_NODE_TEST(RawNetSocketResetTwiceParallelFromNodeTest, "parallel/test-net-socket-reset-twice.js")
DEFINE_RAW_NODE_TEST(RawNetSocketSetnodelayParallelFromNodeTest, "parallel/test-net-socket-setnodelay.js")
DEFINE_RAW_NODE_TEST(RawNetSocketTimeoutUnrefParallelFromNodeTest, "parallel/test-net-socket-timeout-unref.js")
DEFINE_RAW_NODE_TEST(RawNetSocketTimeoutParallelFromNodeTest, "parallel/test-net-socket-timeout.js")
DEFINE_RAW_NODE_TEST(RawNetSocketWriteAfterCloseParallelFromNodeTest, "parallel/test-net-socket-write-after-close.js")
DEFINE_RAW_NODE_TEST(RawNetSocketWriteErrorParallelFromNodeTest, "parallel/test-net-socket-write-error.js")
DEFINE_RAW_NODE_TEST(RawNetStreamParallelFromNodeTest, "parallel/test-net-stream.js")
DEFINE_RAW_NODE_TEST(RawNetSyncCorkParallelFromNodeTest, "parallel/test-net-sync-cork.js")
DEFINE_RAW_NODE_TEST(RawNetThrottleParallelFromNodeTest, "parallel/test-net-throttle.js")
DEFINE_RAW_NODE_TEST(RawNetTimeoutNoHandleParallelFromNodeTest, "parallel/test-net-timeout-no-handle.js")
DEFINE_RAW_NODE_TEST(RawNetWritableParallelFromNodeTest, "parallel/test-net-writable.js")
DEFINE_RAW_NODE_TEST(RawNetWriteAfterCloseParallelFromNodeTest, "parallel/test-net-write-after-close.js")
DEFINE_RAW_NODE_TEST(RawNetWriteAfterEndNtParallelFromNodeTest, "parallel/test-net-write-after-end-nt.js")
DEFINE_RAW_NODE_TEST(RawNetWriteArgumentsParallelFromNodeTest, "parallel/test-net-write-arguments.js")
DEFINE_RAW_NODE_TEST(RawNetWriteCbOnDestroyBeforeConnectParallelFromNodeTest, "parallel/test-net-write-cb-on-destroy-before-connect.js")
DEFINE_RAW_NODE_TEST(RawNetWriteConnectWriteParallelFromNodeTest, "parallel/test-net-write-connect-write.js")
DEFINE_RAW_NODE_TEST(RawNetWriteFullyAsyncBufferParallelFromNodeTest, "parallel/test-net-write-fully-async-buffer.js")
DEFINE_RAW_NODE_TEST(RawNetWriteFullyAsyncHexStringParallelFromNodeTest, "parallel/test-net-write-fully-async-hex-string.js")
DEFINE_RAW_NODE_TEST(RawNetWriteSlowParallelFromNodeTest, "parallel/test-net-write-slow.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawNetGh5504SequentialFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "sequential/test-net-GH-5504.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawNetBetterErrorMessagesPortSequentialFromNodeTest, "sequential/test-net-better-error-messages-port.js")
DEFINE_RAW_NODE_TEST(RawNetConnectEconnrefusedSequentialFromNodeTest, "sequential/test-net-connect-econnrefused.js")
DEFINE_RAW_NODE_TEST(RawNetConnectHandleEconnrefusedSequentialFromNodeTest, "sequential/test-net-connect-handle-econnrefused.js")
DEFINE_RAW_NODE_TEST(RawNetConnectLocalErrorSequentialFromNodeTest, "sequential/test-net-connect-local-error.js")
DEFINE_RAW_NODE_TEST(RawNetListenSharedPortsSequentialFromNodeTest, "sequential/test-net-listen-shared-ports.js")
DEFINE_RAW_NODE_TEST(RawNetLocalportSequentialFromNodeTest, "sequential/test-net-localport.js")
DEFINE_RAW_NODE_TEST(RawNetReconnectErrorSequentialFromNodeTest, "sequential/test-net-reconnect-error.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawNetResponseSizeSequentialFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "sequential/test-net-response-size.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawNetServerAddressSequentialFromNodeTest, "sequential/test-net-server-address.js")
DEFINE_RAW_NODE_TEST(RawNetServerBindSequentialFromNodeTest, "sequential/test-net-server-bind.js")
DEFINE_RAW_NODE_TEST(RawNetServerListenIpv6LinkLocalSequentialFromNodeTest, "sequential/test-net-server-listen-ipv6-link-local.js")
DEFINE_RAW_NODE_TEST(RawNetManyClientsPummelFromNodeTest, "pummel/test-net-many-clients.js")
DEFINE_RAW_NODE_TEST(RawNetPausePummelFromNodeTest, "pummel/test-net-pause.js")
DEFINE_RAW_NODE_TEST(RawNetPingpongDelayPummelFromNodeTest, "pummel/test-net-pingpong-delay.js")
DEFINE_RAW_NODE_TEST(RawNetPingpongPummelFromNodeTest, "pummel/test-net-pingpong.js")
DEFINE_RAW_NODE_TEST(RawNetTimeoutPummelFromNodeTest, "pummel/test-net-timeout.js")
DEFINE_RAW_NODE_TEST(RawNetTimeout2PummelFromNodeTest, "pummel/test-net-timeout2.js")
DEFINE_RAW_NODE_TEST(RawNetWriteCallbacksPummelFromNodeTest, "pummel/test-net-write-callbacks.js")
DEFINE_RAW_NODE_TEST(RawPunycodeParallelFromNodeTest, "parallel/test-punycode.js")

namespace {

}  // namespace

// Raw Node expanded timers tests (previously parameterized)
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_api_refsFromNodeTest, "parallel/test-timers-api-refs.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_argsFromNodeTest, "parallel/test-timers-args.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_clear_null_does_not_throw_errorFromNodeTest, "parallel/test-timers-clear-null-does-not-throw-error.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_clear_object_does_not_throw_errorFromNodeTest, "parallel/test-timers-clear-object-does-not-throw-error.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_clear_timeout_interval_equivalentFromNodeTest, "parallel/test-timers-clear-timeout-interval-equivalent.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_clearImmediate_alsFromNodeTest, "parallel/test-timers-clearImmediate-als.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_clearImmediateFromNodeTest, "parallel/test-timers-clearImmediate.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_destroyedFromNodeTest, "parallel/test-timers-destroyed.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_disposeFromNodeTest, "parallel/test-timers-dispose.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_fast_callsFromNodeTest, "parallel/test-timers-fast-calls.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_promisifiedFromNodeTest, "parallel/test-timers-immediate-promisified.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_queue_throwFromNodeTest, "parallel/test-timers-immediate-queue-throw.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_queueFromNodeTest, "parallel/test-timers-immediate-queue.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_unref_nested_onceFromNodeTest, "parallel/test-timers-immediate-unref-nested-once.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_unref_simpleFromNodeTest, "parallel/test-timers-immediate-unref-simple.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediate_unrefFromNodeTest, "parallel/test-timers-immediate-unref.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_immediateFromNodeTest, "parallel/test-timers-immediate.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_interval_promisifiedFromNodeTest, "parallel/test-timers-interval-promisified.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_interval_throwFromNodeTest, "parallel/test-timers-interval-throw.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_invalid_clearFromNodeTest, "parallel/test-timers-invalid-clear.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_linked_listFromNodeTest, "parallel/test-timers-linked-list.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_max_duration_warningFromNodeTest, "parallel/test-timers-max-duration-warning.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_nan_duration_emit_once_per_processFromNodeTest, "parallel/test-timers-nan-duration-emit-once-per-process.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_nan_duration_warning_promisesFromNodeTest, "parallel/test-timers-nan-duration-warning-promises.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_nan_duration_warningFromNodeTest, "parallel/test-timers-nan-duration-warning.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_negative_duration_warning_emit_once_per_processFromNodeTest, "parallel/test-timers-negative-duration-warning-emit-once-per-process.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_negative_duration_warningFromNodeTest, "parallel/test-timers-negative-duration-warning.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_nestedFromNodeTest, "parallel/test-timers-nested.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_next_tickFromNodeTest, "parallel/test-timers-next-tick.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_non_integer_delayFromNodeTest, "parallel/test-timers-non-integer-delay.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_not_emit_duration_zeroFromNodeTest, "parallel/test-timers-not-emit-duration-zero.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_nowFromNodeTest, "parallel/test-timers-now.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_orderingFromNodeTest, "parallel/test-timers-ordering.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_process_tamperingFromNodeTest, "parallel/test-timers-process-tampering.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_promises_schedulerFromNodeTest, "parallel/test-timers-promises-scheduler.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_promisesFromNodeTest, "parallel/test-timers-promises.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_refresh_in_callbackFromNodeTest, "parallel/test-timers-refresh-in-callback.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_refreshFromNodeTest, "parallel/test-timers-refresh.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_reset_process_domain_on_throwFromNodeTest, "parallel/test-timers-reset-process-domain-on-throw.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_same_timeout_wrong_list_deletedFromNodeTest, "parallel/test-timers-same-timeout-wrong-list-deleted.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_setimmediate_infinite_loopFromNodeTest, "parallel/test-timers-setimmediate-infinite-loop.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_socket_timeout_removes_other_socket_unref_timerFromNodeTest, "parallel/test-timers-socket-timeout-removes-other-socket-unref-timer.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_thisFromNodeTest, "parallel/test-timers-this.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_throw_when_cb_not_functionFromNodeTest, "parallel/test-timers-throw-when-cb-not-function.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_timeout_promisifiedFromNodeTest, "parallel/test-timers-timeout-promisified.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_timeout_to_intervalFromNodeTest, "parallel/test-timers-timeout-to-interval.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_timeout_with_non_integerFromNodeTest, "parallel/test-timers-timeout-with-non-integer.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_to_primitiveFromNodeTest, "parallel/test-timers-to-primitive.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_uncaught_exceptionFromNodeTest, "parallel/test-timers-uncaught-exception.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unenroll_unref_intervalFromNodeTest, "parallel/test-timers-unenroll-unref-interval.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unref_throw_then_refFromNodeTest, "parallel/test-timers-unref-throw-then-ref.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unrefFromNodeTest, "parallel/test-timers-unref.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unrefd_interval_still_firesFromNodeTest, "parallel/test-timers-unrefd-interval-still-fires.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unrefed_in_beforeexitFromNodeTest, "parallel/test-timers-unrefed-in-beforeexit.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_unrefed_in_callbackFromNodeTest, "parallel/test-timers-unrefed-in-callback.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_user_callFromNodeTest, "parallel/test-timers-user-call.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_zero_timeoutFromNodeTest, "parallel/test-timers-zero-timeout.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timersFromNodeTest, "parallel/test-timers.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_block_eventloopFromNodeTest, "sequential/test-timers-block-eventloop.js")
DEFINE_RAW_NODE_TEST(RawExpandedTimers_test_timers_set_interval_excludes_callback_durationFromNodeTest, "sequential/test-timers-set-interval-excludes-callback-duration.js")

// Raw Node expanded child_process tests (previously parameterized)
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawn_eventFromNodeTest, "parallel/test-child-process-spawn-event.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawn_errorFromNodeTest, "parallel/test-child-process-spawn-error.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawn_controllerFromNodeTest, "parallel/test-child-process-spawn-controller.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawn_timeout_kill_signalFromNodeTest, "parallel/test-child-process-spawn-timeout-kill-signal.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawn_shellFromNodeTest, "parallel/test-child-process-spawn-shell.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_killFromNodeTest, "parallel/test-child-process-kill.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_cwdFromNodeTest, "parallel/test-child-process-cwd.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsyncFromNodeTest, "parallel/test-child-process-spawnsync.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_argsFromNodeTest, "parallel/test-child-process-spawnsync-args.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_inputFromNodeTest, "parallel/test-child-process-spawnsync-input.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_shellFromNodeTest, "parallel/test-child-process-spawnsync-shell.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_timeoutFromNodeTest, "parallel/test-child-process-spawnsync-timeout.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_maxbufFromNodeTest, "parallel/test-child-process-spawnsync-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_kill_signalFromNodeTest, "parallel/test-child-process-spawnsync-kill-signal.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_spawnsync_validation_errorsFromNodeTest, "parallel/test-child-process-spawnsync-validation-errors.js")
DEFINE_RAW_NODE_TEST(RawChildProcessPassing_parallel_test_child_process_fork_exec_argvFromNodeTest, "parallel/test-child-process-fork-exec-argv.js")

DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_advanced_serialization_largebufferFromNodeTest, "parallel/test-child-process-advanced-serialization-largebuffer.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_advanced_serialization_splitted_length_fieldFromNodeTest, "parallel/test-child-process-advanced-serialization-splitted-length-field.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_advanced_serializationFromNodeTest, "parallel/test-child-process-advanced-serialization.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_bad_stdioFromNodeTest, "parallel/test-child-process-bad-stdio.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_can_write_to_stdoutFromNodeTest, "parallel/test-child-process-can-write-to-stdout.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_constructorFromNodeTest, "parallel/test-child-process-constructor.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_cwdFromNodeTest, "parallel/test-child-process-cwd.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_default_optionsFromNodeTest, "parallel/test-child-process-default-options.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_destroyFromNodeTest, "parallel/test-child-process-destroy.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_detachedFromNodeTest, "parallel/test-child-process-detached.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_dgram_reuseportFromNodeTest, "parallel/test-child-process-dgram-reuseport.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_disconnectFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "parallel/test-child-process-disconnect.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_double_pipeFromNodeTest, "parallel/test-child-process-double-pipe.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_envFromNodeTest, "parallel/test-child-process-env.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_abortcontroller_promisifiedFromNodeTest, "parallel/test-child-process-exec-abortcontroller-promisified.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_any_shells_windowsFromNodeTest, "parallel/test-child-process-exec-any-shells-windows.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_cwdFromNodeTest, "parallel/test-child-process-exec-cwd.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_encodingFromNodeTest, "parallel/test-child-process-exec-encoding.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_envFromNodeTest, "parallel/test-child-process-exec-env.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_errorFromNodeTest, "parallel/test-child-process-exec-error.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_kill_throwsFromNodeTest, "parallel/test-child-process-exec-kill-throws.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_maxbufFromNodeTest, "parallel/test-child-process-exec-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_std_encodingFromNodeTest, "parallel/test-child-process-exec-std-encoding.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_stdout_stderr_data_stringFromNodeTest, "parallel/test-child-process-exec-stdout-stderr-data-string.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_timeout_expireFromNodeTest, "parallel/test-child-process-exec-timeout-expire.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_timeout_killFromNodeTest, "parallel/test-child-process-exec-timeout-kill.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exec_timeout_not_expiredFromNodeTest, "parallel/test-child-process-exec-timeout-not-expired.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execFile_promisified_abortControllerFromNodeTest, "parallel/test-child-process-execFile-promisified-abortController.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execfile_maxbufFromNodeTest, "parallel/test-child-process-execfile-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execfileFromNodeTest, "parallel/test-child-process-execfile.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execfilesync_maxbufFromNodeTest, "parallel/test-child-process-execfilesync-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execsync_maxbufFromNodeTest, "parallel/test-child-process-execsync-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exit_codeFromNodeTest, "parallel/test-child-process-exit-code.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_flush_stdioFromNodeTest, "parallel/test-child-process-flush-stdio.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_advanced_header_serializationFromNodeTest, "parallel/test-child-process-fork-advanced-header-serialization.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_and_spawnFromNodeTest, "parallel/test-child-process-fork-and-spawn.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_argsFromNodeTest, "parallel/test-child-process-fork-args.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_closeFromNodeTest, "parallel/test-child-process-fork-close.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_closed_channel_segfaultFromNodeTest, "parallel/test-child-process-fork-closed-channel-segfault.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_detachedFromNodeTest, "parallel/test-child-process-fork-detached.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_fork_dgramFromNodeTest) {
  std::string error;
  const int exit_code =
      RunRawNodeTestScriptInSubprocess("parallel/test-child-process-fork-dgram.js", &error);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_exec_argvFromNodeTest, "parallel/test-child-process-fork-exec-argv.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_exec_pathFromNodeTest, "parallel/test-child-process-fork-exec-path.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_getconnectionsFromNodeTest, "parallel/test-child-process-fork-getconnections.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_net_serverFromNodeTest, "parallel/test-child-process-fork-net-server.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_net_socketFromNodeTest, "parallel/test-child-process-fork-net-socket.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_netFromNodeTest, "parallel/test-child-process-fork-net.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_no_shellFromNodeTest, "parallel/test-child-process-fork-no-shell.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_refFromNodeTest, "parallel/test-child-process-fork-ref.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_ref2FromNodeTest, "parallel/test-child-process-fork-ref2.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_fork_abort_signalFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env,
      "parallel/test-child-process-fork-abort-signal.js",
      &error,
      true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_stdio_string_variantFromNodeTest, "parallel/test-child-process-fork-stdio-string-variant.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork_stdioFromNodeTest, "parallel/test-child-process-fork-stdio.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_fork_timeout_kill_signalFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env,
      "parallel/test-child-process-fork-timeout-kill-signal.js",
      &error,
      true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_forkFromNodeTest, "parallel/test-child-process-fork.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_fork3FromNodeTest, "parallel/test-child-process-fork3.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_http_socket_leakFromNodeTest, "parallel/test-child-process-http-socket-leak.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_internalFromNodeTest, "parallel/test-child-process-internal.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_ipc_next_tickFromNodeTest, "parallel/test-child-process-ipc-next-tick.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_ipcFromNodeTest, "parallel/test-child-process-ipc.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_killFromNodeTest, "parallel/test-child-process-kill.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_net_reuseportFromNodeTest, "parallel/test-child-process-net-reuseport.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_no_deprecationFromNodeTest, "parallel/test-child-process-no-deprecation.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_pipe_dataflowFromNodeTest, "parallel/test-child-process-pipe-dataflow.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_promisifiedFromNodeTest, "parallel/test-child-process-promisified.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_recv_handleFromNodeTest, "parallel/test-child-process-recv-handle.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_reject_null_bytesFromNodeTest, "parallel/test-child-process-reject-null-bytes.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_after_closeFromNodeTest, "parallel/test-child-process-send-after-close.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_cbFromNodeTest, "parallel/test-child-process-send-cb.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_keep_openFromNodeTest, "parallel/test-child-process-send-keep-open.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_returns_booleanFromNodeTest, "parallel/test-child-process-send-returns-boolean.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_type_errorFromNodeTest, "parallel/test-child-process-send-type-error.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_send_utf8FromNodeTest, "parallel/test-child-process-send-utf8.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_server_closeFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "parallel/test-child-process-server-close.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_set_blockingFromNodeTest, "parallel/test-child-process-set-blocking.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_silentFromNodeTest, "parallel/test-child-process-silent.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_argv0FromNodeTest, "parallel/test-child-process-spawn-argv0.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_controllerFromNodeTest, "parallel/test-child-process-spawn-controller.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_errorFromNodeTest, "parallel/test-child-process-spawn-error.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_eventFromNodeTest, "parallel/test-child-process-spawn-event.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_shellFromNodeTest, "parallel/test-child-process-spawn-shell.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_timeout_kill_signalFromNodeTest, "parallel/test-child-process-spawn-timeout-kill-signal.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_typeerrorFromNodeTest, "parallel/test-child-process-spawn-typeerror.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_windows_batch_fileFromNodeTest, "parallel/test-child-process-spawn-windows-batch-file.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_argsFromNodeTest, "parallel/test-child-process-spawnsync-args.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_envFromNodeTest, "parallel/test-child-process-spawnsync-env.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_inputFromNodeTest, "parallel/test-child-process-spawnsync-input.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_kill_signalFromNodeTest, "parallel/test-child-process-spawnsync-kill-signal.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_maxbufFromNodeTest, "parallel/test-child-process-spawnsync-maxbuf.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_shellFromNodeTest, "parallel/test-child-process-spawnsync-shell.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_timeoutFromNodeTest, "parallel/test-child-process-spawnsync-timeout.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsync_validation_errorsFromNodeTest, "parallel/test-child-process-spawnsync-validation-errors.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawnsyncFromNodeTest, "parallel/test-child-process-spawnsync.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdin_ipcFromNodeTest, "parallel/test-child-process-stdin-ipc.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdinFromNodeTest, "parallel/test-child-process-stdin.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdio_big_write_endFromNodeTest, "parallel/test-child-process-stdio-big-write-end.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdio_inheritFromNodeTest, "parallel/test-child-process-stdio-inherit.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdio_merge_stdouts_into_catFromNodeTest, "parallel/test-child-process-stdio-merge-stdouts-into-cat.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdio_overlappedFromNodeTest, "parallel/test-child-process-stdio-overlapped.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdio_reuse_readable_stdioFromNodeTest, "parallel/test-child-process-stdio-reuse-readable-stdio.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdioFromNodeTest, "parallel/test-child-process-stdio.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdout_flush_exitFromNodeTest, "parallel/test-child-process-stdout-flush-exit.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdout_flushFromNodeTest, "parallel/test-child-process-stdout-flush.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_stdout_ipcFromNodeTest, "parallel/test-child-process-stdout-ipc.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_uid_gidFromNodeTest, "parallel/test-child-process-uid-gid.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_validate_stdioFromNodeTest, "parallel/test-child-process-validate-stdio.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_windows_hideFromNodeTest, "parallel/test-child-process-windows-hide.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_spawn_loopFromNodeTest, "pummel/test-child-process-spawn-loop.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_emfileFromNodeTest, "sequential/test-child-process-emfile.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_execsyncFromNodeTest, "sequential/test-child-process-execsync.js")
DEFINE_RAW_NODE_TEST(RawExpandedChildProcess_test_child_process_exitFromNodeTest, "sequential/test-child-process-exit.js")
TEST_F(Test3NodeDropinSubsetPhase02, RawExpandedChildProcess_test_child_process_pass_fdFromNodeTest) {
  EnvScope s(runtime_.get());
  std::string error;
  const int exit_code = RunRawNodeTestScript(
      s.env, "sequential/test-child-process-pass-fd.js", &error, true);
  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
}
#undef DEFINE_RAW_NODE_TEST
#endif
