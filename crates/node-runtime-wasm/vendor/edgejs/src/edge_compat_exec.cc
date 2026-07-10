#include "edge_compat_exec.h"

#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <set>
#include <sstream>
#include <string>
#include <vector>

#if defined(_WIN32)
#include <errno.h>
#include <process.h>
#else
#include <errno.h>
#include <sys/wait.h>
#include <unistd.h>
#endif

#include "edge_process.h"
#include "edge_version.h"

namespace {

constexpr const char kSafeModeInstallUrl[] = "https://docs.wasmer.io/install";

#ifndef EDGE_DEFAULT_WASMER_PACKAGE
#define EDGE_DEFAULT_WASMER_PACKAGE "wasmer/edgejs@=" EDGE_VERSION_STRING
#endif

struct CommandResult {
  int exit_code = 1;
  int exec_errno = 0;
  std::string stdout_output;
  std::string stderr_output;
};

std::string ResolveWasmerBinary(std::string_view wasmer_bin) {
  if (!wasmer_bin.empty()) return std::string(wasmer_bin);
  return "wasmer";
}

std::string ResolveWasmerPackage(std::string_view wasmer_package) {
  if (!wasmer_package.empty()) return std::string(wasmer_package);
  return EDGE_DEFAULT_WASMER_PACKAGE;
}

std::string BuildEdgeBinaryPath() {
  namespace fs = std::filesystem;
  fs::path exec_path = EdgeGetProcessExecPath();
  if (exec_path.empty()) exec_path = "edge";
  return exec_path.lexically_normal().string();
}

std::string BuildCompatWrappedPathPrefix() {
  namespace fs = std::filesystem;
  fs::path exec_path = EdgeGetProcessExecPath();
  if (exec_path.empty()) exec_path = "edge";
  fs::path exec_dir = exec_path.parent_path();
  fs::path compat_dir;

  std::vector<fs::path> candidate_dirs = {
      (exec_dir / ".." / "bin-compat").lexically_normal(),
      (exec_dir / "bin-compat").lexically_normal(),
  };

  std::error_code ec;
  for (const fs::path& candidate : candidate_dirs) {
    ec.clear();
    if (candidate.empty()) continue;
    if (!fs::exists(candidate, ec) || ec) continue;
    ec.clear();
    if (fs::is_directory(candidate, ec) && !ec) {
      compat_dir = candidate;
      break;
    }
  }

  if (compat_dir.empty()) {
    compat_dir = candidate_dirs.front();
  }

#if defined(_WIN32)
  constexpr char kPathSeparator = ';';
#else
  constexpr char kPathSeparator = ':';
#endif
  std::string updated_path = compat_dir.lexically_normal().string();
  const char* current_path = std::getenv("PATH");
  if (current_path != nullptr && current_path[0] != '\0') {
    updated_path.push_back(kPathSeparator);
    updated_path += current_path;
  }
  return updated_path;
}

std::vector<std::string> BuildDefaultSafeModeCommand(std::string_view wasmer_bin,
                                                     std::string_view wasmer_package) {
  const std::string resolved_wasmer_bin = ResolveWasmerBinary(wasmer_bin);
  const std::string resolved_wasmer_package = ResolveWasmerPackage(wasmer_package);
  return {
      resolved_wasmer_bin,
      "run",
      "--experimental-napi",
      resolved_wasmer_package,
      "--volume=.",
      "--net",
      "--",
  };
}

bool SafeModeVersionHasRequiredNapiFeatures(const std::string& version_output) {
  std::istringstream stream(version_output);
  std::string line;
  while (std::getline(stream, line)) {
    if (line.rfind("features:", 0) != 0) continue;

    std::istringstream feature_stream(line.substr(std::strlen("features:")));
    std::set<std::string> features;
    std::string feature;
    while (feature_stream >> feature) {
      if (!feature.empty() && feature.back() == ',') {
        feature.pop_back();
      }
      if (!feature.empty()) {
        features.insert(feature);
      }
    }

    return features.contains("napi_v10") && features.contains("napi_extension_wasmer_v0");
  }
  return false;
}

CommandResult RunCommandCapture(const std::vector<std::string>& command) {
  CommandResult result;
  if (command.empty()) {
    result.stderr_output = "empty command";
    return result;
  }

  int stdout_pipe[2] = {-1, -1};
  int stderr_pipe[2] = {-1, -1};
  int exec_error_pipe[2] = {-1, -1};
  if (pipe(stdout_pipe) != 0 || pipe(stderr_pipe) != 0 || pipe(exec_error_pipe) != 0) {
    const int pipe_errno = errno;
    for (int fd : stdout_pipe) {
      if (fd >= 0) close(fd);
    }
    for (int fd : stderr_pipe) {
      if (fd >= 0) close(fd);
    }
    for (int fd : exec_error_pipe) {
      if (fd >= 0) close(fd);
    }
    result.stderr_output = std::strerror(pipe_errno);
    return result;
  }

  const pid_t child_pid = fork();
  if (child_pid < 0) {
    const int fork_errno = errno;
    close(stdout_pipe[0]);
    close(stdout_pipe[1]);
    close(stderr_pipe[0]);
    close(stderr_pipe[1]);
    close(exec_error_pipe[0]);
    close(exec_error_pipe[1]);
    result.stderr_output = std::strerror(fork_errno);
    return result;
  }

  if (child_pid == 0) {
    close(stdout_pipe[0]);
    close(stderr_pipe[0]);
    close(exec_error_pipe[0]);
    if (dup2(stdout_pipe[1], STDOUT_FILENO) < 0 || dup2(stderr_pipe[1], STDERR_FILENO) < 0) {
      const int dup_errno = errno;
      (void)write(exec_error_pipe[1], &dup_errno, sizeof(dup_errno));
      _exit(127);
    }
    close(stdout_pipe[1]);
    close(stderr_pipe[1]);

    std::vector<char*> argv;
    argv.reserve(command.size() + 1);
    for (const auto& arg : command) {
      argv.push_back(const_cast<char*>(arg.c_str()));
    }
    argv.push_back(nullptr);

    execvp(argv[0], argv.data());
    const int exec_errno = errno;
    (void)write(exec_error_pipe[1], &exec_errno, sizeof(exec_errno));
    _exit(127);
  }

  close(stdout_pipe[1]);
  close(stderr_pipe[1]);
  close(exec_error_pipe[1]);

  char buffer[512];
  while (true) {
    const ssize_t n = read(stdout_pipe[0], buffer, sizeof(buffer));
    if (n > 0) {
      result.stdout_output.append(buffer, static_cast<size_t>(n));
      continue;
    }
    break;
  }
  while (true) {
    const ssize_t n = read(stderr_pipe[0], buffer, sizeof(buffer));
    if (n > 0) {
      result.stderr_output.append(buffer, static_cast<size_t>(n));
      continue;
    }
    break;
  }

  int exec_errno = 0;
  const ssize_t exec_errno_size = read(exec_error_pipe[0], &exec_errno, sizeof(exec_errno));
  if (exec_errno_size == static_cast<ssize_t>(sizeof(exec_errno))) {
    result.exec_errno = exec_errno;
  }

  close(stdout_pipe[0]);
  close(stderr_pipe[0]);
  close(exec_error_pipe[0]);

  int status = 0;
  while (waitpid(child_pid, &status, 0) < 0) {
    if (errno == EINTR) continue;
    result.stderr_output = std::strerror(errno);
    return result;
  }

  if (WIFEXITED(status)) {
    result.exit_code = WEXITSTATUS(status);
  } else if (WIFSIGNALED(status)) {
    result.exit_code = 128 + WTERMSIG(status);
  }

  return result;
}

int RunCommandPassthrough(const std::vector<std::string>& command, std::string* error_out) {
  if (command.empty()) {
    if (error_out != nullptr) *error_out = "empty command";
    return 1;
  }

  int exec_error_pipe[2] = {-1, -1};
  if (pipe(exec_error_pipe) != 0) {
    if (error_out != nullptr) {
      *error_out = std::strerror(errno);
    }
    return 1;
  }

  const pid_t child_pid = fork();
  if (child_pid < 0) {
    const int fork_errno = errno;
    close(exec_error_pipe[0]);
    close(exec_error_pipe[1]);
    if (error_out != nullptr) {
      *error_out = std::strerror(fork_errno);
    }
    return 1;
  }

  if (child_pid == 0) {
    close(exec_error_pipe[0]);

    std::vector<char*> argv;
    argv.reserve(command.size() + 1);
    for (const auto& arg : command) {
      argv.push_back(const_cast<char*>(arg.c_str()));
    }
    argv.push_back(nullptr);

    execvp(argv[0], argv.data());
    const int exec_errno = errno;
    (void)write(exec_error_pipe[1], &exec_errno, sizeof(exec_errno));
    _exit(127);
  }

  close(exec_error_pipe[1]);

  int exec_errno = 0;
  const ssize_t exec_errno_size = read(exec_error_pipe[0], &exec_errno, sizeof(exec_errno));
  close(exec_error_pipe[0]);

  int status = 0;
  while (waitpid(child_pid, &status, 0) < 0) {
    if (errno == EINTR) continue;
    if (error_out != nullptr) {
      *error_out = std::strerror(errno);
    }
    return 1;
  }

  if (exec_errno_size == static_cast<ssize_t>(sizeof(exec_errno)) && exec_errno == ENOENT) {
    if (error_out != nullptr) {
      *error_out = "safe mode requires Wasmer. Install it from " + std::string(kSafeModeInstallUrl) + ".";
    }
    return 1;
  }

  if (WIFEXITED(status)) return WEXITSTATUS(status);
  if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
  if (error_out != nullptr) {
    *error_out = "safe mode command ended unexpectedly";
  }
  return 1;
}

}  // namespace

