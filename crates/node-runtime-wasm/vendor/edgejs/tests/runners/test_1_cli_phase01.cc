#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <string>
#include <vector>

#if !defined(_WIN32)
#include <csignal>
#include <sys/wait.h>
#endif

#include "test_env.h"
#include "node_version.h"
#include "edge_cli.h"
#include "edge_version.h"

class Test1CliPhase01 : public FixtureTestBase {};

namespace {

std::string WriteTempScript(const std::string& stem, const std::string& contents) {
  const auto temp_dir = std::filesystem::temp_directory_path();
  const auto unique_name =
      stem + "_" + std::to_string(static_cast<unsigned long long>(std::hash<std::string>{}(contents))) + ".js";
  const auto script_path = temp_dir / unique_name;
  std::ofstream out(script_path);
  out << contents;
  out.close();
  return script_path.string();
}

void RemoveTempScript(const std::string& path) {
  std::error_code ec;
  std::filesystem::remove(path, ec);
}

std::string GetEnvOrEmpty(const char* name) {
  const char* value = std::getenv(name);
  return value != nullptr ? std::string(value) : std::string();
}

std::filesystem::path ResolveBuiltBinary(const char* name) {
  namespace fs = std::filesystem;
  const fs::path cwd = fs::current_path();
  const std::vector<fs::path> candidates = {
      cwd / name,
      cwd / "build-edge" / name,
      cwd / "build-edge-rename" / name,
      cwd / "build" / name,
      cwd.parent_path() / name,
      cwd.parent_path() / "build-edge" / name,
      cwd.parent_path() / "build-edge-rename" / name,
      cwd.parent_path() / "build" / name,
  };
  for (const auto& candidate : candidates) {
    std::error_code ec;
    if (!fs::exists(candidate, ec) || ec) continue;
    if (fs::is_directory(candidate, ec) || ec) continue;
    return fs::absolute(candidate).lexically_normal();
  }
  return {};
}

std::filesystem::path ResolveBuiltEdgeBinary() {
  return ResolveBuiltBinary("edge");
}

std::filesystem::path ResolveBuiltEdgeenvBinary() {
  return ResolveBuiltBinary("edgeenv");
}

#if defined(PROJECT_ROOT_PATH)
std::filesystem::path ResolveNodeTestScriptPath(const char* relative_path) {
  namespace fs = std::filesystem;
  fs::path test_root(PROJECT_ROOT_PATH "/test");
  if (!test_root.is_absolute()) {
    test_root = fs::absolute(test_root).lexically_normal();
  }
  return fs::absolute(test_root / relative_path).lexically_normal();
}
#endif

std::string ShellSingleQuoted(const std::string& input) {
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

std::string JsSingleQuoted(const std::string& input) {
  std::string out;
  out.reserve(input.size() + 2);
  out.push_back('\'');
  for (char c : input) {
    if (c == '\\' || c == '\'') {
      out.push_back('\\');
    }
    out.push_back(c);
  }
  out.push_back('\'');
  return out;
}

struct CommandResult {
  int status = -1;
  std::string stdout_output;
  std::string stderr_output;
};

CommandResult RunBuiltBinaryAndCapture(const std::filesystem::path& binary,
                                       const std::vector<std::string>& args,
                                       const std::string& stem) {
  namespace fs = std::filesystem;
  std::string unique_key = binary.string();
  for (const auto& arg : args) {
    unique_key.append("\n");
    unique_key.append(arg);
  }

  const fs::path temp_root =
      fs::temp_directory_path() /
      (stem + "_" + std::to_string(static_cast<unsigned long long>(std::hash<std::string>{}(unique_key))));
  const fs::path stdout_path = temp_root / "stdout.txt";
  const fs::path stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(temp_root, ec);

  std::string cmd = ShellSingleQuoted(binary.string());
  for (const auto& arg : args) {
    cmd.push_back(' ');
    cmd += ShellSingleQuoted(arg);
  }
  cmd += " >" + ShellSingleQuoted(stdout_path.string()) + " 2>" + ShellSingleQuoted(stderr_path.string());

  CommandResult result;
  result.status = std::system(cmd.c_str());

  std::ifstream stdout_in(stdout_path);
  result.stdout_output.assign(std::istreambuf_iterator<char>(stdout_in), std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  result.stderr_output.assign(std::istreambuf_iterator<char>(stderr_in), std::istreambuf_iterator<char>());

  fs::remove_all(temp_root, ec);
  return result;
}

}  // namespace

TEST_F(Test1CliPhase01, NoArgsWithStdinEofFallsBackToStdinMode) {
#if defined(_WIN32)
  GTEST_SKIP() << "stdin EOF subprocess check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = std::filesystem::temp_directory_path() / "edge_phase01_cli_no_args";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  std::filesystem::remove_all(temp_root, ec);
  std::filesystem::create_directories(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to create temp directory";

  const std::string cmd =
      ShellSingleQuoted(edge_path.string()) + " </dev/null >" +
      ShellSingleQuoted(stdout_path.string()) + " 2>" +
      ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;
  EXPECT_EQ(WEXITSTATUS(status), 0);

  std::ifstream stderr_in(stderr_path);
  std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                            std::istreambuf_iterator<char>());
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;

  std::filesystem::remove_all(temp_root, ec);
#endif
}

TEST_F(Test1CliPhase01, CompatWrappedCommandsBypassCliParsingAndPrefixPath) {
#if defined(_WIN32)
  GTEST_SKIP() << "compat wrapper subprocess check is POSIX-oriented";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_cli_compat_wrap";
  const auto install_bin_dir = temp_root / "bin";
  const auto compat_dir = temp_root / "bin-compat";
  const auto compat_node = compat_dir / "node";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(install_bin_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create install bin directory";
  fs::create_directories(compat_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create compat directory";

  std::ofstream compat_out(compat_node);
  compat_out
      << "#!/bin/sh\n"
      << "printf 'args=%s\\n' \"$*\"\n"
      << "printf 'path0=%s\\n' \"${PATH%%:*}\"\n"
      << "printf 'edge_binary_path=%s\\n' \"$EDGE_BINARY_PATH\"\n";
  compat_out.close();
  ASSERT_TRUE(compat_out.good()) << "Failed to write compat node shim";
  fs::permissions(
      compat_node,
      fs::perms::owner_read | fs::perms::owner_write | fs::perms::owner_exec |
          fs::perms::group_read | fs::perms::group_exec |
          fs::perms::others_read | fs::perms::others_exec,
      fs::perm_options::replace,
      ec);
  ASSERT_FALSE(ec) << "Failed to chmod compat node shim";

  const std::string old_path = GetEnvOrEmpty("PATH");
  const std::string cmd =
      "EDGE_EXEC_PATH=" + ShellSingleQuoted((install_bin_dir / "edge").string()) + " PATH=" +
      ShellSingleQuoted(old_path) + " " + ShellSingleQuoted(edge_path.string()) +
      " node -p 42 >" + ShellSingleQuoted(stdout_path.string()) +
      " 2>" + ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find("args=-p 42"), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("path0=" + compat_dir.string()), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("edge_binary_path=" + (install_bin_dir / "edge").string()),
            std::string::npos)
      << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, CompatWrappedCommandsUseParentBinCompatFromBuildTreeExecPath) {
#if defined(_WIN32)
  GTEST_SKIP() << "compat wrapper subprocess check is POSIX-oriented";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_cli_compat_build_tree";
  const auto build_dir = temp_root / "build-edge-rename";
  const auto compat_dir = temp_root / "bin-compat";
  const auto compat_node = compat_dir / "node";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(build_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create build directory";
  fs::create_directories(compat_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create compat directory";

  std::ofstream compat_out(compat_node);
  compat_out
      << "#!/bin/sh\n"
      << "printf 'args=%s\\n' \"$*\"\n"
      << "printf 'path0=%s\\n' \"${PATH%%:*}\"\n"
      << "printf 'edge_binary_path=%s\\n' \"$EDGE_BINARY_PATH\"\n";
  compat_out.close();
  ASSERT_TRUE(compat_out.good()) << "Failed to write compat node shim";
  fs::permissions(
      compat_node,
      fs::perms::owner_read | fs::perms::owner_write | fs::perms::owner_exec |
          fs::perms::group_read | fs::perms::group_exec |
          fs::perms::others_read | fs::perms::others_exec,
      fs::perm_options::replace,
      ec);
  ASSERT_FALSE(ec) << "Failed to chmod compat node shim";

  const std::string old_path = GetEnvOrEmpty("PATH");
  const std::string cmd =
      "EDGE_EXEC_PATH=" + ShellSingleQuoted((build_dir / "edge").string()) + " PATH=" +
      ShellSingleQuoted(old_path) + " " + ShellSingleQuoted(edge_path.string()) +
      " node -p 42 >" + ShellSingleQuoted(stdout_path.string()) +
      " 2>" + ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find("args=-p 42"), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("path0=" + compat_dir.string()), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("edge_binary_path=" + (build_dir / "edge").string()),
            std::string::npos)
      << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, EdgeenvAlwaysPrefixesPathAndBypassesCliParsing) {
#if defined(_WIN32)
  GTEST_SKIP() << "compat wrapper subprocess check is POSIX-oriented";
#else
  namespace fs = std::filesystem;
  const auto edgeenv_path = ResolveBuiltEdgeenvBinary();
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edgeenv_path.empty()) << "Failed to resolve built edgeenv binary";
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_edgeenv_wrap";
  const auto build_dir = temp_root / "build-edge-rename";
  const auto compat_dir = temp_root / "bin-compat";
  const auto compat_tool = compat_dir / "custom-tool";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(build_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create build directory";
  fs::create_directories(compat_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create compat directory";

  std::ofstream compat_out(compat_tool);
  compat_out
      << "#!/bin/sh\n"
      << "printf 'args=%s\\n' \"$*\"\n"
      << "printf 'path0=%s\\n' \"${PATH%%:*}\"\n"
      << "printf 'edge_binary_path=%s\\n' \"$EDGE_BINARY_PATH\"\n";
  compat_out.close();
  ASSERT_TRUE(compat_out.good()) << "Failed to write compat tool shim";
  fs::permissions(
      compat_tool,
      fs::perms::owner_read | fs::perms::owner_write | fs::perms::owner_exec |
          fs::perms::group_read | fs::perms::group_exec |
          fs::perms::others_read | fs::perms::others_exec,
      fs::perm_options::replace,
      ec);
  ASSERT_FALSE(ec) << "Failed to chmod compat tool shim";

  const std::string old_path = GetEnvOrEmpty("PATH");
  const std::string cmd =
      "EDGE_EXEC_PATH=" + ShellSingleQuoted((build_dir / "edgeenv").string()) + " PATH=" +
      ShellSingleQuoted(old_path) + " " + ShellSingleQuoted(edgeenv_path.string()) +
      " custom-tool -p 42 >" + ShellSingleQuoted(stdout_path.string()) +
      " 2>" + ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find("args=-p 42"), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("path0=" + compat_dir.string()), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("edge_binary_path=" + edge_path.string()),
            std::string::npos)
      << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, EdgeenvRunsNodeThroughCompatWrapper) {
#if defined(_WIN32)
  GTEST_SKIP() << "edgeenv subprocess check is POSIX-oriented";
#else
  namespace fs = std::filesystem;
  const auto edgeenv_path = ResolveBuiltEdgeenvBinary();
  ASSERT_FALSE(edgeenv_path.empty()) << "Failed to resolve built edgeenv binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_edgeenv_node_alias";
  const auto version_stdout_path = temp_root / "version_stdout.txt";
  const auto version_stderr_path = temp_root / "version_stderr.txt";
  const auto stdin_stdout_path = temp_root / "stdin_stdout.txt";
  const auto stdin_stderr_path = temp_root / "stdin_stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to create temp directory";

  const std::string version_cmd =
      ShellSingleQuoted(edgeenv_path.string()) + " node --version >" +
      ShellSingleQuoted(version_stdout_path.string()) + " 2>" +
      ShellSingleQuoted(version_stderr_path.string());
  const int version_status = std::system(version_cmd.c_str());
  ASSERT_NE(version_status, -1);
  ASSERT_TRUE(WIFEXITED(version_status)) << "status=" << version_status;

  std::ifstream version_stdout_in(version_stdout_path);
  const std::string version_stdout_output((std::istreambuf_iterator<char>(version_stdout_in)),
                                          std::istreambuf_iterator<char>());
  std::ifstream version_stderr_in(version_stderr_path);
  const std::string version_stderr_output((std::istreambuf_iterator<char>(version_stderr_in)),
                                          std::istreambuf_iterator<char>());

  EXPECT_EQ(WEXITSTATUS(version_status), 0) << "stderr=" << version_stderr_output;
  EXPECT_TRUE(version_stderr_output.empty()) << "stderr=" << version_stderr_output;
  EXPECT_NE(version_stdout_output.find(NODE_VERSION_STRING), std::string::npos)
      << version_stdout_output;

  const std::string stdin_cmd =
      ShellSingleQuoted(edgeenv_path.string()) + " node </dev/null >" +
      ShellSingleQuoted(stdin_stdout_path.string()) + " 2>" +
      ShellSingleQuoted(stdin_stderr_path.string());
  const int stdin_status = std::system(stdin_cmd.c_str());
  ASSERT_NE(stdin_status, -1);
  ASSERT_TRUE(WIFEXITED(stdin_status)) << "status=" << stdin_status;

  std::ifstream stdin_stdout_in(stdin_stdout_path);
  const std::string stdin_stdout_output((std::istreambuf_iterator<char>(stdin_stdout_in)),
                                        std::istreambuf_iterator<char>());
  std::ifstream stdin_stderr_in(stdin_stderr_path);
  const std::string stdin_stderr_output((std::istreambuf_iterator<char>(stdin_stderr_in)),
                                        std::istreambuf_iterator<char>());

  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(stdin_status), 0) << "stderr=" << stdin_stderr_output;
  EXPECT_TRUE(stdin_stdout_output.empty()) << "stdout=" << stdin_stdout_output;
  EXPECT_TRUE(stdin_stderr_output.empty()) << "stderr=" << stdin_stderr_output;
#endif
}

TEST_F(Test1CliPhase01, HelpFlagPrintsUsageToStdout) {
#if defined(_WIN32)
  GTEST_SKIP() << "Help subprocess check is POSIX-oriented";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_help_flag";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to create temp directory";

  const std::string cmd =
      ShellSingleQuoted(edge_path.string()) + " --help >" +
      ShellSingleQuoted(stdout_path.string()) + " 2>" +
      ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find("Usage: edge"), std::string::npos) << stdout_output;
  EXPECT_NE(stdout_output.find("--completion-bash"), std::string::npos) << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, StartupTraceEnvIsIgnoredInDefaultBuild) {
#if defined(_WIN32)
  GTEST_SKIP() << "Trace subprocess check is POSIX-oriented";
#elif defined(ENABLE_TRACING)
  GTEST_SKIP() << "Startup tracing is compiled in for this build";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const auto temp_root = fs::temp_directory_path() / "edge_phase01_trace_default";
  const auto stdout_path = temp_root / "stdout.txt";
  const auto stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to create temp directory";

  const std::string cmd =
      "EDGE_STARTUP_TRACE=1 " + ShellSingleQuoted(edge_path.string()) +
      " --version >" + ShellSingleQuoted(stdout_path.string()) +
      " 2>" + ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find(NODE_VERSION_STRING), std::string::npos)
      << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, ExtraArgsAreForwardedToScriptArgv) {
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_extra_args",
      "console.log(process.argv.slice(2).join(','));\n");
  const char* argv[] = {"edge", script_path.c_str(), "alpha", "beta", "gamma"};

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(5, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("alpha,beta,gamma"), std::string::npos);

