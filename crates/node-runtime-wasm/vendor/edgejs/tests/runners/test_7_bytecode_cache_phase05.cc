#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <functional>
#include <string>
#include <vector>

#if !defined(_WIN32)
#include <sys/wait.h>
#endif

#include "test_env.h"
#include "edge_bytecode_cache.h"

class Test7BytecodeCachePhase05 : public FixtureTestBase {};

namespace {

namespace fs = std::filesystem;

constexpr const char kTag[] = "v8-test-tag";
constexpr const char kSource[] = "module.exports = 42;\n";

std::vector<uint8_t> SamplePayload() {
  return {0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03};
}

std::filesystem::path ResolveBuiltEdgeBinary() {
  const fs::path cwd = fs::current_path();
  const std::vector<fs::path> candidates = {
      cwd / "edge",
      cwd / "build-edge" / "edge",
      cwd / "build" / "edge",
      cwd.parent_path() / "edge",
      cwd.parent_path() / "build-edge" / "edge",
      cwd.parent_path() / "build" / "edge",
  };
  for (const auto& candidate : candidates) {
    std::error_code ec;
    if (!fs::exists(candidate, ec) || ec) continue;
    if (fs::is_directory(candidate, ec) || ec) continue;
    return fs::absolute(candidate).lexically_normal();
  }
  return {};
}

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

struct CommandResult {
  int exit_code = -1;
  std::string stdout_output;
  std::string stderr_output;
};

CommandResult RunEdge(const fs::path& binary,
                      const std::vector<std::string>& args,
                      const fs::path& temp_root,
                      const std::string& env_prefix = "") {
  const fs::path stdout_path = temp_root / "cmd_stdout.txt";
  const fs::path stderr_path = temp_root / "cmd_stderr.txt";

  std::string cmd = env_prefix;
  if (!cmd.empty()) cmd.push_back(' ');
  cmd += ShellSingleQuoted(binary.string());
  for (const auto& arg : args) {
    cmd.push_back(' ');
    cmd += ShellSingleQuoted(arg);
  }
  cmd += " >" + ShellSingleQuoted(stdout_path.string()) + " 2>" + ShellSingleQuoted(stderr_path.string());

  CommandResult result;
  const int status = std::system(cmd.c_str());
#if !defined(_WIN32)
  if (status != -1 && WIFEXITED(status)) result.exit_code = WEXITSTATUS(status);
#else
  result.exit_code = status;
#endif
  std::ifstream stdout_in(stdout_path);
  result.stdout_output.assign(std::istreambuf_iterator<char>(stdout_in), std::istreambuf_iterator<char>());
  std::ifstream stderr_in(stderr_path);
  result.stderr_output.assign(std::istreambuf_iterator<char>(stderr_in), std::istreambuf_iterator<char>());
  return result;
}

class TempProjectDir {
 public:
  explicit TempProjectDir(const std::string& stem) {
    root_ = fs::temp_directory_path() / (stem + "_" + std::to_string(static_cast<unsigned long long>(
                                                          std::hash<std::string>{}(stem))));
    std::error_code ec;
    fs::remove_all(root_, ec);
    fs::create_directories(root_, ec);
  }
  ~TempProjectDir() {
    std::error_code ec;
    fs::remove_all(root_, ec);
  }
  const fs::path& root() const { return root_; }
  fs::path Write(const std::string& relative, const std::string& contents) const {
    const fs::path path = root_ / relative;
    std::error_code ec;
    fs::create_directories(path.parent_path(), ec);
    std::ofstream out(path, std::ios::binary);
    out << contents;
    return path;
  }

 private:
  fs::path root_;
};

}  // namespace

TEST_F(Test7BytecodeCachePhase05, Hash64MatchesKnownVectors) {
  // XXH3-64 reference vectors.
  EXPECT_EQ(edge_bytecode_cache::Hash64("", 0), 0x2D06800538D394C2ull);
  // Determinism + sensitivity.
  EXPECT_EQ(edge_bytecode_cache::Hash64("abc", 3), edge_bytecode_cache::Hash64("abc", 3));
  EXPECT_NE(edge_bytecode_cache::Hash64("abc", 3), edge_bytecode_cache::Hash64("abd", 3));
}