bool EdgeShouldWrapCompatCommand(std::string_view command) {
  static constexpr std::string_view kWrappedCommands[] = {
      "node",
      "npm",
      "npx",
      "pnpm",
      "pnpx",
      "yarn",
      "bun",
      "deno",
  };
  for (const auto candidate : kWrappedCommands) {
    if (command == candidate) return true;
  }
  return false;
}

int EdgeRunSafeModeCommand(const std::vector<std::string>& forwarded_args,
                           std::string_view wasmer_bin,
                           std::string_view wasmer_package,
                           std::string* error_out) {
  const std::string resolved_wasmer_bin = ResolveWasmerBinary(wasmer_bin);
  const CommandResult version_result = RunCommandCapture({resolved_wasmer_bin, "--version", "-v"});
  if (version_result.exec_errno == ENOENT) {
    if (error_out != nullptr) {
      if (resolved_wasmer_bin == "wasmer") {
        *error_out = "safe mode requires Wasmer.\nInstall it from " + std::string(kSafeModeInstallUrl) + ".";
      } else {
        *error_out = "safe mode requires a valid Wasmer binary: " + resolved_wasmer_bin;
      }
    }
    return 1;
  }
  if (version_result.exit_code != 0) {
    if (error_out != nullptr) {
      *error_out = "failed to query Wasmer version";
      const std::string details = version_result.stderr_output.empty() ? version_result.stdout_output : version_result.stderr_output;
      if (!details.empty()) {
        *error_out += ": " + details;
      }
    }
    return 1;
  }
  const std::string version_output =
      version_result.stdout_output.empty() ? version_result.stderr_output : version_result.stdout_output;
  if (!SafeModeVersionHasRequiredNapiFeatures(version_output)) {
    if (error_out != nullptr) {
      *error_out =
          "safe mode requires a Wasmer build with the napi_v10 and "
          "napi_extension_wasmer_v0 features enabled.\nInstall it from " +
          std::string(kSafeModeInstallUrl);
    }
    return 1;
  }

  std::vector<std::string> child_args =
      BuildDefaultSafeModeCommand(resolved_wasmer_bin, wasmer_package);
  child_args.insert(child_args.end(), forwarded_args.begin(), forwarded_args.end());
  return RunCommandPassthrough(child_args, error_out);
}