  RemoveTempScript(script_path);
}

TEST_F(Test1CliPhase01, MissingScriptFileReturnsNonZero) {
  const std::string script_path =
      std::string(NAPI_V8_ROOT_PATH) + "/tests/fixtures/this_file_should_not_exist_phase01.js";
  const char* argv[] = {"edge", script_path.c_str()};
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_NE(error.find("Cannot find module"), std::string::npos) << "error=" << error;
  EXPECT_NE(error.find("MODULE_NOT_FOUND"), std::string::npos) << "error=" << error;
  EXPECT_NE(error.find(script_path), std::string::npos) << "error=" << error;
}

TEST_F(Test1CliPhase01, ValidFixtureScriptReturnsZero) {
  const std::string script_path = std::string(NAPI_V8_ROOT_PATH) + "/tests/fixtures/phase0_hello.js";
  ASSERT_TRUE(std::filesystem::exists(script_path)) << "Missing fixture: " << script_path;
  const char* argv[] = {"edge", script_path.c_str()};

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("hello from edge"), std::string::npos);
}

TEST_F(Test1CliPhase01, RelativeScriptPathWithoutDotPrefixRunsFromCwd) {
  const auto temp_root = std::filesystem::temp_directory_path() / "edge_phase01_relative_entry";
  const auto script_dir = temp_root / "examples";
  const auto script_path = script_dir / "relative_entry.js";
  std::error_code ec;
  std::filesystem::remove_all(temp_root, ec);
  std::filesystem::create_directories(script_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create temp test directories";

  std::ofstream out(script_path);
  out << "console.log('relative entry ok');\n";
  out.close();
  ASSERT_TRUE(out.good()) << "Failed to write temp script";

  const auto original_cwd = std::filesystem::current_path();
  std::filesystem::current_path(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to set cwd to temp root";

  const char* argv[] = {"edge", "examples/relative_entry.js"};
  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  std::filesystem::current_path(original_cwd, ec);
  std::filesystem::remove_all(temp_root, ec);

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("relative entry ok"), std::string::npos);
}

TEST_F(Test1CliPhase01, RelativeScriptPathFallsBackToRepoExamplesDirectory) {
  const auto temp_root = std::filesystem::temp_directory_path() / "edge_phase01_repo_fallback";
  const auto script_dir = temp_root / "examples";
  const auto script_path = script_dir / "fallback_entry.js";
  std::error_code ec;
  std::filesystem::remove_all(temp_root, ec);
  std::filesystem::create_directories(script_dir, ec);
  ASSERT_FALSE(ec) << "Failed to create temp test directories";

  std::ofstream out(script_path);
  out << "console.log('fallback entry ok');\n";
  out.close();
  ASSERT_TRUE(out.good()) << "Failed to write temp script";

  const auto original_cwd = std::filesystem::current_path();
  std::filesystem::current_path(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to set cwd to temp root";

  const char* argv[] = {"edge", "examples/fallback_entry.js"};
  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  std::filesystem::current_path(original_cwd, ec);
  std::filesystem::remove_all(temp_root, ec);

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("fallback entry ok"), std::string::npos);
}

TEST_F(Test1CliPhase01, RuntimeThrownErrorReturnsNonZero) {
  const std::string script_path = WriteTempScript("edge_phase01_cli_throw", "throw new Error('boom from cli');");
  const char* argv[] = {"edge", script_path.c_str()};
  std::string error;

  const int exit_code = EdgeRunCli(2, argv, &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_NE(error.find("boom from cli"), std::string::npos);

  RemoveTempScript(script_path);
}

TEST_F(Test1CliPhase01, RuntimeSyntaxErrorReturnsNonZero) {
  const std::string script_path = WriteTempScript("edge_phase01_cli_syntax", "function (");
  const char* argv[] = {"edge", script_path.c_str()};
  std::string error;

  const int exit_code = EdgeRunCli(2, argv, &error);
  EXPECT_EQ(exit_code, 1);
  EXPECT_FALSE(error.empty());

  RemoveTempScript(script_path);
}

TEST_F(Test1CliPhase01, EvalFlagExecutesSource) {
  const char* argv[] = {"edge", "-e", "console.log('eval-ok')"};
  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(3, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("eval-ok"), std::string::npos);
}

TEST_F(Test1CliPhase01, PrintFlagEvaluatesExpression) {
  const char* argv[] = {"edge", "-p", "40 + 2"};
  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(3, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("42"), std::string::npos);
}

TEST_F(Test1CliPhase01, PrintFlagExposesEdgeProcessVersionEntry) {
  const char* argv[] = {"edge", "-p", "process.versions.edge"};
  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(3, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 0) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find(EDGE_VERSION_STRING), std::string::npos) << stdout_output;
}

TEST_F(Test1CliPhase01, InteractiveWelcomeMessageIncludesEdgeAndNodeVersions) {
#if defined(_WIN32)
  GTEST_SKIP() << "interactive repl subprocess check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path temp_root = fs::temp_directory_path() / "edge_phase01_cli_repl_banner";
  const fs::path stdout_path = temp_root / "stdout.txt";
  const fs::path stderr_path = temp_root / "stderr.txt";
  std::error_code ec;
  fs::remove_all(temp_root, ec);
  fs::create_directories(temp_root, ec);
  ASSERT_FALSE(ec) << "Failed to create temp directory";

  const std::string cmd =
      "printf '.exit\\n' | script -q " +
      ShellSingleQuoted(stdout_path.string()) + " " +
      ShellSingleQuoted(edge_path.string()) + " -i >/dev/null 2>" +
      ShellSingleQuoted(stderr_path.string());
  const int status = std::system(cmd.c_str());
  ASSERT_NE(status, -1);
  ASSERT_TRUE(WIFEXITED(status)) << "status=" << status;

  std::ifstream stdout_in(stdout_path);
  const std::string stdout_output((std::istreambuf_iterator<char>(stdout_in)),
                                  std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  const std::string stderr_output((std::istreambuf_iterator<char>(stderr_in)),
                                  std::istreambuf_iterator<char>());
  fs::remove_all(temp_root, ec);

  EXPECT_EQ(WEXITSTATUS(status), 0) << "stderr=" << stderr_output;
  EXPECT_TRUE(stderr_output.empty()) << "stderr=" << stderr_output;
  EXPECT_NE(stdout_output.find("Welcome to Edge.js " EDGE_VERSION_STRING " (Node.js " NODE_VERSION ")."),
            std::string::npos)
      << stdout_output;
#endif
}

TEST_F(Test1CliPhase01, ScriptFileDoesNotLeakEvalGlobals) {
#if defined(_WIN32)
  GTEST_SKIP() << "script-file global-shape check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_script_globals",
      "const assert = require('assert');\n"
      "for (const key of ['require', '__filename', '__dirname', '__napi_dynamic_import']) {\n"
      "  assert.strictEqual(Object.prototype.hasOwnProperty.call(globalThis, key), false, key);\n"
      "  assert.strictEqual(typeof globalThis[key], 'undefined', key);\n"
      "}\n"
      "const leaked = [];\n"
      "for (const key in globalThis) {\n"
      "  if (key === 'require' || key === '__filename' || key === '__dirname' || key === '__napi_dynamic_import') leaked.push(key);\n"
      "}\n"
      "assert.deepStrictEqual(leaked, []);\n"
      "console.log('script-globals:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_script_globals_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty() ||
              result.stderr_output.find("internal/test/binding") != std::string::npos)
      << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("script-globals:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, EvalModeExposesNodeStyleEvalGlobals) {
#if defined(_WIN32)
  GTEST_SKIP() << "eval global-shape check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"-e",
       "const assert = require('assert');"
       "assert.strictEqual(Object.prototype.hasOwnProperty.call(globalThis,'require'), true);"
       "assert.strictEqual(Object.prototype.propertyIsEnumerable.call(globalThis,'require'), true);"
       "assert.strictEqual(typeof globalThis.require, 'function');"
       "assert.strictEqual(globalThis.__filename, '[eval]');"
       "assert.strictEqual(globalThis.__dirname, '.');"
       "assert.strictEqual(typeof globalThis.__napi_dynamic_import, 'undefined');"
       "console.log('eval-globals:ok');"},
      "edge_phase01_cli_eval_globals_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty() ||
              result.stderr_output.find("internal/test/binding") != std::string::npos)
      << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("eval-globals:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, ConsoleUsesNodeBootstrappedLazyStdioStreams) {
#if defined(_WIN32)
  GTEST_SKIP() << "stdio bootstrap shape check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"-e",
       "const assert = require('assert');"
       "const { Console } = require('console');"
       "const stdoutDesc = Object.getOwnPropertyDescriptor(process, 'stdout');"
       "const stderrDesc = Object.getOwnPropertyDescriptor(process, 'stderr');"
       "assert.strictEqual(typeof stdoutDesc.get, 'function');"
       "assert.strictEqual(stdoutDesc.set, undefined);"
       "assert.strictEqual(Object.prototype.hasOwnProperty.call(stdoutDesc, 'value'), false);"
       "assert.strictEqual(stdoutDesc.enumerable, true);"
       "assert.strictEqual(stdoutDesc.configurable, true);"
       "assert.strictEqual(typeof stderrDesc.get, 'function');"
       "assert.strictEqual(stderrDesc.set, undefined);"
       "assert.strictEqual(Object.prototype.hasOwnProperty.call(stderrDesc, 'value'), false);"
       "assert.strictEqual(stderrDesc.enumerable, true);"
       "assert.strictEqual(stderrDesc.configurable, true);"
       "assert.strictEqual(console instanceof Console, true);"
       "assert.strictEqual(process.stdout._isStdio, true);"
       "assert.strictEqual(process.stderr._isStdio, true);"
       "assert.strictEqual(process.stdout.constructor && process.stdout.constructor.name, 'SyncWriteStream');"
       "assert.strictEqual(process.stderr.constructor && process.stderr.constructor.name, 'SyncWriteStream');"
       "assert.strictEqual(process.stdout._type, 'fs');"
       "assert.strictEqual(process.stderr._type, 'fs');"
       "assert.strictEqual(typeof process.stdout.write, 'function');"
       "assert.strictEqual(typeof process.stderr.write, 'function');"
       "console.log('console-stdio-bootstrap:ok');"},
      "edge_phase01_cli_console_stdio_bootstrap");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("console-stdio-bootstrap:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsPromisesReadFileInsideListenCallbackDoesNotHang) {
#if defined(_WIN32)
  GTEST_SKIP() << "listen/readFile subprocess parity check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path file_path =
      fs::temp_directory_path() / "edge_phase01_listen_readfile_target.txt";
  {
    std::ofstream out(file_path);
    out << "hello-from-readfile";
    ASSERT_TRUE(out.good()) << "Failed to write readFile fixture";
  }

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_listen_readfile",
      "const http = require('http');\n"
      "const fs = require('fs/promises');\n"
      "const timer = setTimeout(() => {\n"
      "  console.error('timeout');\n"
      "  process.exit(9);\n"
      "}, 2000);\n"
      "const server = http.createServer((req, res) => res.end('ok'));\n"
      "server.listen(0, async () => {\n"
      "  try {\n"
      "    const text = await fs.readFile(" + JsSingleQuoted(file_path.string()) + ", 'utf8');\n"
      "    console.log('read:' + text);\n"
      "  } catch (err) {\n"
      "    console.error(err && err.stack || err);\n"
      "    process.exitCode = 1;\n"
      "  } finally {\n"
      "    clearTimeout(timer);\n"
      "    server.close();\n"
      "  }\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_listen_readfile_run");

  std::error_code ec;
  fs::remove(file_path, ec);
  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("read:hello-from-readfile"), std::string::npos) << result.stdout_output;
  EXPECT_EQ(result.stderr_output.find("timeout"), std::string::npos) << result.stderr_output;