TEST_F(Test7BytecodeCachePhase05, EncodeDecodeRoundTrip) {
  const auto payload = SamplePayload();
  const auto encoded =
      edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  size_t offset = 0;
  size_t size = 0;
  ASSERT_TRUE(edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size(), kTag, kSource,
                                                 edge_bytecode_cache::kFlagCjsFunctionV1, &offset, &size));
  ASSERT_EQ(size, payload.size());
  EXPECT_EQ(std::vector<uint8_t>(encoded.begin() + offset, encoded.begin() + offset + size), payload);
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsBadMagic) {
  const auto payload = SamplePayload();
  auto encoded = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  encoded[0] ^= 0xff;
  size_t off = 0;
  size_t len = 0;
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size(), kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsBadFormatVersion) {
  const auto payload = SamplePayload();
  auto encoded = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  encoded[8] ^= 0xff;
  size_t off = 0;
  size_t len = 0;
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size(), kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsWrongEngineTag) {
  const auto payload = SamplePayload();
  const auto encoded = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  size_t off = 0;
  size_t len = 0;
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size(), "v8-other-tag", kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                                  &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsSourceMismatch) {
  const auto payload = SamplePayload();
  const auto encoded = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  size_t off = 0;
  size_t len = 0;
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size(), kTag,
                                                  "module.exports = 43;\n",
                                                  edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsTruncatedAndCorruptedPayload) {
  const auto payload = SamplePayload();
  const auto encoded = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                         payload.data(), payload.size());
  size_t off = 0;
  size_t len = 0;

  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(encoded.data(), 16, kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
  EXPECT_FALSE(
      edge_bytecode_cache::DecodeSidecar(encoded.data(), encoded.size() - 1, kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));

  auto corrupted = encoded;
  corrupted.back() ^= 0x01;
  // The container only pins structure (payload_len); payload integrity is the
  // engine's job (V8 CachedData / the QuickJS provider's QJSB header), so a
  // flipped payload byte passes container validation on both engines.
  EXPECT_TRUE(
      edge_bytecode_cache::DecodeSidecar(corrupted.data(), corrupted.size(), kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, DecodeRejectsLengthFieldOverflow) {
  // A crafted header whose payload_len/tag_len fields would overflow the
  // additive structural check must be rejected without reading out of bounds.
  const auto payload = SamplePayload();
  const auto base = edge_bytecode_cache::EncodeSidecar(
      kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1, payload.data(), payload.size());
  size_t off = 0;
  size_t len = 0;

  auto put_u64 = [](std::vector<uint8_t>* buf, size_t pos, uint64_t value) {
    for (int i = 0; i < 8; ++i) (*buf)[pos + i] = static_cast<uint8_t>(value >> (8 * i));
  };
  auto put_u32 = [](std::vector<uint8_t>* buf, size_t pos, uint32_t value) {
    for (int i = 0; i < 4; ++i) (*buf)[pos + i] = static_cast<uint8_t>(value >> (8 * i));
  };

  // payload_len (offset 32) set so kHeaderSize + tag_len + payload_len wraps
  // back to the real size; the additive check must still reject it.
  auto bad_payload_len = base;
  put_u64(&bad_payload_len, 32, ~static_cast<uint64_t>(0));
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(bad_payload_len.data(), bad_payload_len.size(),
                                                  kTag, kSource,
                                                  edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));

  // tag_len (offset 40) larger than the buffer must reject before the tag
  // memcmp reads out of bounds.
  auto bad_tag_len = base;
  put_u32(&bad_tag_len, 40, 0xFFFFFFFFu);
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(bad_tag_len.data(), bad_tag_len.size(), kTag,
                                                  kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                                  &off, &len));
}

TEST_F(Test7BytecodeCachePhase05, ShapeFlagsCrossRejected) {
  const auto payload = SamplePayload();
  size_t off = 0;
  size_t len = 0;

  const auto cjs = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagCjsFunctionV1,
                                                      payload.data(), payload.size());
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(cjs.data(), cjs.size(), kTag, kSource,
                                                  edge_bytecode_cache::kFlagEsmModuleV1, &off, &len));

  const auto esm = edge_bytecode_cache::EncodeSidecar(kTag, kSource, edge_bytecode_cache::kFlagEsmModuleV1,
                                                      payload.data(), payload.size());
  EXPECT_FALSE(edge_bytecode_cache::DecodeSidecar(esm.data(), esm.size(), kTag, kSource,
                                                  edge_bytecode_cache::kFlagCjsFunctionV1, &off, &len));
  EXPECT_TRUE(edge_bytecode_cache::DecodeSidecar(esm.data(), esm.size(), kTag, kSource,
                                                 edge_bytecode_cache::kFlagEsmModuleV1, &off, &len));
  ASSERT_EQ(len, payload.size());
  EXPECT_EQ(std::vector<uint8_t>(esm.begin() + off, esm.begin() + off + len), payload);
}


TEST_F(Test7BytecodeCachePhase05, SidecarSuffixMatchesProvider) {
#if defined(EDGE_BUNDLED_NAPI_V8)
  EXPECT_STREQ(edge_bytecode_cache::SidecarSuffix(), ".v8b");
#elif defined(EDGE_NAPI_QUICKJS)
  EXPECT_STREQ(edge_bytecode_cache::SidecarSuffix(), ".qjsb");
#else
  GTEST_SKIP() << "no bundled engine";
#endif
}

