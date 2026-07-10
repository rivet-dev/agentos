#include <cstdlib>
#include <filesystem>
#include <iostream>
#include <string>
#include <vector>

#if defined(_WIN32)
#include <cstring>
#include <errno.h>
#include <process.h>
#include <windows.h>
#else
#include <cerrno>
#include <cstring>
#include <unistd.h>
#if defined(__APPLE__)
#include <mach-o/dyld.h>
#endif
#endif
namespace {

std::vector<std::filesystem::path> BuildCandidateInstallDirs(const std::string& actual_exec_path,
                                                             const std::string& logical_exec_path) {
  namespace fs = std::filesystem;
  std::vector<fs::path> candidate_dirs;
  auto append_dir = [&](const std::string& exec_path) {
    if (exec_path.empty()) return;
    const fs::path dir = fs::path(exec_path).parent_path();
    if (dir.empty()) return;
    for (const auto& existing : candidate_dirs) {
      if (existing == dir) return;
    }
    candidate_dirs.push_back(dir);
  };

  append_dir(logical_exec_path);
  append_dir(actual_exec_path);
  return candidate_dirs;
}

std::string FallbackExecPath(const char* argv0) {
  namespace fs = std::filesystem;
  if (argv0 != nullptr && argv0[0] != '\0') {
    const fs::path candidate(argv0);
    if (candidate.is_absolute()) {
      return candidate.lexically_normal().string();
    }
    std::error_code ec;
    if (candidate.has_parent_path()) {
      return fs::absolute(candidate, ec).lexically_normal().string();
    }

    const char* path_env = std::getenv("PATH");
    if (path_env != nullptr && path_env[0] != '\0') {
#if defined(_WIN32)
      constexpr char kPathSeparator = ';';
      const char* exe_suffix = ".exe";
#else
      constexpr char kPathSeparator = ':';
#endif
      std::string path_value(path_env);
      size_t start = 0;
      while (start <= path_value.size()) {
        const size_t end = path_value.find(kPathSeparator, start);
        const std::string entry =
            path_value.substr(start, end == std::string::npos ? std::string::npos : end - start);
        if (!entry.empty()) {
          const fs::path base(entry);
          std::vector<fs::path> candidates = {base / candidate};
#if defined(_WIN32)
          candidates.push_back(base / (candidate.string() + exe_suffix));
#endif
          for (const auto& path_candidate : candidates) {
            ec.clear();
            if (fs::exists(path_candidate, ec) && !ec && !fs::is_directory(path_candidate, ec)) {
              return fs::absolute(path_candidate, ec).lexically_normal().string();
            }
          }
        }
        if (end == std::string::npos) break;
        start = end + 1;
      }
    }
    return candidate.string();
  }
  return "edgeenv";
}

std::string DetectActualExecPath(const char* argv0) {
#if defined(_WIN32)
  std::vector<char> buffer(MAX_PATH, '\0');
  DWORD length = GetModuleFileNameA(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
  while (length == buffer.size()) {
    buffer.resize(buffer.size() * 2, '\0');
    length = GetModuleFileNameA(nullptr, buffer.data(), static_cast<DWORD>(buffer.size()));
  }
  if (length > 0) {
    return std::string(buffer.data(), length);
  }
#elif defined(__APPLE__)
  uint32_t size = 0;
  _NSGetExecutablePath(nullptr, &size);
  if (size > 0) {
    std::vector<char> buffer(size + 1, '\0');
    if (_NSGetExecutablePath(buffer.data(), &size) == 0) {
      return std::string(buffer.data());
    }
  }
#elif defined(__linux__)
  std::vector<char> buffer(4096, '\0');
  const ssize_t length = readlink("/proc/self/exe", buffer.data(), buffer.size() - 1);
  if (length > 0) {
    buffer[static_cast<size_t>(length)] = '\0';
    return std::string(buffer.data());
  }
#endif
  return FallbackExecPath(argv0);
}

std::string DetectLogicalExecPath(const std::string& actual_exec_path) {
  const char* forced_exec = std::getenv("EDGE_EXEC_PATH");
  if (forced_exec != nullptr && forced_exec[0] != '\0') {
    return forced_exec;
  }
  return actual_exec_path;
}

std::string ResolveEdgeBinaryPath(const std::string& actual_exec_path,
                                  const std::string& logical_exec_path) {
  namespace fs = std::filesystem;
  std::vector<fs::path> candidate_dirs = BuildCandidateInstallDirs(actual_exec_path, logical_exec_path);

#if defined(_WIN32)
  constexpr const char* kEdgeBinaryName = "edge.exe";
#else
  constexpr const char* kEdgeBinaryName = "edge";
#endif

  std::error_code ec;
  for (const auto& dir : candidate_dirs) {
    if (dir.empty()) continue;
    const fs::path candidate = (dir / kEdgeBinaryName).lexically_normal();
    ec.clear();
    if (!fs::exists(candidate, ec) || ec) continue;
    ec.clear();
    if (fs::is_directory(candidate, ec) || ec) continue;
    return candidate.string();
  }

  const fs::path fallback_dir =
      !candidate_dirs.empty() ? candidate_dirs.front() : fs::path(".");
  return (fallback_dir / kEdgeBinaryName).lexically_normal().string();
}

std::string BuildCompatWrappedPathPrefix(const std::string& logical_exec_path) {
  namespace fs = std::filesystem;
  fs::path exec_path = logical_exec_path.empty() ? fs::path("edgeenv") : fs::path(logical_exec_path);
  fs::path exec_dir = exec_path.parent_path();
  std::vector<fs::path> compat_candidates = {
      (exec_dir / ".." / "bin-compat").lexically_normal(),
      (exec_dir / "bin-compat").lexically_normal(),
  };

  std::error_code ec;
  fs::path compat_dir;
  for (const auto& candidate : compat_candidates) {
    ec.clear();
    if (!fs::exists(candidate, ec) || ec) continue;
    ec.clear();
    if (fs::is_directory(candidate, ec) && !ec) {
      compat_dir = candidate;
      break;
    }
  }

  if (compat_dir.empty()) {
    compat_dir = !compat_candidates.empty() ? compat_candidates.front() : fs::path("bin-compat");
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

bool SetEnvVar(const char* key, const std::string& value, std::string* error_out) {
#if defined(_WIN32)
  if (_putenv_s(key, value.c_str()) == 0) {
    return true;
  }
  if (error_out != nullptr) {
    *error_out = std::string("failed to set ") + key;
  }
  return false;
#else
  if (setenv(key, value.c_str(), 1) == 0) {
    return true;
  }
  if (error_out != nullptr) {
    *error_out = std::string("failed to set ") + key + ": " + std::strerror(errno);
  }
  return false;
#endif
}

}  // namespace

int main(int argc, char** argv) {
  const std::string actual_exec_path = DetectActualExecPath(argc > 0 ? argv[0] : nullptr);
  const std::string logical_exec_path = DetectLogicalExecPath(actual_exec_path);
  const std::string edge_binary_path = ResolveEdgeBinaryPath(actual_exec_path, logical_exec_path);
  const std::string compat_path = BuildCompatWrappedPathPrefix(logical_exec_path);

  if (argc <= 1 || argv == nullptr || argv[1] == nullptr) {
    std::cerr << "Missing wrapped command\n";
    return 1;
  }

  std::string error;
  if (!SetEnvVar("PATH", compat_path, &error) ||
      !SetEnvVar("EDGE_BINARY_PATH", edge_binary_path, &error)) {
    std::cerr << error << "\n";
    return 1;
  }

  std::vector<char*> child_argv;
  child_argv.reserve(static_cast<size_t>(argc));
  for (int i = 1; i < argc; ++i) {
    if (argv[i] != nullptr) child_argv.push_back(argv[i]);
  }
  child_argv.push_back(nullptr);

#if defined(_WIN32)
  _execvp(child_argv[0], child_argv.data());
  std::cerr << "exec failed for compat command: " << child_argv[0] << ": "
            << std::strerror(errno) << "\n";
  return errno == ENOENT ? 127 : 1;
#else
  execvp(child_argv[0], child_argv.data());
  std::cerr << "exec failed for compat command: " << child_argv[0] << ": "
            << std::strerror(errno) << "\n";
  return errno == ENOENT ? 127 : 1;
#endif
}