#endif
}

TEST_F(Test1CliPhase01, FsPromisesWriteFileInsideRequestDoesNotHang) {
#if defined(_WIN32)
  GTEST_SKIP() << "request/writeFile subprocess parity check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path output_dir =
      fs::temp_directory_path() / ("edge_phase01_request_writefile_" + std::to_string(::getpid()));
  std::error_code ec;
  fs::create_directories(output_dir, ec);

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_request_writefile",
      "const http = require('http');\n"
      "const fs = require('fs/promises');\n"
      "const path = require('path');\n"
      "const payload = Buffer.alloc(512 * 1024, 0x61);\n"
      "let count = 0;\n"
      "const timer = setTimeout(() => {\n"
      "  console.error('timeout');\n"
      "  process.exit(9);\n"
      "}, 4000);\n"
      "const server = http.createServer(async (req, res) => {\n"
      "  try {\n"
      "    count += 1;\n"
      "    const out = path.join(" + JsSingleQuoted(output_dir.string()) + ", 'req-' + count + '.bin');\n"
      "    await fs.writeFile(out, payload);\n"
      "    res.end('ok:' + count);\n"
      "    if (count === 2) {\n"
      "      setImmediate(() => {\n"
      "        clearTimeout(timer);\n"
      "        server.close();\n"
      "      });\n"
      "    }\n"
      "  } catch (err) {\n"
      "    console.error(err && err.stack || err);\n"
      "    process.exitCode = 1;\n"
      "    res.statusCode = 500;\n"
      "    res.end('err');\n"
      "  }\n"
      "});\n"
      "server.listen(0, async () => {\n"
      "  const { port } = server.address();\n"
      "  const getBody = () => new Promise((resolve, reject) => {\n"
      "    http.get({ host: '127.0.0.1', port, path: '/' }, (res) => {\n"
      "      let body = '';\n"
      "      res.setEncoding('utf8');\n"
      "      res.on('data', (chunk) => body += chunk);\n"
      "      res.on('end', () => resolve(body));\n"
      "    }).on('error', reject);\n"
      "  });\n"
      "  try {\n"
      "    console.log(await getBody());\n"
      "    console.log(await getBody());\n"
      "  } catch (err) {\n"
      "    console.error(err && err.stack || err);\n"
      "    process.exitCode = 1;\n"
      "    clearTimeout(timer);\n"
      "    server.close();\n"
      "  }\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_request_writefile_run");

  RemoveTempScript(script_path);
  fs::remove_all(output_dir, ec);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("ok:1"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("ok:2"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsPromisesWritevUsesTypedArrayByteLength) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs.promises.writev typed-array parity check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path file_path =
      fs::temp_directory_path() / ("edge_phase01_writev_typed_array_" + std::to_string(::getpid()) + ".bin");

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_writev_typed_array",
      "const fs = require('fs/promises');\n"
      "(async () => {\n"
      "  const file = " + JsSingleQuoted(file_path.string()) + ";\n"
      "  const fh = await fs.open(file, 'w');\n"
      "  try {\n"
      "    const view = new Uint16Array([0x4241, 0x4443]);\n"
      "    const { bytesWritten } = await fh.writev([view]);\n"
      "    const stat = await fs.stat(file);\n"
      "    console.log('written:' + bytesWritten + ':' + stat.size);\n"
      "  } finally {\n"
      "    await fh.close();\n"
      "  }\n"
      "})().catch((err) => {\n"
      "  console.error(err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_writev_typed_array_run");

  RemoveTempScript(script_path);
  std::error_code ec;
  fs::remove(file_path, ec);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("written:4:4"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, AsyncLocalStoragePromiseContextSurvivesConcurrentAwaits) {
#if defined(_WIN32)
  GTEST_SKIP() << "AsyncLocalStorage subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_async_local_storage_promises",
      "const { AsyncLocalStorage } = require('async_hooks');\n"
      "const als = new AsyncLocalStorage();\n"
      "(async () => {\n"
      "  const pa = als.run('A', async () => {\n"
      "    await Promise.resolve();\n"
      "    await Promise.resolve();\n"
      "    return ['A', als.getStore()];\n"
      "  });\n"
      "  const pb = als.run('B', async () => {\n"
      "    await Promise.resolve();\n"
      "    await Promise.resolve();\n"
      "    return ['B', als.getStore()];\n"
      "  });\n"
      "  console.log(JSON.stringify(await Promise.all([pa, pb])));\n"
      "})().catch((err) => {\n"
      "  console.error(err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_async_local_storage_promises_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("[[\"A\",\"A\"],[\"B\",\"B\"]]"), std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, PromiseDetailsReportSettledQuickjsPromises) {
#if defined(_WIN32)
  GTEST_SKIP() << "promise details subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_promise_details",
      "const assert = require('assert');\n"
      "const { getPromiseDetails } = internalBinding('util');\n"
      "const pending = new Promise(() => {});\n"
      "const fulfilled = Promise.resolve('ok');\n"
      "const rejected = Promise.reject(new Error('boom'));\n"
      "rejected.catch(() => {});\n"
      "const pendingDetails = getPromiseDetails(pending);\n"
      "const fulfilledDetails = getPromiseDetails(fulfilled);\n"
      "const rejectedDetails = getPromiseDetails(rejected);\n"
      "assert.deepStrictEqual(pendingDetails, [0]);\n"
      "assert.strictEqual(fulfilledDetails[0], 1);\n"
      "assert.strictEqual(fulfilledDetails[1], 'ok');\n"
      "assert.strictEqual(rejectedDetails[0], 2);\n"
      "assert.strictEqual(rejectedDetails[1].message, 'boom');\n"
      "console.log(JSON.stringify([pendingDetails[0], fulfilledDetails[0], rejectedDetails[0]]));\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_promise_details_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("[0,1,2]"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, TextDecoderAcceptsSharedArrayBufferInput) {
#if defined(_WIN32)
  GTEST_SKIP() << "SharedArrayBuffer subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_textdecoder_shared_arraybuffer",
      "const sab = new SharedArrayBuffer(3);\n"
      "new Uint8Array(sab).set([0x66, 0x6f, 0x6f]);\n"
      "console.log(new TextDecoder().decode(sab));\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_textdecoder_shared_arraybuffer_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("foo"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, TextDecoderAcceptsArrayBufferViews) {
#if defined(_WIN32)
  GTEST_SKIP() << "TextDecoder subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_textdecoder_arraybuffer_views",
      "const result = [\n"
      "  new TextDecoder().decode(Uint8Array.from([0x66, 0x6f, 0x6f])),\n"
      "  new TextDecoder().decode(Buffer.from('bar')),\n"
      "  new TextDecoder().decode(new DataView(Uint8Array.from([0x62, 0x61, 0x7a]).buffer)),\n"
      "];\n"
      "console.log(JSON.stringify(result));\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_textdecoder_arraybuffer_views_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("[\"foo\",\"bar\",\"baz\"]"), std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, TextDecoderUsesIcuPathForFatalUtf8AndUtf16Be) {
#if defined(_WIN32)
  GTEST_SKIP() << "Encoding subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_textdecoder_icu_semantics",
      "const result = {\n"
      "  fatalCode: (() => {\n"
      "    try {\n"
      "      new TextDecoder('utf-8', { fatal: true }).decode(Uint8Array.from([0xff]));\n"
      "      return 'ok';\n"
      "    } catch (err) {\n"
      "      return err && err.code;\n"
      "    }\n"
      "  })(),\n"
      "  utf16beCodePoints: Array.from(\n"
      "    new TextDecoder('utf-16be').decode(Buffer.from('test\\u20ac', 'utf16le').swap16()),\n"
      "    (ch) => ch.codePointAt(0),\n"
      "  ),\n"
      "};\n"
      "console.log(JSON.stringify(result));\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_textdecoder_icu_semantics_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("\"fatalCode\":\"ERR_ENCODING_INVALID_ENCODED_DATA\""),
            std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"utf16beCodePoints\":[116,101,115,116,8364]"),
            std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, IntlSurfaceMatchesNodeBasics) {
#if defined(_WIN32)
  GTEST_SKIP() << "Intl subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_intl_surface",
      "const result = {\n"
      "  intlType: typeof Intl,\n"
      "  processHasIntl: !!process.config?.variables?.v8_enable_i18n_support,\n"
      "  nfkdSlash: '\\u2100'.normalize('NFKD'),\n"
      "  nfkdAt: '\\uFF20'.normalize('NFKD'),\n"
      "  trLower: 'I'.toLocaleLowerCase('tr'),\n"
      "  trUpper: 'i'.toLocaleUpperCase('tr'),\n"
      "};\n"
      "console.log(JSON.stringify(result));\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_intl_surface_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("\"intlType\":\"object\""), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"processHasIntl\":true"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"nfkdSlash\":\"a/c\""), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"nfkdAt\":\"@\""), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"trLower\":\"ı\""), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"trUpper\":\"İ\""), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, SerdesBindingMatchesNodeContractAndHostObjectSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "Serdes subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_serdes_contract",
      "const { internalBinding } = require('internal/test/binding');\n"
      "const v8 = require('v8');\n"
      "function snap(fn) {\n"
      "  try { fn(); return { ok: true }; } catch (err) {\n"
      "    return { name: err?.name, message: err?.message, code: err?.code };\n"
      "  }\n"
      "}\n"
      "const ser = new v8.Serializer();\n"
      "ser.writeHeader();\n"
      "const out = ser.releaseBuffer();\n"
      "const hostError = snap(() => v8.serialize(new (internalBinding('js_stream').JSStream)()));\n"
      "console.log(JSON.stringify({\n"
      "  isBuffer: Buffer.isBuffer(out),\n"
      "  byteLength: out.byteLength,\n"
      "  bufferByteLength: out.buffer.byteLength,\n"
      "  byteOffset: out.byteOffset,\n"
      "  serializerName: v8.Serializer.name,\n"
      "  deserializerName: v8.Deserializer.name,\n"
      "  deserializerLength: v8.Deserializer.length,\n"
      "  serializerProtoWritable: Object.getOwnPropertyDescriptor(v8.Serializer, 'prototype').writable,\n"
      "  deserializerProtoWritable: Object.getOwnPropertyDescriptor(v8.Deserializer, 'prototype').writable,\n"
      "  noNewSer: snap(() => v8.Serializer()).code,\n"
      "  noNewDer: snap(() => v8.Deserializer()).code,\n"
      "  badWrite: snap(() => { const s = new v8.Serializer(); s.writeHeader(); s.writeRawBytes('x'); }).code,\n"
      "  badDer: snap(() => new v8.Deserializer('x')).code,\n"
      "  hostErrorMessage: hostError.message,\n"
      "}));\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--no-warnings", "--expose-internals", script_path},
      "edge_phase01_cli_serdes_contract_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty() ||
              result.stderr_output.find("internal/test/binding") != std::string::npos)
      << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("\"isBuffer\":true"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"byteLength\":2"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"bufferByteLength\":2"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"byteOffset\":0"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"serializerName\":\"Serializer\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"deserializerName\":\"Deserializer\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"deserializerLength\":1"), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"serializerProtoWritable\":false"), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"deserializerProtoWritable\":false"), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"noNewSer\":\"ERR_CONSTRUCT_CALL_REQUIRED\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"noNewDer\":\"ERR_CONSTRUCT_CALL_REQUIRED\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"badWrite\":\"ERR_INVALID_ARG_TYPE\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"badDer\":\"ERR_INVALID_ARG_TYPE\""), std::string::npos)
      << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("\"hostErrorMessage\":\"Unserializable host object: JSStream {}\""),
            std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, SerdesPassesRawNodeV8SerdesScript) {
#if defined(_WIN32)
  GTEST_SKIP() << "Serdes raw Node subprocess parity check is POSIX-only";
#elif !defined(PROJECT_ROOT_PATH)
  GTEST_SKIP() << "Node test root path is unavailable";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::filesystem::path script_path = ResolveNodeTestScriptPath("parallel/test-v8-serdes.js");
  ASSERT_TRUE(std::filesystem::exists(script_path)) << "Missing script: " << script_path.string();

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--no-warnings", "--expose-internals", script_path.string()},
      "edge_phase01_cli_raw_node_v8_serdes_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty() ||
              result.stderr_output.find("internal/test/binding") != std::string::npos)
      << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stdout_output.empty()) << "stdout=" << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsWatchEmitsChangeAndClosesCleanly) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs.watch subprocess parity check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path file_path =
      fs::temp_directory_path() / ("edge_phase01_fs_watch_" + std::to_string(::getpid()) + ".txt");
  {
    std::ofstream out(file_path);
    out << "start";
  }

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_fs_watch",
      "const fs = require('fs');\n"
      "const file = " + JsSingleQuoted(file_path.string()) + ";\n"
      "const timer = setTimeout(() => {\n"
      "  console.error('timeout');\n"
      "  process.exit(2);\n"
      "}, 4000);\n"
      "const watcher = fs.watch(file, (eventType, filename) => {\n"
      "  console.log('watch:' + eventType + ':' + String(filename));\n"
      "  watcher.close();\n"
      "  clearTimeout(timer);\n"
      "});\n"
      "setTimeout(() => fs.appendFileSync(file, 'x'), 50);\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_fs_watch_run");

  RemoveTempScript(script_path);
  std::error_code ec;
  fs::remove(file_path, ec);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("watch:"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsWatchFileEmitsChangeAndUnwatchWorks) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs.watchFile subprocess parity check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path file_path =
      fs::temp_directory_path() / ("edge_phase01_fs_watchfile_" + std::to_string(::getpid()) + ".txt");
  {
    std::ofstream out(file_path);
    out << "start";
  }

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_fs_watchfile",
      "const fs = require('fs');\n"
      "const file = " + JsSingleQuoted(file_path.string()) + ";\n"
      "const timer = setTimeout(() => {\n"
      "  console.error('timeout');\n"
      "  process.exit(2);\n"
      "}, 6000);\n"
      "fs.watchFile(file, { interval: 50 }, (curr, prev) => {\n"
      "  if (curr.mtimeMs === prev.mtimeMs) return;\n"
      "  console.log('watchFile:' + curr.size + ':' + prev.size);\n"
      "  fs.unwatchFile(file);\n"
      "  clearTimeout(timer);\n"
      "});\n"
      "setTimeout(() => fs.appendFileSync(file, 'x'), 100);\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_fs_watchfile_run");

  RemoveTempScript(script_path);
  std::error_code ec;
  fs::remove(file_path, ec);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("watchFile:"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsHandleWrapCloseAndHasRefMatchNodeSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs handle-wrap semantics check is POSIX-only";
#else
  namespace fs = std::filesystem;
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const fs::path file_path =
      fs::temp_directory_path() / ("edge_phase01_fs_handle_wrap_" + std::to_string(::getpid()) + ".txt");
  {
    std::ofstream out(file_path);
    out << "start";
  }

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_fs_handle_wrap_semantics",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const { FSEvent } = internalBinding('fs_event_wrap');\n"
      "const { StatWatcher } = internalBinding('fs');\n"
      "const file = " + JsSingleQuoted(file_path.string()) + ";\n"
      "const fe = new FSEvent();\n"
      "const feCalls = [];\n"
      "fe.close(() => feCalls.push('nope'));\n"
      "assert.strictEqual(fe.hasRef(), false);\n"
      "assert.strictEqual(fe.start(file, true, false, 'utf8'), 0);\n"
      "assert.strictEqual(fe.initialized, true);\n"
      "assert.strictEqual(fe.hasRef(), true);\n"
      "fe.unref();\n"
      "assert.strictEqual(fe.hasRef(), false);\n"
      "fe.ref();\n"
      "assert.strictEqual(fe.hasRef(), true);\n"
      "fe.close(() => feCalls.push('first'));\n"
      "fe.close(() => feCalls.push('second'));\n"
      "const sw = new StatWatcher(false);\n"
      "const swCalls = [];\n"
      "sw.close(() => swCalls.push('nope'));\n"
      "assert.strictEqual(sw.hasRef(), false);\n"
      "assert.strictEqual(sw.start(file, 25), 0);\n"
      "assert.strictEqual(sw.hasRef(), true);\n"
      "sw.unref();\n"
      "assert.strictEqual(sw.hasRef(), false);\n"
      "sw.ref();\n"
      "assert.strictEqual(sw.hasRef(), true);\n"
      "sw.close(() => swCalls.push('first'));\n"
      "sw.close(() => swCalls.push('second'));\n"
      "setTimeout(() => {\n"
      "  assert.deepStrictEqual(feCalls, ['first']);\n"
      "  assert.deepStrictEqual(swCalls, ['first']);\n"
      "  console.log('fs-handle-wrap:ok');\n"
      "}, 60);\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--expose-internals", script_path},
      "edge_phase01_cli_fs_handle_wrap_semantics_run");

  RemoveTempScript(script_path);
  std::error_code ec;
  fs::remove(file_path, ec);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("fs-handle-wrap:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsPromisesMkdirRecursiveReturnsPromise) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs.promises.mkdir subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_fs_promises_mkdir",
      "const fs = require('fs/promises');\n"
      "const path = require('path');\n"
      "const os = require('os');\n"
      "process.on('unhandledRejection', (err) => {\n"
      "  console.error('unhandledRejection', err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n"
      "(async () => {\n"
      "  const dir = path.join(os.tmpdir(), 'edge-fs-promises-mkdir-' + process.pid, 'a', 'b');\n"
      "  const firstCreated = await fs.mkdir(dir, { recursive: true });\n"
      "  console.log('mkdir:' + firstCreated);\n"
      "})().catch((err) => {\n"
      "  console.error(err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_fs_promises_mkdir_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_EQ(result.stdout_output.find("mkdir:"), 0U) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, FsPromisesCoreFileOpsReturnPromises) {
#if defined(_WIN32)
  GTEST_SKIP() << "fs.promises core file ops subprocess parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_fs_promises_core_ops",
      "const fs = require('fs/promises');\n"
      "const path = require('path');\n"
      "const os = require('os');\n"
      "process.on('unhandledRejection', (err) => {\n"
      "  console.error('unhandledRejection', err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n"
      "(async () => {\n"
      "  const root = path.join(os.tmpdir(), 'edge-fs-promises-core-' + process.pid);\n"
      "  await fs.mkdir(root, { recursive: true });\n"
      "  const a = path.join(root, 'a.txt');\n"
      "  const b = path.join(root, 'b.txt');\n"
      "  const c = path.join(root, 'c.txt');\n"
      "  const link = path.join(root, 'link.txt');\n"
      "  await fs.writeFile(a, 'alpha');\n"
      "  await fs.rename(a, b);\n"
      "  await fs.copyFile(b, c);\n"
      "  await fs.symlink(c, link);\n"
      "  const linkTarget = await fs.readlink(link, 'utf8');\n"
      "  console.log('ops:' + [path.basename(b), path.basename(c), path.basename(linkTarget)].join(','));\n"
      "})().catch((err) => {\n"
      "  console.error(err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_fs_promises_core_ops_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("ops:b.txt,c.txt,c.txt"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, BeforeExitCanScheduleMoreWork) {
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_before_exit_loop",
      "const dns = require('dns');\n"
      "let count = 0;\n"
      "process.on('beforeExit', (code) => {\n"
      "  console.log('beforeExit:' + count + ':' + code);\n"
      "  if (count === 0) {\n"
      "    count += 1;\n"
      "    dns.lookup('localhost', () => {\n"
      "      console.log('lookup-fired');\n"
      "      process.exitCode = 3;\n"
      "    });\n"
      "  }\n"
      "});\n"
      "process.on('exit', (code) => console.log('exit:' + code));\n");
  const char* argv[] = {"edge", script_path.c_str()};

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 3) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("beforeExit:0:0"), std::string::npos);
  EXPECT_NE(stdout_output.find("lookup-fired"), std::string::npos);
  EXPECT_NE(stdout_output.find("beforeExit:1:3"), std::string::npos);
  EXPECT_NE(stdout_output.find("exit:3"), std::string::npos);

  RemoveTempScript(script_path);
}