TEST_F(Test7BytecodeCachePhase05, SidecarPathUsesEdgecacheSubdirKeepingFullFilename) {
  const std::string& tag = edge_bytecode_cache::EngineFileTag();
  if (tag.empty()) GTEST_SKIP() << "no bundled engine";

  // The short file tag is "<engine>-<major>-<minor>" — no dots, so it stays a
  // single clean filename component (e.g. v8-13-6 / qjs-0-14).
  EXPECT_EQ(tag.find('.'), std::string::npos) << tag;
#if defined(EDGE_BUNDLED_NAPI_V8)
  EXPECT_EQ(tag.rfind("v8-", 0), 0u) << tag;
#elif defined(EDGE_NAPI_QUICKJS)
  EXPECT_EQ(tag.rfind("qjs-", 0), 0u) << tag;
#endif

  const std::string path = edge_bytecode_cache::SidecarPathForSource("/foo/bar/app.js");
  // Caches go in a per-directory __edgecache__ subdir, keyed by the full source
  // filename (extension included) + the short engine tag.
  EXPECT_NE(path.find("/foo/bar/__edgecache__/"), std::string::npos) << path;
  EXPECT_EQ(path, "/foo/bar/__edgecache__/app.js." + tag + ".jsc");

  // The full filename keys the entry, so ".js"/".mjs" siblings never collide.
  EXPECT_NE(edge_bytecode_cache::SidecarPathForSource("/x/app.js"),
            edge_bytecode_cache::SidecarPathForSource("/x/app.mjs"));
  EXPECT_EQ(edge_bytecode_cache::SidecarPathForSource("/x/app.mjs"),
            "/x/__edgecache__/app.mjs." + tag + ".jsc");

  // A relative/bare source keeps the cache next to it (empty parent dir).
  EXPECT_EQ(edge_bytecode_cache::SidecarPathForSource("main.mjs"),
            "__edgecache__/main.mjs." + tag + ".jsc");
  // Compound names keep every component.
  EXPECT_EQ(edge_bytecode_cache::SidecarPathForSource("/x/a.test.js"),
            "/x/__edgecache__/a.test.js." + tag + ".jsc");
}

#if !defined(_WIN32)

TEST_F(Test7BytecodeCachePhase05, WriteOnFirstRunThenConsume) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_first_run");
  project.Write("lib.js", "module.exports = { value: 21 };\n");
  const fs::path main_path =
      project.Write("main.js", "const lib = require('./lib.js');\nconsole.log(lib.value * 2);\n");
  const fs::path sidecar = edge_bytecode_cache::SidecarPathForSource(main_path.string());

  auto disabled = RunEdge(edge_path, {"--no-bytecode-cache", main_path.string()}, project.root());
  EXPECT_EQ(disabled.exit_code, 0) << disabled.stderr_output;
  EXPECT_EQ(disabled.stdout_output, "42\n");
  EXPECT_FALSE(fs::exists(sidecar)) << "--no-bytecode-cache must not write sidecars";

  auto first = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root(),
                       "EDGE_BYTECODE_CACHE_TRACE=1");
  EXPECT_EQ(first.exit_code, 0) << first.stderr_output;
  EXPECT_EQ(first.stdout_output, "42\n");
  EXPECT_TRUE(fs::exists(sidecar)) << "first run should write a sidecar; stderr=" << first.stderr_output;
  EXPECT_NE(first.stderr_output.find("write"), std::string::npos) << first.stderr_output;

  auto second = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root(),
                        "EDGE_BYTECODE_CACHE_TRACE=1");
  EXPECT_EQ(second.exit_code, 0) << second.stderr_output;
  EXPECT_EQ(second.stdout_output, "42\n");
  EXPECT_NE(second.stderr_output.find("hit"), std::string::npos)
      << "second run should consume the sidecar; stderr=" << second.stderr_output;
}

TEST_F(Test7BytecodeCachePhase05, PrecompileWritesSidecarsWithoutExecuting) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_precompile");
  project.Write("side_effect.js", "process.exitCode = 3; console.log('EXECUTED');\n");
  const fs::path main_path = project.Write("main.js", "console.log('ok');\n");

  auto precompiled = RunEdge(edge_path, {"--precompile", project.root().string()}, project.root());
  EXPECT_EQ(precompiled.exit_code, 0) << precompiled.stderr_output;
  EXPECT_EQ(precompiled.stdout_output.find("EXECUTED"), std::string::npos)
      << "--precompile must not execute module bodies";

  EXPECT_TRUE(fs::exists(edge_bytecode_cache::SidecarPathForSource(main_path.string())));
  EXPECT_TRUE(fs::exists(edge_bytecode_cache::SidecarPathForSource(
      (project.root() / "side_effect.js").string())));

  auto run = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root(), "EDGE_BYTECODE_CACHE_TRACE=1");
  EXPECT_EQ(run.exit_code, 0) << run.stderr_output;
  EXPECT_EQ(run.stdout_output, "ok\n");
  EXPECT_NE(run.stderr_output.find("hit"), std::string::npos) << run.stderr_output;
}