int EdgeRunCompatCommand(int argc, const char* const* argv, std::string* error_out) {
  if (argc <= 1 || argv == nullptr || argv[1] == nullptr) {
    if (error_out != nullptr) *error_out = "Missing wrapped command";
    return 1;
  }

  const std::string compat_path = BuildCompatWrappedPathPrefix();
  const std::string edge_binary_path = BuildEdgeBinaryPath();

#if defined(_WIN32)
  std::vector<const char*> child_argv;
  child_argv.reserve(static_cast<size_t>(argc));
  for (int i = 1; i < argc; ++i) {
    if (argv[i] != nullptr) child_argv.push_back(argv[i]);
  }
  child_argv.push_back(nullptr);

  char* old_path = nullptr;
  size_t old_path_len = 0;
  char* old_edge_binary_path = nullptr;
  size_t old_edge_binary_path_len = 0;
  (void)_dupenv_s(&old_path, &old_path_len, "PATH");
  (void)_dupenv_s(&old_edge_binary_path, &old_edge_binary_path_len, "EDGE_BINARY_PATH");
  _putenv_s("PATH", compat_path.c_str());
  _putenv_s("EDGE_BINARY_PATH", edge_binary_path.c_str());
  const intptr_t rc = _spawnvp(_P_WAIT, child_argv[0], child_argv.data());
  if (old_path != nullptr) {
    _putenv_s("PATH", old_path);
    free(old_path);
  } else {
    _putenv_s("PATH", "");
  }
  if (old_edge_binary_path != nullptr) {
    _putenv_s("EDGE_BINARY_PATH", old_edge_binary_path);
    free(old_edge_binary_path);
  } else {
    _putenv_s("EDGE_BINARY_PATH", "");
  }
  if (rc == -1) {
    if (error_out != nullptr) {
      *error_out = "spawn failed for compat command: " + std::string(child_argv[0]) +
                   ": " + std::strerror(errno);
    }
    return 127;
  }
  return static_cast<int>(rc);
#else
  int error_pipe[2] = {-1, -1};
  if (pipe(error_pipe) != 0) {
    if (error_out != nullptr) {
      *error_out = "pipe failed for compat command: " + std::string(std::strerror(errno));
    }
    return 1;
  }

  const pid_t child_pid = fork();
  if (child_pid < 0) {
    const int fork_errno = errno;
    close(error_pipe[0]);
    close(error_pipe[1]);
    if (error_out != nullptr) {
      *error_out = "fork failed for compat command: " + std::string(std::strerror(fork_errno));
    }
    return 1;
  }

  if (child_pid == 0) {
    close(error_pipe[0]);
    if (setenv("PATH", compat_path.c_str(), 1) != 0) {
      const std::string child_error =
          "setenv(PATH) failed for compat command: " + std::string(std::strerror(errno));
      (void)write(error_pipe[1], child_error.c_str(), child_error.size());
      _exit(127);
    }
    if (setenv("EDGE_BINARY_PATH", edge_binary_path.c_str(), 1) != 0) {
      const std::string child_error =
          "setenv(EDGE_BINARY_PATH) failed for compat command: " +
          std::string(std::strerror(errno));
      (void)write(error_pipe[1], child_error.c_str(), child_error.size());
      _exit(127);
    }

    std::vector<char*> child_argv;
    child_argv.reserve(static_cast<size_t>(argc));
    for (int i = 1; i < argc; ++i) {
      if (argv[i] != nullptr) child_argv.push_back(const_cast<char*>(argv[i]));
    }
    child_argv.push_back(nullptr);
    execvp(child_argv[0], child_argv.data());

    const std::string child_error = "exec failed for compat command: " + std::string(child_argv[0]) +
                                    ": " + std::string(std::strerror(errno));
    (void)write(error_pipe[1], child_error.c_str(), child_error.size());
    _exit(127);
  }

  close(error_pipe[1]);

  int status = 0;
  while (waitpid(child_pid, &status, 0) < 0) {
    if (errno == EINTR) continue;
    const int wait_errno = errno;
    close(error_pipe[0]);
    if (error_out != nullptr) {
      *error_out = "waitpid failed for compat command: " + std::string(std::strerror(wait_errno));
    }
    return 1;
  }

  std::string child_error;
  char buffer[512];
  while (true) {
    const ssize_t n = read(error_pipe[0], buffer, sizeof(buffer));
    if (n > 0) {
      child_error.append(buffer, static_cast<size_t>(n));
      continue;
    }
    break;
  }
  close(error_pipe[0]);

  if (!child_error.empty() && error_out != nullptr) {
    *error_out = child_error;
  }

  if (WIFEXITED(status)) return WEXITSTATUS(status);
  if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
  if (error_out != nullptr && child_error.empty()) {
    *error_out = "compat command ended unexpectedly";
  }
  return 1;
#endif
}