TEST_F(Test1CliPhase01, ProcessExitCodeWithoutExplicitProcessExitIsReturned) {
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_exit_code_only",
      "process.exitCode = 7;\n"
      "process.on('exit', (code) => console.log('exit:' + code));\n");
  const char* argv[] = {"edge", script_path.c_str()};

  testing::internal::CaptureStdout();
  std::string error;
  const int exit_code = EdgeRunCli(2, argv, &error);
  const std::string stdout_output = testing::internal::GetCapturedStdout();

  EXPECT_EQ(exit_code, 7) << "error=" << error;
  EXPECT_TRUE(error.empty()) << "error=" << error;
  EXPECT_NE(stdout_output.find("exit:7"), std::string::npos);

  RemoveTempScript(script_path);
}

TEST_F(Test1CliPhase01, ExplicitProcessExitDoesNotEmitBeforeExit) {
#if defined(_WIN32)
  GTEST_SKIP() << "explicit process exit CLI parity check is POSIX-only";
#else
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_explicit_exit",
      "process.on('beforeExit', () => console.log('beforeExit'));\n"
      "process.on('exit', (code) => console.log('exit:' + code));\n"
      "process.exit(5);\n");
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_explicit_exit_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 5) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_EQ(result.stdout_output.find("beforeExit"), std::string::npos) << result.stdout_output;
  EXPECT_NE(result.stdout_output.find("exit:5"), std::string::npos) << result.stdout_output;

  RemoveTempScript(script_path);