TEST_F(Test7BytecodeCachePhase05, CorruptedAndStaleSidecarsFallBack) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_corrupt");
  const fs::path main_path = project.Write("main.js", "console.log('first');\n");
  const fs::path sidecar = edge_bytecode_cache::SidecarPathForSource(main_path.string());

  auto first = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root());
  ASSERT_EQ(first.exit_code, 0) << first.stderr_output;
  ASSERT_TRUE(fs::exists(sidecar));

  // Corrupt the payload: flip the last byte.
  {
    std::fstream f(sidecar, std::ios::binary | std::ios::in | std::ios::out);
    f.seekg(0, std::ios::end);
    const auto size = static_cast<long long>(f.tellg());
    ASSERT_GT(size, 0);
    f.seekg(size - 1);
    char last = 0;
    f.read(&last, 1);
    last = static_cast<char>(last ^ 0x01);
    f.seekp(size - 1);
    f.write(&last, 1);
  }
  auto corrupted = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root());
  EXPECT_EQ(corrupted.exit_code, 0) << corrupted.stderr_output;
  EXPECT_EQ(corrupted.stdout_output, "first\n") << "corrupted sidecar must fall back to source";

  // Stale: edit the source, leave the (now mismatched) sidecar in place.
  project.Write("main.js", "console.log('second');\n");
  auto stale = RunEdge(edge_path, {"--bytecode-cache", main_path.string()}, project.root());
  EXPECT_EQ(stale.exit_code, 0) << stale.stderr_output;
  EXPECT_EQ(stale.stdout_output, "second\n") << "stale sidecar must fall back to the new source";
}

TEST_F(Test7BytecodeCachePhase05, CheckModeWritesNoSidecars) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_check");
  const fs::path main_path = project.Write("main.js", "console.log('never');\n");
  const fs::path sidecar = edge_bytecode_cache::SidecarPathForSource(main_path.string());

  auto checked = RunEdge(edge_path, {"--check", main_path.string()}, project.root());
  EXPECT_EQ(checked.exit_code, 0) << checked.stderr_output;
  EXPECT_FALSE(fs::exists(sidecar)) << "--check must not write sidecars";
}

TEST_F(Test7BytecodeCachePhase05, PrecompileConflictsAndValidation) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_conflicts");

  auto no_paths = RunEdge(edge_path, {"--precompile"}, project.root());
  EXPECT_EQ(no_paths.exit_code, 9) << no_paths.stderr_output;

  auto with_check = RunEdge(edge_path, {"--precompile", "--check", "x.js"}, project.root());
  EXPECT_EQ(with_check.exit_code, 9) << with_check.stderr_output;

  auto with_disable =
      RunEdge(edge_path, {"--precompile", "--no-bytecode-cache", project.root().string()}, project.root());
  EXPECT_EQ(with_disable.exit_code, 9) << with_disable.stderr_output;
}

TEST_F(Test7BytecodeCachePhase05, EsmSyntaxFilesGetModuleShapeSidecars) {
  const auto edge_path = ResolveBuiltEdgeBinary();
  ASSERT_FALSE(edge_path.empty()) << "Failed to resolve built edge binary";

  TempProjectDir project("edge_bytecode_cache_esm");
  project.Write("esm.js", "export const x = 1;\n");
  project.Write("ok.js", "module.exports = 1;\n");
  project.Write("mod.mjs", "export const y = 2;\n");

  auto result = RunEdge(edge_path, {"--precompile", project.root().string()}, project.root());
  EXPECT_EQ(result.exit_code, 0) << result.stderr_output;
  EXPECT_TRUE(fs::exists(
      edge_bytecode_cache::SidecarPathForSource((project.root() / "ok.js").string())));
  EXPECT_TRUE(fs::exists(
      edge_bytecode_cache::SidecarPathForSource((project.root() / "esm.js").string())))
      << "ESM-syntax .js should be precompiled with the module shape";
  EXPECT_TRUE(fs::exists(
      edge_bytecode_cache::SidecarPathForSource((project.root() / "mod.mjs").string())));
}

#endif  // !defined(_WIN32)