#endif
}

TEST_F(Test1CliPhase01, ExplicitProcessExitInsideAsyncDoesNotBecomeUnhandledRejection) {
#if defined(_WIN32)
  GTEST_SKIP() << "async process exit CLI parity check is POSIX-only";
#else
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_async_exit",
      "process.on('unhandledRejection', (err) => {\n"
      "  console.error('unhandledRejection', err && err.code, err && err.__ubiExitCode);\n"
      "});\n"
      "(async () => {\n"
      "  await 0;\n"
      "  process.exit(0);\n"
      "})();\n"
      "setTimeout(() => console.log('timer fired'), 50);\n");
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_async_exit_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_EQ(result.stdout_output.find("timer fired"), std::string::npos) << result.stdout_output;
  EXPECT_EQ(result.stderr_output.find("unhandledRejection"), std::string::npos) << result.stderr_output;
  EXPECT_EQ(result.stderr_output.find("ERR_EDGE_PROCESS_EXIT"), std::string::npos) << result.stderr_output;

  RemoveTempScript(script_path);
#endif
}

TEST_F(Test1CliPhase01, ProcessExitFromBeforeExitRunsImmediately) {
#if defined(_WIN32)
  GTEST_SKIP() << "beforeExit process exit CLI parity check is POSIX-only";
#else
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_before_exit_exit",
      "process.on('beforeExit', () => {\n"
      "  console.log('beforeExit');\n"
      "  setTimeout(() => console.log('timer fired'), 5);\n"
      "  process.exit(0);\n"
      "  console.log('afterExit');\n"
      "});\n");
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_before_exit_exit_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("beforeExit"), std::string::npos) << result.stdout_output;
  EXPECT_EQ(result.stdout_output.find("timer fired"), std::string::npos) << result.stdout_output;
  EXPECT_EQ(result.stdout_output.find("afterExit"), std::string::npos) << result.stdout_output;

  RemoveTempScript(script_path);
#endif
}

TEST_F(Test1CliPhase01, ProcessExitCallsMonkeyPatchedReallyExit) {
#if defined(_WIN32)
  GTEST_SKIP() << "process.reallyExit CLI parity check is POSIX-only";
#else
  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_really_exit_patch",
      "process.reallyExit = function(code) {\n"
      "  console.log('really exited:' + code);\n"
      "};\n"
      "process.exit();\n");
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_really_exit_patch_run");

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("really exited:0"), std::string::npos) << result.stdout_output;

  RemoveTempScript(script_path);
#endif
}

TEST_F(Test1CliPhase01, ProcessAbortTerminatesProcess) {
#if defined(_WIN32)
  GTEST_SKIP() << "process.abort CLI parity check is POSIX-only";
#else
  const std::string script_path = WriteTempScript("edge_phase01_cli_abort", "process.abort();\n");
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const CommandResult result = RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_abort_run");

  ASSERT_NE(result.status, -1);
  const bool terminated_by_sigabrt =
      (WIFSIGNALED(result.status) && WTERMSIG(result.status) == SIGABRT) ||
      (WIFEXITED(result.status) && WEXITSTATUS(result.status) == 128 + SIGABRT);
  EXPECT_TRUE(terminated_by_sigabrt) << "status=" << result.status << " stderr=" << result.stderr_output;

  RemoveTempScript(script_path);
#endif
}

TEST_F(Test1CliPhase01, ProcessWrapCloseMatchesHandleWrapNoopAndCallbackSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "process_wrap close semantics check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_process_wrap_close_semantics",
      "const assert = require('assert');\n"
      "const { spawn } = require('child_process');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const Process = internalBinding('process_wrap').Process;\n"
      "let uninitializedCloseCalls = 0;\n"
      "new Process().close(() => { uninitializedCloseCalls++; });\n"
      "setImmediate(() => {\n"
      "  assert.strictEqual(uninitializedCloseCalls, 0);\n"
      "  const cp = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 50)']);\n"
      "  const calls = [];\n"
      "  cp._handle.close(() => calls.push('first'));\n"
      "  cp._handle.close(() => calls.push('second'));\n"
      "  setTimeout(() => {\n"
      "    assert.deepStrictEqual(calls, ['first']);\n"
      "    console.log('process-wrap-close:ok');\n"
      "  }, 25);\n"
      "});\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--expose-internals", script_path},
      "edge_phase01_process_wrap_close_semantics_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("process-wrap-close:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, SignalWrapCloseMatchesHandleWrapCallbackSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "signal_wrap close semantics check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_signal_wrap_close_semantics",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const Signal = internalBinding('signal_wrap').Signal;\n"
      "const signal = new Signal();\n"
      "const calls = [];\n"
      "signal.close(() => calls.push('first'));\n"
      "signal.close(() => calls.push('second'));\n"
      "setTimeout(() => {\n"
      "  assert.deepStrictEqual(calls, ['first']);\n"
      "  console.log('signal-wrap-close:ok');\n"
      "}, 25);\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--expose-internals", script_path},
      "edge_phase01_signal_wrap_close_semantics_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("signal-wrap-close:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, UdpWrapCloseMatchesHandleWrapCallbackAndRefSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "udp_wrap close semantics check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_udp_wrap_close_semantics",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const UDP = internalBinding('udp_wrap').UDP;\n"
      "const handle = new UDP();\n"
      "assert.strictEqual(handle.hasRef(), true);\n"
      "handle.unref();\n"
      "assert.strictEqual(handle.hasRef(), false);\n"
      "handle.ref();\n"
      "assert.strictEqual(handle.hasRef(), true);\n"
      "const calls = [];\n"
      "handle.close(() => calls.push('first'));\n"
      "handle.close(() => calls.push('second'));\n"
      "setTimeout(() => {\n"
      "  assert.deepStrictEqual(calls, ['first']);\n"
      "  console.log('udp-wrap-close:ok');\n"
      "}, 25);\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--expose-internals", script_path},
      "edge_phase01_udp_wrap_close_semantics_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("udp-wrap-close:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, BufferAtobBtoaMatchNodeDomSemantics) {
#if defined(_WIN32)
  GTEST_SKIP() << "buffer atob/btoa CLI parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_buffer_atob_btoa",
      "const assert = require('assert');\n"
      "const buffer = require('buffer');\n"
      "const domDesc = Object.getOwnPropertyDescriptor(globalThis, 'DOMException');\n"
      "const atobDesc = Object.getOwnPropertyDescriptor(globalThis, 'atob');\n"
      "const btoaDesc = Object.getOwnPropertyDescriptor(globalThis, 'btoa');\n"
      "assert.ok(domDesc);\n"
      "assert.ok(atobDesc && typeof atobDesc.get === 'function');\n"
      "assert.ok(btoaDesc && typeof btoaDesc.get === 'function');\n"
      "assert.strictEqual(atobDesc.get.name, 'get atob');\n"
      "assert.strictEqual(btoaDesc.get.name, 'get btoa');\n"
      "assert.strictEqual(domDesc.enumerable, false);\n"
      "assert.strictEqual(domDesc.configurable, true);\n"
      "assert.strictEqual(domDesc.writable, true);\n"
      "assert.strictEqual(typeof domDesc.value, 'function');\n"
      "assert.strictEqual(DOMException.INVALID_CHARACTER_ERR, 5);\n"
      "assert.strictEqual(new DOMException('x', 'InvalidCharacterError').code, 5);\n"
      "assert.strictEqual(buffer.atob('  Y\\fW\\tJ\\njZ A=\\r= '), 'abcd');\n"
      "assert.strictEqual(buffer.atob(null), '\\x9E\\xE9e');\n"
      "assert.strictEqual(buffer.btoa('abcd'), 'YWJjZA==');\n"
      "assert.throws(() => buffer.atob('a'), "
      "{ constructor: DOMException, name: 'InvalidCharacterError', code: 5 });\n"
      "assert.throws(() => buffer.btoa('我要抛错！'), { name: 'InvalidCharacterError' });\n"
      "console.log('buffer-atob-btoa:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_buffer_atob_btoa_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("buffer-atob-btoa:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, InternalBufferValidationAcceptsDataViewAndSharedArrayBuffer) {
#if defined(_WIN32)
  GTEST_SKIP() << "buffer validation CLI parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_internal_buffer_validation_inputs",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const { isUtf8, isAscii } = internalBinding('buffer');\n"
      "const sab = new SharedArrayBuffer(5);\n"
      "new Uint8Array(sab).set([0x68, 0x65, 0x6c, 0x6c, 0x6f]);\n"
      "const sabView = new DataView(sab, 0, 5);\n"
      "assert.strictEqual(isUtf8(sab), true);\n"
      "assert.strictEqual(isUtf8(sabView), true);\n"
      "assert.strictEqual(isAscii(sabView), true);\n"
      "const invalid = new Uint8Array([0xc0, 0x80]);\n"
      "assert.strictEqual(isUtf8(new DataView(invalid.buffer)), false);\n"
      "assert.throws(() => isAscii('hello'), { code: 'ERR_INVALID_ARG_TYPE' });\n"
      "console.log('internal-buffer-validation:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {"--expose-internals", script_path},
          "edge_phase01_cli_internal_buffer_validation_inputs_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("internal-buffer-validation:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, InternalBufferBindingSentinelsAndSharedArrayBufferCopyMatchNode) {
#if defined(_WIN32)
  GTEST_SKIP() << "buffer internal binding CLI parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_internal_buffer_binding",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const binding = internalBinding('buffer');\n"
      "const dst = new SharedArrayBuffer(4);\n"
      "const src = new SharedArrayBuffer(4);\n"
      "new Uint8Array(src).set([1, 2, 3, 4]);\n"
      "binding.copyArrayBuffer(dst, 1, src, 0, 3);\n"
      "assert.deepStrictEqual(Array.from(new Uint8Array(dst)), [0, 1, 2, 3]);\n"
      "assert.strictEqual(binding.fill(Buffer.alloc(4), 1, 0, 4), undefined);\n"
      "assert.strictEqual(binding.fill(Buffer.alloc(4), 'zz', 0, 4, 'hex'), -1);\n"
      "assert.strictEqual(binding.fill(Buffer.alloc(4), 1, 3, 2), -2);\n"
      "const toggle = binding.getZeroFillToggle();\n"
      "assert.ok(toggle instanceof Uint32Array);\n"
      "assert.strictEqual(toggle.length, 1);\n"
      "toggle[0] = 1;\n"
      "assert.ok(Array.from(new Uint8Array(binding.createUnsafeArrayBuffer(16))).every((v) => v === 0));\n"
      "console.log('internal-buffer-binding:ok');\n");

  const CommandResult result = RunBuiltBinaryAndCapture(
      edge_path,
      {"--expose-internals", script_path},
      "edge_phase01_cli_internal_buffer_binding_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("internal-buffer-binding:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, OsUserInfoBufferEncodingReturnsBuffersAndNullPrototype) {
#if defined(_WIN32)
  GTEST_SKIP() << "os.userInfo buffer parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_os_userinfo_buffer",
      "const assert = require('assert');\n"
      "const os = require('os');\n"
      "const user = os.userInfo();\n"
      "const userBuf = os.userInfo({ encoding: 'buffer' });\n"
      "assert.strictEqual(Object.getPrototypeOf(user), null);\n"
      "assert.strictEqual(Object.getPrototypeOf(userBuf), null);\n"
      "assert.ok(Buffer.isBuffer(userBuf.username));\n"
      "assert.ok(Buffer.isBuffer(userBuf.homedir));\n"
      "assert.strictEqual(user.username, userBuf.username.toString('utf8'));\n"
      "assert.strictEqual(user.homedir, userBuf.homedir.toString('utf8'));\n"
      "if (userBuf.shell === null) {\n"
      "  assert.strictEqual(user.shell, null);\n"
      "} else {\n"
      "  assert.ok(Buffer.isBuffer(userBuf.shell));\n"
      "  assert.strictEqual(user.shell, userBuf.shell.toString('utf8'));\n"
      "}\n"
      "console.log('os-userinfo-buffer:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(edge_path, {script_path}, "edge_phase01_cli_os_userinfo_buffer_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("os-userinfo-buffer:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, InternalOsBindingGetHomeDirectoryHasNodeDescriptorShape) {
#if defined(_WIN32)
  GTEST_SKIP() << "os internal binding descriptor parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_os_binding_descriptor",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const binding = internalBinding('os');\n"
      "const desc = Object.getOwnPropertyDescriptor(binding, 'getHomeDirectory');\n"
      "assert.ok(desc);\n"
      "assert.strictEqual(typeof desc.value, 'function');\n"
      "assert.strictEqual(desc.get, undefined);\n"
      "assert.strictEqual(desc.set, undefined);\n"
      "assert.strictEqual(desc.enumerable, true);\n"
      "assert.strictEqual(desc.configurable, true);\n"
      "binding.getHomeDirectory = function(ctx) {\n"
      "  ctx.syscall = 'foo';\n"
      "  ctx.code = 'bar';\n"
      "  ctx.message = 'baz';\n"
      "};\n"
      "const os = require('os');\n"
      "assert.throws(\n"
      "  () => os.homedir(),\n"
      "  (err) => err && err.code === 'ERR_SYSTEM_ERROR' &&\n"
      "      err.message === 'A system error occurred: foo returned bar (baz)');\n"
      "console.log('os-binding-descriptor:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {"--expose-internals", script_path},
          "edge_phase01_cli_os_binding_descriptor_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("os-binding-descriptor:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, OsConstantsExposeNodeLikeShapeAndCoverage) {
#if defined(_WIN32)
  GTEST_SKIP() << "os constants parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_os_constants_shape",
      "const assert = require('assert');\n"
      "const { internalBinding } = require('internal/test/binding');\n"
      "const osConstants = internalBinding('constants').os;\n"
      "assert.strictEqual(Object.getPrototypeOf(osConstants), null);\n"
      "assert.strictEqual(Object.getPrototypeOf(osConstants.errno), null);\n"
      "assert.strictEqual(Object.getPrototypeOf(osConstants.signals), null);\n"
      "assert.strictEqual(Object.getPrototypeOf(osConstants.priority), null);\n"
      "assert.strictEqual(Object.getPrototypeOf(osConstants.dlopen), null);\n"
      "assert.ok(Object.keys(osConstants.errno).length >= 50);\n"
      "assert.ok(Object.keys(osConstants.signals).length >= 25);\n"
      "assert.ok('EALREADY' in osConstants.errno);\n"
      "assert.ok('ECANCELED' in osConstants.errno);\n"
      "assert.ok('ENOBUFS' in osConstants.errno);\n"
      "assert.ok('ENOTEMPTY' in osConstants.errno);\n"
      "assert.ok('PRIORITY_NORMAL' in osConstants.priority);\n"
      "assert.ok('PRIORITY_HIGH' in osConstants.priority);\n"
      "assert.ok('UV_UDP_REUSEADDR' in osConstants);\n"
      "assert.ok(Object.isFrozen(osConstants.signals));\n"
      "if (process.platform !== 'win32') {\n"
      "  assert.ok('SIGBUS' in osConstants.signals);\n"
      "  assert.ok('SIGFPE' in osConstants.signals);\n"
      "  assert.ok('SIGSEGV' in osConstants.signals);\n"
      "}\n"
      "console.log('os-constants-shape:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {"--expose-internals", script_path},
          "edge_phase01_cli_os_constants_shape_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  if (!result.stderr_output.empty()) {
    EXPECT_NE(result.stderr_output.find("internal/test/binding"), std::string::npos) << result.stderr_output;
  }
  EXPECT_NE(result.stdout_output.find("os-constants-shape:ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, SpawnSyncBindingMatchesNodeOptionParsingForCoreFields) {
#if defined(_WIN32)
  GTEST_SKIP() << "spawn_sync binding option-parity check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_spawn_sync_binding_options",
      "const assert = require('assert');\n"
      "const binding = process.binding('spawn_sync');\n"
      "const base = {\n"
      "  file: process.execPath,\n"
      "  args: [process.execPath, '-e', ''],\n"
      "  stdio: [],\n"
      "};\n"
      "let result = binding.spawn({ ...base, args: 1 });\n"
      "assert.strictEqual(result.error, -22);\n"
      "assert.strictEqual(result.output, null);\n"
      "result = binding.spawn({ ...base, envPairs: 1 });\n"
      "assert.strictEqual(result.error, -22);\n"
      "assert.strictEqual(result.output, null);\n"
      "result = binding.spawn({ ...base, detached: true, killSignal: 0 });\n"
      "assert.strictEqual(result.error, undefined);\n"
      "assert.strictEqual(result.status, 0);\n"
      "assert.ok(Array.isArray(result.output));\n"
      "if (typeof process.getuid === 'function') {\n"
      "  result = binding.spawn({ ...base, uid: process.getuid() });\n"
      "  assert.strictEqual(result.error, undefined);\n"
      "  assert.strictEqual(result.status, 0);\n"
      "}\n"
      "if (typeof process.getgid === 'function') {\n"
      "  result = binding.spawn({ ...base, gid: process.getgid() });\n"
      "  assert.strictEqual(result.error, undefined);\n"
      "  assert.strictEqual(result.status, 0);\n"
      "}\n"
      "console.log('spawn-sync-binding-options:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {script_path},
          "edge_phase01_cli_spawn_sync_binding_options_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("spawn-sync-binding-options:ok"), std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, SpawnSyncBindingBuildsNodeStyleOutputForExtraPipeFds) {
#if defined(_WIN32)
  GTEST_SKIP() << "spawn_sync binding output-shape check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_spawn_sync_binding_output_shape",
      "const assert = require('assert');\n"
      "const binding = process.binding('spawn_sync');\n"
      "const result = binding.spawn({\n"
      "  file: process.execPath,\n"
      "  args: [process.execPath, '-e', \"require('fs').writeSync(3, 'fd3-out')\"],\n"
      "  stdio: ['ignore', 'ignore', 'ignore', 'pipe'],\n"
      "});\n"
      "assert.strictEqual(result.error, undefined);\n"
      "assert.strictEqual(result.status, 0);\n"
      "assert.ok(Array.isArray(result.output));\n"
      "assert.strictEqual(result.output.length, 4);\n"
      "assert.strictEqual(result.output[0], null);\n"
      "assert.strictEqual(result.output[1], null);\n"
      "assert.strictEqual(result.output[2], null);\n"
      "assert.strictEqual(result.output[3].toString(), 'fd3-out');\n"
      "console.log('spawn-sync-binding-output-shape:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {script_path},
          "edge_phase01_cli_spawn_sync_binding_output_shape_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("spawn-sync-binding-output-shape:ok"), std::string::npos)
      << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, CliResolvesEntryWithoutJsExtensionLikeNode) {
#if defined(_WIN32)
  GTEST_SKIP() << "entry resolution check is POSIX-only";
#else
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_entry_noext",
      "console.log('entry-noext-ok');\n");
  ASSERT_GE(script_path.size(), 3u);
  const std::string extensionless = script_path.substr(0, script_path.size() - 3);

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {extensionless},
          "edge_phase01_cli_entry_noext_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("entry-noext-ok"), std::string::npos) << result.stdout_output;
#endif
}

TEST_F(Test1CliPhase01, MessagePortMatchesNodeLocalTransferCloseAndRefSemantics) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_messageport_parity",
      "const assert = require('assert');\n"
      "const { MessageChannel, receiveMessageOnPort } = require('worker_threads');\n"
      "(async () => {\n"
      "  const channel = new MessageChannel();\n"
      "  assert.strictEqual(channel.port1.hasRef(), false);\n"
      "  channel.port1.ref();\n"
      "  assert.strictEqual(channel.port1.hasRef(), true);\n"
      "  channel.port1.unref();\n"
      "  assert.strictEqual(channel.port1.hasRef(), false);\n"
      "  const typedArray = new Uint8Array([0, 1, 2, 3, 4]);\n"
      "  channel.port2.postMessage({ typedArray }, [typedArray.buffer]);\n"
      "  assert.strictEqual(typedArray.buffer.byteLength, 0);\n"
      "  const first = receiveMessageOnPort(channel.port1);\n"
      "  assert.deepStrictEqual(Array.from(first.message.typedArray), [0, 1, 2, 3, 4]);\n"
      "  for (const value of [null, 0, -1, {}, []]) {\n"
      "    assert.throws(() => receiveMessageOnPort(value), {\n"
      "      name: 'TypeError',\n"
      "      code: 'ERR_INVALID_ARG_TYPE',\n"
      "      message: 'The \"port\" argument must be a MessagePort instance'\n"
      "    });\n"
      "  }\n"
      "  const late = new MessageChannel();\n"
      "  const seen = [];\n"
      "  late.port2.postMessage('firstMessage');\n"
      "  late.port2.postMessage('lastMessage');\n"
      "  late.port2.close();\n"
      "  late.port1.on('message', (message) => seen.push(message));\n"
      "  await new Promise((resolve) => setTimeout(resolve, 20));\n"
      "  assert.deepStrictEqual(seen, ['firstMessage', 'lastMessage']);\n"
      "  console.log('messageport-parity:ok');\n"
      "})().catch((err) => {\n"
      "  console.error(err && err.stack || err);\n"
      "  process.exitCode = 1;\n"
      "});\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {script_path},
          "edge_phase01_cli_messageport_parity_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("messageport-parity:ok"), std::string::npos)
      << result.stdout_output;
}

TEST_F(Test1CliPhase01, MessagingStructuredCloneSupportsSharedArrayBuffer) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  const std::string script_path = WriteTempScript(
      "edge_phase01_cli_structured_clone_sab",
      "const assert = require('assert');\n"
      "const sab = new SharedArrayBuffer(8);\n"
      "const original = new Uint8Array(sab);\n"
      "original[0] = 7;\n"
      "original[1] = 13;\n"
      "const clone = structuredClone({ sab }).sab;\n"
      "assert.notStrictEqual(clone, sab);\n"
      "const cloned = new Uint8Array(clone);\n"
      "assert.strictEqual(cloned[0], 7);\n"
      "assert.strictEqual(cloned[1], 13);\n"
      "original[2] = 29;\n"
      "assert.strictEqual(cloned[2], 29);\n"
      "console.log('structured-clone-sab:ok');\n");

  const CommandResult result =
      RunBuiltBinaryAndCapture(
          edge_path,
          {script_path},
          "edge_phase01_cli_structured_clone_sab_run");

  RemoveTempScript(script_path);

  ASSERT_NE(result.status, -1);
  ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
  EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
  EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
  EXPECT_NE(result.stdout_output.find("structured-clone-sab:ok"), std::string::npos)
      << result.stdout_output;
}

// TEST_F(Test1CliPhase01, WorkerMessagePortRequiresTransferListLikeNode) {
//   const auto edge_path = ResolveBuiltEdgeBinary();
//   ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

//   const std::string script_path = WriteTempScript(
//       "edge_phase01_cli_worker_port_transfer_validation",
//       "const assert = require('assert');\n"
//       "const { Worker, MessageChannel } = require('worker_threads');\n"
//       "const channel = new MessageChannel();\n"
//       "assert.throws(() => new Worker('0', {\n"
//       "  eval: true,\n"
//       "  workerData: { message: channel.port1 },\n"
//       "  transferList: [],\n"
//       "}), {\n"
//       "  constructor: DOMException,\n"
//       "  name: 'DataCloneError',\n"
//       "  code: 25,\n"
//       "  message: 'Object that needs transfer was found in message but not listed in transferList',\n"
//       "});\n"
//       "console.log('worker-port-transfer-validation:ok');\n");

//   const CommandResult result =
//       RunBuiltBinaryAndCapture(
//           edge_path,
//           {script_path},
//           "edge_phase01_cli_worker_port_transfer_validation_run");

//   RemoveTempScript(script_path);

//   ASSERT_NE(result.status, -1);
//   ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
//   EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
//   EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
//   EXPECT_NE(result.stdout_output.find("worker-port-transfer-validation:ok"), std::string::npos)
//       << result.stdout_output;
// }

// TEST_F(Test1CliPhase01, MessagingMatchesNodeBroadcastAndTransferEdgeSemantics) {
//   const auto edge_path = ResolveBuiltEdgeBinary();
//   ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

//   const std::string script_path = WriteTempScript(
//       "edge_phase01_cli_messaging_edge_parity",
//       "const assert = require('assert');\n"
//       "const { BroadcastChannel, MessageChannel, moveMessagePortToContext } = require('worker_threads');\n"
//       "(async () => {\n"
//       "  const channelName = 'edge-phase01-broadcast-' + process.pid;\n"
//       "  const bc1 = new BroadcastChannel(channelName);\n"
//       "  const bc2 = new BroadcastChannel(channelName);\n"
//       "  const seen = [];\n"
//       "  bc2.onmessage = ({ data }) => seen.push(data);\n"
//       "  bc1.postMessage('hello');\n"
//       "  await new Promise((resolve) => setTimeout(resolve, 20));\n"
//       "  assert.deepStrictEqual(seen, ['hello']);\n"
//       "  bc1.close();\n"
//       "  bc2.close();\n"
//       "\n"
//       "  {\n"
//       "    const { port1 } = new MessageChannel();\n"
//       "    assert.throws(() => port1.postMessage('x', [port1]), {\n"
//       "      constructor: DOMException,\n"
//       "      name: 'DataCloneError',\n"
//       "      code: 25,\n"
//       "      message: 'Transfer list contains source port',\n"
//       "    });\n"
//       "    port1.close();\n"
//       "  }\n"
//       "\n"
//       "  {\n"
//       "    const { port1, port2 } = new MessageChannel();\n"
//       "    const warnings = [];\n"
//       "    const originalEmitWarning = process.emitWarning;\n"
//       "    process.emitWarning = (warning) => { warnings.push(String(warning)); };\n"
//       "    let closeCount = 0;\n"
//       "    port1.on('close', () => closeCount++);\n"
//       "    port2.on('close', () => closeCount++);\n"
//       "    const arrayBuf = new ArrayBuffer(4);\n"
//       "    assert.strictEqual(port2.postMessage(null, [port1, arrayBuf]), true);\n"
//       "    assert.strictEqual(arrayBuf.byteLength, 0);\n"
//       "    await new Promise((resolve) => setTimeout(resolve, 20));\n"
//       "    process.emitWarning = originalEmitWarning;\n"
//       "    assert.deepStrictEqual(warnings, [\n"
//       "      'The target port was posted to itself, and the communication channel was lost',\n"
//       "    ]);\n"
//       "    assert.strictEqual(closeCount, 2);\n"
//       "  }\n"
//       "\n"
//       "  {\n"
//       "    const { port1 } = new MessageChannel();\n"
//       "    port1.close();\n"
//       "    assert.throws(() => moveMessagePortToContext(port1, {}), {\n"
//       "      code: 'ERR_CLOSED_MESSAGE_PORT',\n"
//       "      message: 'Cannot send data on closed MessagePort',\n"
//       "    });\n"
//       "  }\n"
//       "\n"
//       "  console.log('messaging-edge-parity:ok');\n"
//       "})().catch((err) => {\n"
//       "  console.error(err && err.stack || err);\n"
//       "  process.exitCode = 1;\n"
//       "});\n");

//   const CommandResult result =
//       RunBuiltBinaryAndCapture(
//           edge_path,
//           {script_path},
//           "edge_phase01_cli_messaging_edge_parity_run");

//   RemoveTempScript(script_path);

//   ASSERT_NE(result.status, -1);
//   ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
//   EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
//   EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
//   EXPECT_NE(result.stdout_output.find("messaging-edge-parity:ok"), std::string::npos)
//       << result.stdout_output;
// }

// TEST_F(Test1CliPhase01, WorkerShareEnvAndTerminateMatchNode) {
//   const auto edge_path = ResolveBuiltEdgeBinary();
//   ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

//   const std::string script_path = WriteTempScript(
//       "edge_phase01_cli_worker_share_env_terminate",
//       "const assert = require('assert');\n"
//       "const { once } = require('events');\n"
//       "const { Worker, SHARE_ENV } = require('worker_threads');\n"
//       "(async () => {\n"
//       "  process.env.EDGE_PHASE01_SHARED = 'main-start';\n"
//       "  const sharedWorker = new Worker(`\n"
//       "    const { parentPort } = require('worker_threads');\n"
//       "    process.env.EDGE_PHASE01_SHARED = 'shared-worker';\n"
//       "    setTimeout(() => parentPort.postMessage({\n"
//       "      value: process.env.EDGE_PHASE01_SHARED,\n"
//       "      has: Object.prototype.hasOwnProperty.call(process.env, 'EDGE_PHASE01_SHARED'),\n"
//       "      keys: Object.keys(process.env).includes('EDGE_PHASE01_SHARED'),\n"
//       "    }), 10);\n"
//       "    setInterval(() => {}, 1000);\n"
//       "  `, { eval: true, env: SHARE_ENV });\n"
//       "  const sharedErrors = [];\n"
//       "  sharedWorker.on('error', (err) => sharedErrors.push(String(err && (err.stack || err))));\n"
//       "  const [sharedMessage] = await once(sharedWorker, 'message');\n"
//       "  assert.deepStrictEqual(sharedMessage, {\n"
//       "    value: 'shared-worker',\n"
//       "    has: true,\n"
//       "    keys: true,\n"
//       "  });\n"
//       "  assert.strictEqual(process.env.EDGE_PHASE01_SHARED, 'shared-worker');\n"
//       "  assert.strictEqual(Object.prototype.hasOwnProperty.call(process.env, 'EDGE_PHASE01_SHARED'), true);\n"
//       "  assert.strictEqual(Object.keys(process.env).includes('EDGE_PHASE01_SHARED'), true);\n"
//       "  assert.strictEqual(await sharedWorker.terminate(), 1);\n"
//       "  assert.deepStrictEqual(sharedErrors, []);\n"
//       "\n"
//       "  process.env.EDGE_PHASE01_COPY = 'main-copy';\n"
//       "  const copyWorker = new Worker(`\n"
//       "    const { parentPort } = require('worker_threads');\n"
//       "    process.env.EDGE_PHASE01_COPY = 'copy-worker';\n"
//       "    parentPort.postMessage({\n"
//       "      value: process.env.EDGE_PHASE01_COPY,\n"
//       "      has: Object.prototype.hasOwnProperty.call(process.env, 'EDGE_PHASE01_COPY'),\n"
//       "      keys: Object.keys(process.env).includes('EDGE_PHASE01_COPY'),\n"
//       "    });\n"
//       "    setInterval(() => {}, 1000);\n"
//       "  `, { eval: true, env: null });\n"
//       "  const [copyMessage] = await once(copyWorker, 'message');\n"
//       "  assert.deepStrictEqual(copyMessage, {\n"
//       "    value: 'copy-worker',\n"
//       "    has: true,\n"
//       "    keys: true,\n"
//       "  });\n"
//       "  assert.strictEqual(process.env.EDGE_PHASE01_COPY, 'main-copy');\n"
//       "  assert.strictEqual(await copyWorker.terminate(), 1);\n"
//       "\n"
//       "  delete process.env.EDGE_PHASE01_SHARED;\n"
//       "  delete process.env.EDGE_PHASE01_COPY;\n"
//       "  console.log('worker-share-env-terminate:ok');\n"
//       "})().catch((err) => {\n"
//       "  console.error(err && err.stack || err);\n"
//       "  process.exitCode = 1;\n"
//       "});\n");

//   const CommandResult result =
//       RunBuiltBinaryAndCapture(
//           edge_path,
//           {script_path},
//           "edge_phase01_cli_worker_share_env_terminate_run");

//   RemoveTempScript(script_path);

//   ASSERT_NE(result.status, -1);
//   ASSERT_TRUE(WIFEXITED(result.status)) << "status=" << result.status;
//   EXPECT_EQ(WEXITSTATUS(result.status), 0) << "stderr=" << result.stderr_output;
//   EXPECT_TRUE(result.stderr_output.empty()) << "stderr=" << result.stderr_output;
//   EXPECT_NE(result.stdout_output.find("worker-share-env-terminate:ok"), std::string::npos)
//       << result.stdout_output;
// }
